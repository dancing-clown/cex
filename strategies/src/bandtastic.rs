use ta::{
    indicators::{BollingerBands, ExponentialMovingAverage, MoneyFlowIndex, RelativeStrengthIndex},
    Next,
};
use ta::DataItem; // The struct that already implements these traits
use std::collections::VecDeque;
use cex_core::SimpleKLine;
use cex_core::structure::Signal;
use cex_core::structure::Position;
use cex_core::structure::Direction;
use cex_core::structure::ExitReason;
use tracing::error;

#[derive(Clone)]
pub struct BandtasticStrategy {
    // Buy parameters
    buy_rsi_threshold: f64,
    buy_mfi_threshold: f64,
    buy_rsi_enabled: bool,
    buy_mfi_enabled: bool,
    buy_ema_enabled: bool,
    buy_trigger: String,
    
    // Sell parameters
    sell_rsi_threshold: f64,
    sell_mfi_threshold: f64,
    sell_rsi_enabled: bool,
    sell_mfi_enabled: bool,
    sell_ema_enabled: bool,
    sell_trigger: String,
    
    // ROI and stop parameters
    min_roi: Vec<(usize, f64)>, // (minutes, percentage)
    stoploss: f64,
    trailing_stop: bool,
    trailing_stop_positive: f64,
    trailing_stop_positive_offset: f64,
    trailing_only_offset_is_reached: bool,
    
    // Indicators
    rsi: RelativeStrengthIndex,
    mfi: MoneyFlowIndex,
    bb1: BollingerBands,
    bb2: BollingerBands,
    bb3: BollingerBands,
    bb4: BollingerBands,
    buy_fast_ema: ExponentialMovingAverage,
    buy_slow_ema: ExponentialMovingAverage,
    sell_fast_ema: ExponentialMovingAverage,
    sell_slow_ema: ExponentialMovingAverage,
    
    // State
    position: Option<Position>,
    bars_since_entry: usize,
    bar_index: usize,
    price_history: VecDeque<f64>,
}

impl BandtasticStrategy {
    pub fn new(
        buy_fast_ema_period: usize,
        buy_slow_ema_period: usize,
        buy_rsi_threshold: f64,
        buy_mfi_threshold: f64,
        buy_rsi_enabled: bool,
        buy_mfi_enabled: bool,
        buy_ema_enabled: bool,
        buy_trigger: String,
        sell_fast_ema_period: usize,
        sell_slow_ema_period: usize,
        sell_rsi_threshold: f64,
        sell_mfi_threshold: f64,
        sell_rsi_enabled: bool,
        sell_mfi_enabled: bool,
        sell_ema_enabled: bool,
        sell_trigger: String,
    ) -> Self {
        // Initialize indicators with default periods (can be adjusted)
        let rsi_period = 14;
        let mfi_period = 14;
        let bb_period = 20;
        
        BandtasticStrategy {
            buy_rsi_threshold,
            buy_mfi_threshold,
            buy_rsi_enabled,
            buy_mfi_enabled,
            buy_ema_enabled,
            buy_trigger,
            sell_rsi_threshold,
            sell_mfi_threshold,
            sell_rsi_enabled,
            sell_mfi_enabled,
            sell_ema_enabled,
            sell_trigger,
            
            // ROI table (minutes, percentage)
            min_roi: vec![
                (0, 0.162),
                (69, 0.097),
                (229, 0.061),
                (566, 0.0),
            ],
            stoploss: -0.345,
            trailing_stop: true,
            trailing_stop_positive: 0.01,
            trailing_stop_positive_offset: 0.058,
            trailing_only_offset_is_reached: false,
            
            // Indicators
            rsi: RelativeStrengthIndex::new(rsi_period).unwrap(),
            mfi: MoneyFlowIndex::new(mfi_period).unwrap(),
            bb1: BollingerBands::new(bb_period, 1.0).unwrap(),
            bb2: BollingerBands::new(bb_period, 2.0).unwrap(),
            bb3: BollingerBands::new(bb_period, 3.0).unwrap(),
            bb4: BollingerBands::new(bb_period, 4.0).unwrap(),
            buy_fast_ema: ExponentialMovingAverage::new(buy_fast_ema_period).unwrap(),
            buy_slow_ema: ExponentialMovingAverage::new(buy_slow_ema_period).unwrap(),
            sell_fast_ema: ExponentialMovingAverage::new(sell_fast_ema_period).unwrap(),
            sell_slow_ema: ExponentialMovingAverage::new(sell_slow_ema_period).unwrap(),
            
            // State
            position: None,
            bars_since_entry: 0,
            bar_index: 0,
            price_history: VecDeque::new(),
        }
    }
    
    pub fn next(&mut self, kline: SimpleKLine) -> Option<Signal> {
        if kline.interval != "15m" {
            error!("非法的K线间隔,请检查行情输入");
            return None;
        }
        let (open, high, low, close, volume) = (kline.open, kline.high, kline.low, kline.close, kline.volume);
        self.bar_index += 1;
        
         // Create a DataItem that implements all required traits
        let data_item = DataItem::builder()
            .open(open)
            .high(high)
            .low(low)
            .close(close)
            .volume(volume)
            .build()
            .unwrap();
        // Update indicators with current bar data
        let hlc3 = (high + low + close) / 3.0;
        
        let rsi = self.rsi.next(close);
        let mfi = self.mfi.next(&data_item);
        
        let bb1 = self.bb1.next(hlc3);
        let bb2 = self.bb2.next(hlc3);
        let bb3 = self.bb3.next(hlc3);
        let bb4 = self.bb4.next(hlc3);
        
        let buy_fast_ema_value = self.buy_fast_ema.next(close);
        let buy_slow_ema_value = self.buy_slow_ema.next(close);
        let sell_fast_ema_value = self.sell_fast_ema.next(close);
        let sell_slow_ema_value = self.sell_slow_ema.next(close);
        
        // Store price for trailing stop calculation
        self.price_history.push_back(close);
        if self.price_history.len() > 100 {
            self.price_history.pop_front();
        }
        
        // Update position tracking
        if let Some(position) = &mut self.position {
            self.bars_since_entry = self.bar_index - position.entry_bar_index;
        } else {
            self.bars_since_entry = 0;
        }
        
        // Generate signals
        let mut signal = None;
        
        // Buy conditions
        let buy_condition1 = !self.buy_rsi_enabled || (rsi < self.buy_rsi_threshold);
        let buy_condition2 = !self.buy_mfi_enabled || (mfi < self.buy_mfi_threshold);
        let buy_condition3 = !self.buy_ema_enabled || (buy_fast_ema_value > buy_slow_ema_value);
        
        let buy_condition4 = match self.buy_trigger.as_str() {
            "bb_lower1" => close < bb1.lower,
            "bb_lower2" => close < bb2.lower,
            "bb_lower3" => close < bb3.lower,
            "bb_lower4" => close < bb4.lower,
            _ => false,
        };
        
        let buy_condition5 = volume > 0.0;
        
        let buy_signal = buy_condition1 && buy_condition2 && buy_condition3 && buy_condition4 && buy_condition5;
        
        // Sell conditions
        let sell_condition1 = !self.sell_rsi_enabled || (rsi > self.sell_rsi_threshold);
        let sell_condition2 = !self.sell_mfi_enabled || (mfi > self.sell_mfi_threshold);
        let sell_condition3 = !self.sell_ema_enabled || (sell_fast_ema_value < sell_slow_ema_value);
        
        let sell_condition4 = match self.sell_trigger.as_str() {
            "sell-bb_upper1" => close > bb1.upper,
            "sell-bb_upper2" => close > bb2.upper,
            "sell-bb_upper3" => close > bb3.upper,
            "sell-bb_upper4" => close > bb4.upper,
            _ => false,
        };
        
        let sell_condition5 = volume > 0.0;
        
        let sell_signal = sell_condition1 && sell_condition2 && sell_condition3 && sell_condition4 && sell_condition5;
        
        // Check ROI exits
        if let Some(position) = &self.position {
            for (minutes, roi_percentage) in &self.min_roi {
                // Assuming 15 minutes per bar (adjust according to your timeframe)
                let bars_needed = minutes / 15;
                if self.bars_since_entry >= bars_needed {
                    let target_price = position.price * (1.0 + roi_percentage);
                    if close >= target_price {
                        signal = Some(Signal::Exit {
                            reason: ExitReason::Roi(*minutes, *roi_percentage),
                            price: close,
                        });
                        break;
                    }
                }
            }
        }
        
        // Check stop loss
        if let Some(position) = &self.position {
            let stop_loss_price = position.price * (1.0 + self.stoploss);
            if close <= stop_loss_price {
                signal = Some(Signal::Exit {
                    reason: ExitReason::StopLoss,
                    price: close,
                });
            }
        }
        
        // Check trailing stop
        if self.trailing_stop && self.position.is_some() {
            let position = self.position.as_ref().unwrap();
            let trail_offset = position.price * self.trailing_stop_positive_offset;
            let trail_activation = position.price * (1.0 + self.trailing_stop_positive);
            
            if !self.trailing_only_offset_is_reached || close > trail_activation {
                if let Some(highest_price) = self.price_history.iter().max_by(|a, b| a.partial_cmp(b).unwrap()) {
                    let trail_price = highest_price - trail_offset;
                    if close <= trail_price {
                        signal = Some(Signal::Exit {
                            reason: ExitReason::TrailingStop,
                            price: close,
                        });
                    }
                }
            }
        }
        
        // Generate entry signals only if we don't have a position
        if self.position.is_none() && buy_signal {
            signal = Some(Signal::Enter {
                direction: Direction::Long,
                price: close,
            });
        }
        
        // Generate exit signal if we have a position and sell conditions are met
        if self.position.is_some() && sell_signal {
            signal = Some(Signal::Exit {
                reason: ExitReason::StopProfit,
                price: close,
            });
        }
        
        // Update position based on signal
        if let Some(signal) = &signal {
            match signal {
                Signal::Enter { direction:_, price } => {
                    self.position = Some(Position {
                        price: *price,
                        entry_bar_index: self.bar_index,
                        size: 1.0, // Assuming full position size
                    });
                },
                Signal::Exit { .. } => {
                    self.position = None;
                },
            }
        }
        
        signal
    }
}