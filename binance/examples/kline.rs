use cex_core::{writer::{create_writer, FileWriterConfig, WriterType}, ChannelMsg};
use binance::subscribe_binance;
use serde::Deserialize;
use tracing::info;
use std::{path::PathBuf, fs};
use tracing_subscriber::fmt::format::FmtSpan;

// 配置
#[derive(Debug, Deserialize)]
struct Config {
    output_dir: String,
    sub_list: Vec<(String, String)>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .init();

    let config = toml::from_str::<Config>(&fs::read_to_string("sub.toml")?)?;
    
    // 确保数据目录存在
    let data_dir = PathBuf::from(config.output_dir);
    fs::create_dir_all(&data_dir)?;


    // 配置文件写入器
    let writer_type = WriterType::File(FileWriterConfig {
        base_path: data_dir,
        rotation_interval: 8 * 3600, // 8小时轮转一次
    });

    let pair_list = config.sub_list;
    let (tx, rx) = crossbeam::channel::bounded(pair_list.len());
    tokio::spawn(async move { subscribe_binance(pair_list, tx).await });

    let writer = create_writer(writer_type)?;
    info!("开始写入K线数据");
    while let Ok(msg) = rx.recv() {
        match msg {
            ChannelMsg::Kline(kline) => {
                writer.write(&kline).await?;
                writer.flush().await?;
                info!("写入到文件: {:?}", kline);
            }
            _ => {}
        };
    }

    Ok(())
} 