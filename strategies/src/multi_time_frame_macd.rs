use ta::indicators::MovingAverageConvergenceDivergence;
use ta::Next;
use std::collections::VecDeque;
use cex_core::SimpleKLine;
use cex_core::structure::{Signal, Position, Direction, ExitReason};
use tracing::error;

/// Multi-timeframe MACD strategy with breakeven stop loss optimization
#[derive(Clone)]
pub struct MultiTimeFrameMacdStrategy {
    short_trend_time: String,
    long_trend_time: String, // Time frame for long-term trend analysis (e.g., "240" for 4-hour)

    // Stop loss and take profit parameters
    stop_loss_perc: f64,      // Initial stop loss percentage
    // TODO: 此部分逻辑未实现
    take_profit_perc: f64,    // Initial take profit percentage
    breakeven_threshold: f64, // Percentage at which breakeven is triggered
    trail_offset: f64,        // Trail offset after breakeven

    // MACD indicators for different time frames
    macd_4h: MovingAverageConvergenceDivergence,
    macd_1h: MovingAverageConvergenceDivergence,

    // State variables
    position: Option<Position>,
    entry_price: Option<f64>,
    breakeven_activated: bool,
    bar_index: usize,
    price_history: VecDeque<f64>,
}

impl MultiTimeFrameMacdStrategy {
    pub fn new(
        fast_length: usize, // 12
        slow_length: usize, // 26
        signal_length: usize,   // 9
        short_trend_time: String,   // "60m"
        long_trend_time: String,    // "240m"
        stop_loss_perc: f64,        // 1.9
        take_profit_perc: f64,      // 5.4
        breakeven_threshold: f64,   // 1.0
        trail_offset: f64,          // 0.5
    ) -> Self {
        MultiTimeFrameMacdStrategy {
            short_trend_time,
            long_trend_time,
            stop_loss_perc,
            take_profit_perc,
            breakeven_threshold,
            trail_offset,
            // Initialize MACD indicators for different time frames
            macd_4h: MovingAverageConvergenceDivergence::new(fast_length, slow_length, signal_length).unwrap(),
            macd_1h: MovingAverageConvergenceDivergence::new(fast_length, slow_length, signal_length).unwrap(),
            // Initialize state variables
            position: None,
            entry_price: None,
            breakeven_activated: false,
            bar_index: 0,
            price_history: VecDeque::new(),
        }
    }
}

impl MultiTimeFrameMacdStrategy {
    pub fn next(&mut self, kline: SimpleKLine) -> Option<Signal> {
        // Skip if the kline interval is not supported
        if kline.interval != "60m" && kline.interval != self.long_trend_time {
            error!("Unsupported kline interval: {}, need short term: {},need long term: {}", kline.interval, self.short_trend_time, self.long_trend_time);
            return None;
        }
        let close = kline.close;

        // 只有小周期才增加bar_index
        if kline.interval == "60m" {
            self.bar_index += 1;
        }

        let mut macd_4h = f64::NAN;
        let mut signal_4h = f64::NAN;
        let mut hist_4h  = f64::NAN;
        let mut macd_1h = f64::NAN;
        let mut signal_1h = f64::NAN;
        let mut hist_1h  = f64::NAN;
        // Update indicators based on the time frame
        match kline.interval.as_str() {
            "240m" => {
                // Update 4-hour MACD indicators
                let output = self.macd_4h.next(close);
                macd_4h = output.macd;
                signal_4h = output.signal;
                hist_4h = output.histogram;
            },
            "60m" => {
                // Update 1-hour MACD indicators
                let output = self.macd_1h.next(close);
                macd_1h = output.macd;
                signal_1h = output.signal;
                hist_1h = output.histogram;
            },
            _ => { /* Ignore other time frames */ }
        }

        // Store price for trailing stop calculation
        self.price_history.push_back(close);
        if self.price_history.len() > 100 {
            self.price_history.pop_front();
        }

        // Trend determination (long-term trend analysis using 4H chart)
        let is_long_trend = !hist_4h.is_nan() && (macd_4h > signal_4h || hist_4h > 0.0);
        let is_short_trend = !hist_4h.is_nan() && (macd_4h < signal_4h || hist_4h < 0.0);

        // Entry signals (based on 1H chart)
        let long_entry = is_long_trend && (macd_1h > signal_1h && hist_1h > 0.0);
        let short_entry = is_short_trend && (macd_1h < signal_1h && hist_1h < 0.0);

        // Exit signals (based on 4H MACD)
        let long_exit = !hist_4h.is_nan() && macd_4h < signal_4h;
        let short_exit = !hist_4h.is_nan() && macd_4h > signal_4h;

        // Track entry price and manage breakeven activation
        if let Some(position) = &self.position {
            if self.entry_price.is_none() {
                self.entry_price = Some(position.price);
            }

            // Check if we've reached the breakeven threshold
            if let Some(entry_price) = self.entry_price {
                let long_position = position.size > 0.0;
                
                if long_position && !self.breakeven_activated {
                    if close >= entry_price * (1.0 + self.breakeven_threshold / 100.0) {
                        // Activate breakeven
                        self.breakeven_activated = true;
                    }
                } else if !long_position && !self.breakeven_activated {
                    if close <= entry_price * (1.0 - self.breakeven_threshold / 100.0) {
                        // Activate breakeven
                        self.breakeven_activated = true;
                    }
                }
            }
        }

        // Generate exit signals based on dynamic conditions
        let mut signal = None;

        // Check for exit conditions first
        if self.breakeven_activated && self.position.is_some() && self.entry_price.is_some() {
            let entry_price = self.entry_price.unwrap();
            let trail_stop_price = if self.breakeven_activated {
                if entry_price > 0.0 {
                    entry_price * (1.0 + self.trail_offset / 100.0)
                } else {
                    0.0
                }
            } else {
                entry_price * (1.0 - self.stop_loss_perc / 100.0)
            };
            

            // Regular stop loss
            if let Some(position) = &self.position {
                if position.size > 0.0 && close <= trail_stop_price {
                    signal = Some(Signal::Exit {
                        reason: ExitReason::TrailingStop,
                        price: close,
                    });
                } else if position.size < 0.0 && close >= trail_stop_price {
                    signal = Some(Signal::Exit {
                        reason: ExitReason::TrailingStop,
                        price: close,
                    });
                }
            }
        }

        // Check for trend reversal exits
        if let Some(position) = &self.position {
            if position.size > 0.0 && long_exit {
                signal = Some(Signal::Exit {
                    reason: ExitReason::StopProfit,
                    price: close,
                });
            } else if position.size < 0.0 && short_exit {
                signal = Some(Signal::Exit {
                    reason: ExitReason::StopProfit,
                    price: close,
                });
            }
        }

        // Generate entry signals only if we don't have a position
        if self.position.is_none() {
            if long_entry {
                signal = Some(Signal::Enter {
                    direction: Direction::Long,
                    price: close,
                });
            } else if short_entry {
                signal = Some(Signal::Enter {
                    direction: Direction::Short,
                    price: close,
                });
            }
        }

        // Update position based on signal
        if let Some(signal) = &signal {
            match signal {
                Signal::Enter { direction, price } => {
                    let size = match *direction {
                        Direction::Long => 1.0,
                        Direction::Short => -1.0,
                        _ => panic!("Invalid direction"),
                    };
                    self.position = Some(Position {
                        price: *price,
                        entry_bar_index: self.bar_index,
                        size, // Assuming full position size
                    });
                    self.entry_price = Some(*price);
                    self.breakeven_activated = false;
                },
                Signal::Exit { .. } => {
                    self.position = None;
                },
            }
        }

        signal
    }
}