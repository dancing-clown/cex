use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod writer;

#[derive(Debug, Error)]
pub enum CexError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// 简单K线数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleKLine {
    /// 交易所
    pub exchange: String,
    /// 交易对
    pub symbol: String,
    /// 开盘时间戳（毫秒）
    pub open_time_ms: u64,
    /// 收盘时间戳（毫秒）
    pub close_time_ms: u64,
    /// 开盘时间戳（易读）: 20250601-20:01
    pub open_time_h: String,
    /// 时间间隔
    pub interval: String,
    /// 开盘价
    pub open: f64,
    /// 最高价
    pub high: f64,
    /// 最低价
    pub low: f64,
    /// 收盘价
    pub close: f64,
    /// 交易量
    pub volume: f64,
    /// 交易额
    // pub quote_volume: f64,
    /// 交易笔数
    pub trades_count: u64,
}

impl SimpleKLine {
    /// 创建新的K线数据
    pub fn new(
        exchange: &str,
        symbol: &str,
        open_time: u64,
        close_time: u64,
        interval: KlineInterval,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        // quote_volume: f64,
        trades_count: u64,
    ) -> Self {
        // 将时间戳转换为UTC+8时区的易读格式
        let open_time_h = {
            let dt = chrono::DateTime::from_timestamp_millis(open_time as i64)
                .unwrap();
            // 转换成utc+8
            let dt = dt.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap());
            dt.format("%Y%m%d-%H:%M").to_string()
        };

        Self {
            open_time_ms: open_time,
            close_time_ms: close_time,
            open_time_h,
            interval: interval.as_str().to_string(),
            open,
            high,
            low,
            close,
            volume,
            // quote_volume,
            trades_count,
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
        }
    }
} 

#[derive(Debug, Clone)]
pub enum KlineInterval {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    FourHours,
    OneDay,
}

impl KlineInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            KlineInterval::OneMinute => "1m",
            KlineInterval::FiveMinutes => "5m",
            KlineInterval::FifteenMinutes => "15m",
            KlineInterval::ThirtyMinutes => "30m",
            KlineInterval::OneHour => "1h",
            KlineInterval::FourHours => "4h",
            KlineInterval::OneDay => "1d",
        }
    }
}