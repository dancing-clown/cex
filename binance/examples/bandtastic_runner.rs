use cex_core::{
    structure::{Direction, Position, Signal, Trade},
    writer::{create_writer, FileWriterConfig, WriterType},
    CexError, ChannelMsg, Ping, SimpleKLine
};
use binance::subscribe_binance;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tracing::{error, info};
use std::{path::PathBuf, fs};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_appender;
use strategies::{bandtastic::BandtasticStrategy, Strategy};

// 配置
#[derive(Debug, Deserialize)]
struct Config {
    output_dir: String,
    webhook_url: Vec<String>,
    sub_list: Vec<(String, String)>,
}

enum BoardcastMsg {
    Ping(Ping),
    Trade(SimpleKLine, Trade),
    Error(CexError),
}


#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let file_appender = tracing_appender::rolling::RollingFileAppender::new(
        tracing_appender::rolling::Rotation::DAILY,
        "logs",
        "bandtastic.log",
    );

    tracing_subscriber::fmt()
        .with_writer(file_appender)
        .with_span_events(FmtSpan::CLOSE)
        .with_ansi(false)
        .init();

    let config = toml::from_str::<Config>(&fs::read_to_string("sub.toml")?)?;

    let params = json!({
        "buy_fast_ema_period": 20,
        "buy_slow_ema_period": 40,
        "buy_rsi_threshold": 50.0,
        "buy_mfi_threshold": 30.0,
        "buy_rsi_enabled": true,
        "buy_mfi_enabled": true,
        "buy_ema_enabled": true,
        "buy_trigger": "bb_lower1",
        "sell_fast_ema_period": 7,
        "sell_slow_ema_period": 6,
        "sell_rsi_threshold": 57.0,
        "sell_mfi_threshold": 46.0,
        "sell_rsi_enabled": false,
        "sell_mfi_enabled": true,
        "sell_ema_enabled": true,
        "sell_trigger": "sell-bb_upper2",
    });
    let strategy: BandtasticStrategy = serde_json::from_value(params).unwrap();

    // 确保数据目录存在
    let data_dir = PathBuf::from(config.output_dir);
    fs::create_dir_all(&data_dir)?;

    let boardcast = async |signal: serde_json::Value| -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        for url in config.webhook_url.clone() {
            let res = client.post(url.clone())
                .json(&signal)
                .send()
                .await?;
            info!("Boardcast to {}: {:?}", url, res);
        }
        Ok(())
    };


    let pair_list = config.sub_list;
    let p_len: usize = pair_list.len();
    let (tx, rx) = crossbeam::channel::bounded(p_len);
    tokio::spawn(async move { subscribe_binance(pair_list, tx).await });

    let (bd_tx, bd_rx) = crossbeam::channel::bounded(p_len);

    // index 0 不存在, 需要多创建一个
    let mut strategies = (0..p_len + 1).map(|_| {strategy.clone()}).collect::<Vec<BandtasticStrategy>>();
    let mut trades = (0..p_len + 1).map(|_| {Trade::default()}).collect::<Vec<Trade>>();


    let st_rx = rx.clone();
    std::thread::spawn(move || {
        info!("开始计算策略");
        while let Ok(msg) = st_rx.recv() {
            match msg {
                ChannelMsg::Kline((index, kline)) => {
                    // 如果产生信号，需要根据当前的trade情况来进行判断
                    if let Some(signal) = strategies[index].next(kline.clone()) {
                        match signal {
                            Signal::Enter { direction, price } => {
                                if trades[index].enter_position.is_some() {
                                    error!("入场时已有持仓，该策略不支持重复入场");
                                    continue;
                                }
                                trades[index].exchange = kline.exchange.clone();
                                trades[index].symbol = kline.symbol.clone();
                                trades[index].direction = direction;
                                trades[index].enter_position = Some(Position {
                                    price,
                                    entry_bar_index: 0,
                                    size: 1.0,
                                });
                                trades[index].enter_time = kline.close_time_ms as i64;
                                bd_tx.send(BoardcastMsg::Trade(kline, trades[index].clone())).unwrap();
                            }
                            Signal::Exit { reason, price } => {
                                if trades[index].enter_position.is_none() {
                                    error!("暂未入场，不处理该信号");
                                    continue;
                                }
                                // 更新交易方向
                                match trades[index].direction {
                                    Direction::Long => {
                                        trades[index].direction = Direction::LongClose;
                                    },
                                    Direction::Short => {
                                        trades[index].direction = Direction::ShortClose;
                                    },
                                    _ => {},
                                }
                                trades[index].exit_position = Some(Position {
                                    price: price,
                                    entry_bar_index: 0,
                                    size: 1.0,
                                });
                                trades[index].exit_reason = reason;
                                trades[index].exit_time = kline.close_time_ms as i64;
                                trades[index].calculate();
                                bd_tx.send(BoardcastMsg::Trade(kline, trades[index].clone())).unwrap();
                                // 出场后重置交易信息
                                trades[index] = Trade::default();
                            }
                        }
                    }
                }
                ChannelMsg::Ping(ping) => {
                    bd_tx.send(BoardcastMsg::Ping(ping)).unwrap();
                },
                ChannelMsg::Error(error) => {
                    bd_tx.send(BoardcastMsg::Error(error)).unwrap();
                },
            }
        }
    });

    boardcast(json!({
        "策略名": "Bandtastic Strategy",
        "消息": "开始计算策略",
        "当前时间": Utc::now().timestamp_millis(),
    })).await?;

    while let Ok(msg) = bd_rx.recv() {
        let now_ts = Utc::now().timestamp_millis();
        let dt = chrono::DateTime::from_timestamp_millis(now_ts)
            .unwrap()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap());
        match msg {
            BoardcastMsg::Trade(kline, trade) => {
                let msg = json!({
                    "策略名": "Bandtastic Strategy",
                    "标的名": kline.symbol,
                    "交易信息":  format!("{:?}", trade),
                    "当前时间": dt.format("%Y%m%d-%H:%M.%S").to_string(),
                });
                info!("Signal generated: {:?}", msg);
                if let Err(e)  = boardcast(msg).await {
                    error!("Failed to boardcast signal: {:?}", e);
                }
            },
            BoardcastMsg::Ping(ping) => {
                let msg = json!({
                    "策略名": "Bandtastic Strategy",
                    "ping": ping,
                    "当前时间": dt.format("%Y%m%d-%H:%M.%S").to_string(),
                });
                if let Err(e)  = boardcast(msg).await {
                    error!("Failed to boardcast signal: {:?}", e);
                }
            },
            BoardcastMsg::Error(error) => {
                error!("Error: {:?}", error);
            }
        };
    }

    // 配置文件写入器
    let writer_type = WriterType::File(FileWriterConfig {
        base_path: data_dir,
        rotation_interval: 8 * 3600, // 8小时轮转一次
    });
    
    let writer = create_writer(writer_type)?;
    info!("开始写入K线数据");
    while let Ok(msg) = rx.recv() {
        match msg {
            ChannelMsg::Kline(kline) => {
                writer.write(&kline).await?;
                writer.flush().await?;
            },
            _ => {}
        };
    }

    Ok(())
} 