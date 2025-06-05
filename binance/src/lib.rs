use std::collections::BTreeMap;

use cex_core::{ChannelMsg, Ping, SimpleKLine};

use crossbeam::channel::Sender;

use anyhow::Result;
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};
use tracing::{debug, error, info, warn};


/*
{
    "stream": "btcusdt@kline_1m",
    "data": {
        "e": "kline",
        "E": 1748877604023,
        "s": "BTCUSDT",
        "k": {
            "t": 1748877600000,
            "T": 1748877659999,
            "s": "BTCUSDT",
            "i": "1m",
            "f": 4978109970,
            "L": 4978110557,
            "o": "104349.06000000",
            "c": "104380.96000000",
            "h": "104380.96000000",
            "l": "104349.06000000",
            "v": "10.32405000",
            "n": 588,
            "x": false,
            "q": "1077392.54360710",
            "V": "10.27943000",
            "Q": "1072735.25781810",
            "B": "0"
        }
    }
}
*/
#[derive(Debug, Serialize, Deserialize, Clone)]
struct BNKStreamFrame {
    stream: String,
    data: BNKlineData,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BNKlineData {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "E")]
    event_time: i64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "k")]
    kline: BNKline,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BNKline {
    #[serde(rename = "t")]
    start_time: i64,
    #[serde(rename = "T")]
    end_time: i64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "i")]
    interval: String,
    #[serde(rename = "o")]
    open: String,
    #[serde(rename = "c")]
    close: String,
    #[serde(rename = "h")]
    high: String,
    #[serde(rename = "l")]
    low: String,
    #[serde(rename = "v")]
    volume: String,
    #[serde(rename = "n")]
    number_of_trades: i32,
    #[serde(rename = "x")]
    is_closed: bool,
}

/// (code, interval), sender
/// ("btcusdt", "1m")
pub async fn subscribe_binance(pair_list: Vec<(String, String)>, tx: Sender<ChannelMsg>) {
    info!("subscribe to binance: {:?}", pair_list);
    // let pair_list = pair_list.iter().map(|(symbol, interval)| (symbol.to_string(), interval.to_string())).collect::<Vec<(String, String)>>();
    loop { // 出错自动重连， binance 24h 会断开连接
        if let Err(e) = connect_binance(pair_list.clone(), tx.clone()).await {
            error!("Failed to connect to Binance: {}", e);
        }
    }
}

async fn connect_binance(pair_list: Vec<(String, String)>, tx: Sender<ChannelMsg>) -> anyhow::Result<()> {
    // 用组合流 stream
    let url = format!("wss://stream.binance.com:9443/stream");
    let (mut ws_stream, _) = connect_async(url).await?;
    info!("Connected to Binance");

    let subs = json!({
        "method": "SUBSCRIBE",
        "params": pair_list.iter().map(|(symbol, interval)| format!("{}@kline_{}", symbol, interval)).collect::<Vec<String>>(),
        "id": 1
    });

    ws_stream.send(Message::Text(subs.to_string())).await?;
    info!("Subscribed to Binance");
    
    handle_websocket_stream(ws_stream, tx).await?;

    Ok(())
}

async fn handle_websocket_stream<S>(
    mut ws_stream: WebSocketStream<S>,
    tx: Sender<ChannelMsg>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut m = BTreeMap::new();
    let mut cnt = 0usize;
    while let Some(message) = ws_stream.next().await {
        match message {
            Ok(Message::Text(text)) => match serde_json::from_str::<BNKStreamFrame>(&text) {
                Ok(frame) => {
                    let kline_data = frame.data;
                    debug!(
                        "收到完整K线数据 - Symbol: {}, Time: {}, Open: {}, Close: {}, High: {}, Low: {}, Volume: {}",
                        kline_data.symbol,
                        kline_data.event_time,
                        kline_data.kline.open,
                        kline_data.kline.close,
                        kline_data.kline.high,
                        kline_data.kline.low,
                        kline_data.kline.volume
                    );
                    let symbol = kline_data.symbol.clone();
                    let index = match m.get(&symbol) {
                        Some(v) => *v,
                        None => {
                            cnt += 1;
                            m.insert(symbol, cnt);
                            cnt
                        },
                    };
                    
                    // 只有当K线周期结束时才发送数据
                    if kline_data.kline.is_closed {
                        if let Err(e) = tx.try_send(ChannelMsg::Kline((index, kline_data.into()))) {
                            error!("Failed to handle kline data: {}", e);
                        }
                    }
                }
                Err(_) => {
                    warn!("ignore msg: {}", text);
                }
            },
            Ok(Message::Ping(ping)) => {
                info!("收到Ping消息");
                ws_stream.send(Message::Pong(ping)).await?;
                if let Err(e) = tx.try_send(ChannelMsg::Ping(Ping::new("binance".to_string(), Utc::now().timestamp_millis()))) {
                    error!("Failed to send ping message: {}", e);
                }
            }
            Err(e) => {
                error!("Error receiving message: {}", e);
                break;
            }
            _ => {
                info!("收到其他类型消息");
            }
        }
    }

    Ok(())
}

impl From<BNKlineData> for SimpleKLine {
    fn from(kline_data: BNKlineData) -> Self {
        let open_time_dt = Utc.timestamp_opt(kline_data.kline.start_time / 1000, 0)
            .single()
            .map(|dt| dt.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap()))
            .unwrap_or_else(|| Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap()));
        SimpleKLine {
            exchange: "binance".to_string(),
            symbol: kline_data.symbol,
            open_time_ms: kline_data.kline.start_time as u64,
            close_time_ms: kline_data.kline.end_time as u64,
            open_time_h: open_time_dt.format("%Y%m%d-%H:%M").to_string(),
            interval: kline_data.kline.interval.clone(),
            open: kline_data.kline.open.parse().unwrap_or(0.0),
            high: kline_data.kline.high.parse().unwrap_or(0.0),
            low: kline_data.kline.low.parse().unwrap_or(0.0),
            close: kline_data.kline.close.parse().unwrap_or(0.0),
            volume: kline_data.kline.volume.parse().unwrap_or(0.0),
            // quote_volume: 0.0, // Binance API 没有直接提供这个字段
            trades_count: kline_data.kline.number_of_trades as u64,
        }
    }
}