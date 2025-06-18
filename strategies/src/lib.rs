pub mod bandtastic;
// Add new strategies here
mod multi_time_frame_macd;

pub use bandtastic::BandtasticStrategy;
// Re-export new strategy types
pub use multi_time_frame_macd::MultiTimeFrameMacdStrategy;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
