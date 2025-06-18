use strategies::MultiTimeFrameMacdStrategy;

fn main() {
    // Example parameters for the MultiTimeFrameMacdStrategy
    let fast_length = 12;
    let slow_length = 26;
    let signal_length = 9;
    let short_trend_time = "60m".to_string();
    let long_trend_time = "240m".to_string();
    let stop_loss_perc = 1.9;
    let take_profit_perc = 5.4;
    let breakeven_threshold = 1.0;
    let trail_offset = 0.5;

    // Create a new instance of the MultiTimeFrameMacdStrategy
    let mut _strategy = MultiTimeFrameMacdStrategy::new(
        fast_length,
        slow_length,
        signal_length,
        short_trend_time,
        long_trend_time,
        stop_loss_perc,
        take_profit_perc,
        breakeven_threshold,
        trail_offset,
    );
}