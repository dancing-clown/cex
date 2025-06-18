use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Serialize, Deserialize)]
pub enum Signal {
    Enter {
        direction: Direction,
        price: f64,
    },
    Exit {
        reason: ExitReason,
        price: f64,
    },
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub enum Direction {
    #[default]
    None,
    Long,
    Short,
    LongClose,
    ShortClose,
}

impl fmt::Debug for Direction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Direction::Long => write!(f, "开多"),
            Direction::Short => write!(f, "开空"),
            Direction::LongClose => write!(f, "平多"),
            Direction::ShortClose => write!(f, "平空"),
            Direction::None => write!(f, "未知错误"),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub enum ExitReason {
    #[default]
    None,
    SellSignal,
    StopLoss,
    TrailingStop,
    Roi(usize, f64), // minutes, percentage
}

impl fmt::Debug for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ExitReason::SellSignal => write!(f, "止盈"),
            ExitReason::StopLoss => write!(f, "止损"),
            ExitReason::TrailingStop => write!(f, "动态止盈止损"),
            ExitReason::Roi(time, percentage) => write!(f, "投资回报率: {}分钟收益{}%", time, percentage * 100.0),
            _ => write!(f, "未知错误"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Position {
    pub price: f64,
    pub entry_bar_index: usize,
    pub size: f64,
}

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "持仓价格: {}, 持仓大小: {}, 入场K线位置: {}", self.price, self.size, self.entry_bar_index)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Trade {
    /// 交易所
    pub exchange: String,
    /// 交易对
    pub symbol: String,
    pub direction: Direction,
    pub enter_position: Option<Position>,
    pub exit_position: Option<Position>,
    pub enter_time: i64,
    pub exit_time: i64,
    pub exit_reason: ExitReason,
    pub roi: Option<f64>,
    pub fee: f64
}

impl Default for Trade {
    fn default() -> Self {
        Trade {
            exchange: "".to_string(),
            symbol: "".to_string(),
            direction: Direction::default(),
            enter_position: None,
            exit_position: None,
            enter_time: 0,
            exit_time: 0,
            exit_reason: ExitReason::default(),
            roi: None,
            fee: 0.06,
        }
    }
}

impl fmt::Debug for Trade {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "交易所:{}, 交易标的: {}, 方向: {:?}, 入场持仓: {:?}, 出场持仓: {:?}, 入场时间: {}, 退场时间: {}, 退出原因: {:?}, 回报率(百分比): {:?}",
               self.exchange, self.symbol, self.direction, self.enter_position, self.exit_position, self.enter_time, self.exit_time, self.exit_reason, self.roi)
    }
}

impl Trade {
    pub fn calculate(&mut self) {
        if self.enter_position.as_ref().is_some() && self.exit_position.as_ref().is_some() {
            match self.direction {
                Direction::Long => {
                    self.roi = Some((self.exit_position.as_ref().unwrap().price - self.enter_position.as_ref().unwrap().price) / self.enter_position.as_ref().unwrap().price * 100.0 - self.fee * 100.0);
                }
                Direction::Short => {
                    self.roi = Some((self.enter_position.as_ref().unwrap().price - self.exit_position.as_ref().unwrap().price) / self.enter_position.as_ref().unwrap().price * 100.0 - self.fee * 100.0);
                }
                _ => {}
            } 
        }
    }
}

