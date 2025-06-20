pub mod bandtastic;
// Add new strategies here
pub mod multi_time_frame_macd;

pub use bandtastic::BandtasticStrategy;
// Re-export new strategy types
pub use multi_time_frame_macd::MultiTimeFrameMacdStrategy;

use cex_core::{structure::Signal, SimpleKLine};

pub trait Strategy {
    fn next(&mut self, kline: SimpleKLine) -> Option<Signal>;
}