use std::path::PathBuf;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use zstd::Encoder;
use chrono::{DateTime, Utc, TimeZone};
use shared_memory::{ShmemConf, Shmem};
use anyhow::{Result, Context};
use serde::Serialize;
use tracing::{info, error, warn};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use std::sync::atomic::{AtomicUsize, Ordering};

// 文件写入器的配置
#[derive(Clone)]
pub struct FileWriterConfig {
    pub base_path: PathBuf,
    pub rotation_interval: i64,  // 文件轮转间隔（秒）
}

// 文件写入器
pub struct FileWriter {
    config: FileWriterConfig,
    current_file: Option<(PathBuf, Encoder<'static, File>)>,
    current_period_start: DateTime<Utc>,
    last_flush_time: DateTime<Utc>,
}

impl FileWriter {
    pub fn new(config: FileWriterConfig) -> Self {
        Self {
            config,
            current_file: None,
            current_period_start: Utc::now(),
            last_flush_time: Utc::now(),
        }
    }

    fn get_file_path(&self, timestamp: DateTime<Utc>) -> PathBuf {
        let period_start = timestamp.timestamp() / self.config.rotation_interval * self.config.rotation_interval;
        let period_start_dt = Utc.timestamp_opt(period_start, 0)
            .single()
            .unwrap_or_else(|| Utc::now())
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap());
            
        let filename = format!(
            "kline_{}.zst",
            period_start_dt.format("%Y%m%d-%H%M")
        );
        self.config.base_path.join(filename)
    }

    fn should_rotate_file(&self, timestamp: DateTime<Utc>) -> bool {
        if self.current_file.is_none() {
            return true;
        }
        
        let current_period = self.current_period_start.timestamp() / self.config.rotation_interval;
        let new_period = timestamp.timestamp() / self.config.rotation_interval;
        current_period != new_period
    }

    async fn rotate_file(&mut self, timestamp: DateTime<Utc>) -> Result<()> {
        if let Some((_, encoder)) = self.current_file.take() {
            // 先完成压缩
            let mut finished_encoder = encoder.finish().context("Failed to finish previous file")?;
            // 再刷新文件
            finished_encoder.flush().context("Failed to flush previous file")?;
        }

        let file_path = self.get_file_path(timestamp);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).context("Failed to create directory")?;
        }

        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)  // 使用追加模式
            .open(&file_path)
            .context("Failed to create/open file")?;
            
        let encoder = zstd::Encoder::new(file, 3).context("Failed to create zstd encoder")?;
        self.current_file = Some((file_path.clone(), encoder));
        self.current_period_start = timestamp;
        self.last_flush_time = Utc::now();
        
        info!("Rotated to new file: {:?}", file_path);
        Ok(())
    }

    async fn write<T: Serialize + Send + Sync>(&mut self, data: &T) -> Result<()> {
        let timestamp = Utc::now();
        
        if self.should_rotate_file(timestamp) {
            self.rotate_file(timestamp).await?;
        }

        if let Some((_path, encoder)) = &mut self.current_file {
            let json = serde_json::to_string(data).context("Failed to serialize data")?;
            writeln!(encoder, "{}", json).context("Failed to write to file")?;
        } else {
            error!("没有可用的文件句柄");
            return Err(anyhow::anyhow!("No file handle available"));
        }

        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        if let Some((path, encoder)) = self.current_file.take() {
            // 先完成压缩
            let mut finished_encoder = encoder.finish().context("Failed to finish encoder")?;
            // 再刷新文件
            finished_encoder.flush().context("Failed to flush file")?;
            
            // 创建新的编码器
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .append(true)
                .open(&path)
                .context("Failed to reopen file")?;
            
            let new_encoder = zstd::Encoder::new(file, 3).context("Failed to create new encoder")?;
            self.current_file = Some((path, new_encoder));
            self.last_flush_time = Utc::now();
        } else {
            warn!("没有文件需要flush");
        }
        Ok(())
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        if let Some((_, encoder)) = self.current_file.take() {
            match encoder.finish().and_then(|mut finished_encoder| finished_encoder.flush()) {
                Ok(_) => (),
                Err(e) => error!("Failed to properly close file: {}", e),
            }
        }
    }
}

// 共享内存写入器的配置
#[derive(Clone)]
pub struct ShmemWriterConfig {
    pub symbol: String,
    pub shmem_size: usize,
    pub shmem_name: String,
}

// 共享内存写入器
pub struct ShmemWriter {
    config: ShmemWriterConfig,
    shmem: Arc<Shmem>,
    write_pos: Arc<AtomicUsize>,
}

// 实现Send和Sync trait
unsafe impl Send for ShmemWriter {}
unsafe impl Sync for ShmemWriter {}

impl ShmemWriter {
    pub fn new(config: ShmemWriterConfig) -> Result<Self> {
        let shmem = ShmemConf::new()
            .size(config.shmem_size)
            .os_id(&config.shmem_name)
            .create()
            .context("Failed to create shared memory")?;

        Ok(Self {
            config,
            shmem: Arc::new(shmem),
            write_pos: Arc::new(AtomicUsize::new(0)),
        })
    }

    async fn write<T: Serialize + Send + Sync>(&self, data: &T) -> Result<()> {
        let json = serde_json::to_string(data).context("Failed to serialize data")?;
        let bytes = json.as_bytes();
        
        // 使用原子操作更新写入位置
        let mut current_pos = self.write_pos.load(Ordering::Relaxed);
        if current_pos + bytes.len() + 1 > self.config.shmem_size {
            current_pos = 0;
            self.write_pos.store(0, Ordering::Relaxed);
        }

        // 创建一个临时缓冲区
        let mut buffer = Vec::with_capacity(bytes.len() + 1);
        buffer.extend_from_slice(bytes);
        buffer.push(b'\n');

        // 一次性写入所有数据
        unsafe {
            std::ptr::copy_nonoverlapping(
                buffer.as_ptr(),
                self.shmem.as_ptr().add(current_pos),
                buffer.len()
            );
        }
        
        self.write_pos.store(current_pos + buffer.len(), Ordering::Relaxed);
        Ok(())
    }

    async fn flush(&self) -> Result<()> {
        // 共享内存不需要flush操作
        Ok(())
    }
}

// 内部写入器枚举
enum WriterInner {
    File(FileWriter),
    Shmem(ShmemWriter),
}

impl WriterInner {
    async fn write<T: Serialize + Send + Sync>(&mut self, data: &T) -> Result<()> {
        match self {
            WriterInner::File(w) => w.write(data).await,
            WriterInner::Shmem(w) => w.write(data).await,
        }
    }

    async fn flush(&mut self) -> Result<()> {
        match self {
            WriterInner::File(w) => w.flush().await,
            WriterInner::Shmem(w) => w.flush().await,
        }
    }
}

// 公开的写入器结构体
#[derive(Clone)]
pub struct Writer(Arc<TokioMutex<WriterInner>>);

impl Writer {
    fn new(inner: WriterInner) -> Self {
        Self(Arc::new(TokioMutex::new(inner)))
    }

    pub async fn write<T: Serialize + Send + Sync>(&self, data: &T) -> Result<()> {
        let mut inner = self.0.lock().await;
        inner.write(data).await
    }

    pub async fn flush(&self) -> Result<()> {
        let mut inner = self.0.lock().await;
        inner.flush().await
    }
}

// 工厂函数，用于创建不同类型的writer
#[derive(Clone)]
pub enum WriterType {
    File(FileWriterConfig),
    Shmem(ShmemWriterConfig),
}

pub fn create_writer(writer_type: WriterType) -> Result<Writer> {
    let inner = match writer_type {
        WriterType::File(config) => WriterInner::File(FileWriter::new(config)),
        WriterType::Shmem(config) => WriterInner::Shmem(ShmemWriter::new(config)?),
    };
    Ok(Writer::new(inner))
} 