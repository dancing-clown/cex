



pub async fn connect_multiple_kline_streams(
    config: KlineConfig,
    proxy: Option<ProxyConfig>,
    writer_type: WriterType,
) -> Result<()> {
    let mut handles = Vec::new();
    
    for interval in config.intervals {
        let symbol = config.symbol.clone();
        let proxy = proxy.clone();
        let writer_type = writer_type.clone();
        
        let handle = tokio::spawn(async move {
            let ws_url = format!(
                "wss://stream.binance.com:9443/ws/{}@kline_{}",
                symbol.to_lowercase(),
                interval.as_str()
            );
            
            info!("Connecting to Binance WebSocket: {}", ws_url);
            
            if let Some(proxy_config) = proxy {
                info!("Using proxy: {}:{}", proxy_config.host, proxy_config.port);
                
                // 测试代理连接
                let output = Command::new("curl")
                    .args(&[
                        "-x",
                        &format!("socks5h://{}:{}", proxy_config.host, proxy_config.port),
                        "https://api.binance.com/api/v3/time",
                        "-v"
                    ])
                    .output()
                    .context("Failed to execute curl command")?;
                    
                if !output.status.success() {
                    let error = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!("Proxy test failed: {}", error));
                }
                
                info!("Proxy test successful");
                
                // 设置系统代理
                unsafe {
                    std::env::set_var("ALL_PROXY", format!("socks5h://{}:{}", proxy_config.host, proxy_config.port));
                    std::env::set_var("HTTPS_PROXY", format!("socks5h://{}:{}", proxy_config.host, proxy_config.port));
                }
                
                let (stream, response) = connect_async(&ws_url).await.context("Failed to connect through proxy")?;
                info!("WebSocket connected successfully through proxy: {:?}", response);
                handle_websocket_stream(stream, symbol, writer_type).await?;
            } else {
                let (stream, _) = connect_async(&ws_url).await.context("Failed to connect directly")?;
                info!("WebSocket connected successfully");
                handle_websocket_stream(stream, symbol, writer_type).await?;
            }
            
            Ok::<(), anyhow::Error>(())
        });
        
        handles.push(handle);
    }
    
    // 等待所有任务完成
    for handle in handles {
        handle.await.context("Failed to join task")??;
    }
    
    Ok(())
}

#[derive(Debug, Clone)]
pub struct KlineConfig {
    pub symbol: String,
    pub intervals: Vec<KlineInterval>,
}

impl KlineConfig {
    pub fn new(symbol: impl Into<String>, intervals: Vec<KlineInterval>) -> Self {
        Self {
            symbol: symbol.into(),
            intervals,
        }
    }
}

pub async fn connect_kline_stream_with_timeout(
    symbol: &str,
    interval: KlineInterval,
    proxy: Option<ProxyConfig>,
    writer_type: WriterType,
    duration: Duration,
) -> Result<()> {
    let config = KlineConfig::new(
        symbol.to_string(),
        vec![interval]
    );
    
    timeout(
        duration,
        connect_multiple_kline_streams(config, proxy, writer_type)
    ).await.context("Connection timed out")?
}

pub async fn connect_kline_stream(symbol: &str, interval: KlineInterval, writer_type: WriterType) -> Result<()> {
    connect_kline_stream_with_proxy(symbol, interval, None, writer_type).await
}

pub async fn connect_kline_stream_with_proxy(
    symbol: &str,
    interval: KlineInterval,
    proxy: Option<ProxyConfig>,
    writer_type: WriterType,
) -> Result<()> {
    let config = KlineConfig::new(
        symbol.to_string(),
        vec![interval]
    );
    connect_multiple_kline_streams(config, proxy, writer_type).await
}



struct KlineHandler {
    _symbol: String,
    writer: Writer,
    current_kline_start_time: Option<i64>,
    cached_kline: Option<SimpleKLine>,
}

impl KlineHandler {
    fn new(symbol: String, writer_type: WriterType) -> Result<Self> {
        let writer = cex_core::writer::create_writer(writer_type)?;
        Ok(Self {
            _symbol: symbol,
            writer,
            current_kline_start_time: None,
            cached_kline: None,
        })
    }

    async fn handle_kline(&mut self, kline_data: &BNKlineData) -> Result<()> {
        let simple_kline = SimpleKLine::from(kline_data.clone());

        // 检查是否是新的一分钟
        if let Some(current_start_time) = self.current_kline_start_time {
            if current_start_time != kline_data.kline.start_time {
                // 如果是新的一分钟，写入之前缓存的数据（如果有的话）
                if let Some(cached_data) = self.cached_kline.take() {
                    self.writer.write(&cached_data).await?;
                    self.writer.flush().await?;
                }
            }
        }

        // 更新当前处理的K线开始时间和缓存数据
        self.current_kline_start_time = Some(kline_data.kline.start_time);
        self.cached_kline = Some(simple_kline);

        Ok(())
    }
}

// 确保KlineHandler是Send
unsafe impl Send for KlineHandler {}

