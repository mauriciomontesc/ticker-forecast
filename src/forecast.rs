use std::collections::HashSet;

use crate::indicators;
use crate::models::{
    Direction, Forecast, ForecastHorizon, IndicatorSignals, MlContribution, PriceData,
};

const RSI_PERIOD: usize = 14;
const ADX_PERIOD: usize = 14;
const MIN_DATA_POINTS: usize = 35;
/// Long-term horizon in trading sessions (~1 month).
pub const LONG_TERM_HORIZON_DAYS: usize = 21;
/// The trend is fitted on `LONG_TERM_WINDOW_FACTOR × horizon` candles (≈3 months).
const LONG_TERM_WINDOW_FACTOR: usize = 3;
/// Minimum projected return over the horizon to call a direction (0.005 = 0.5%).
const LONG_TERM_MIN_RETURN: f64 = 0.005;

/// Indicators plus the weighted bullish/bearish vote.
pub struct TechnicalSignals {
    pub indicators: IndicatorSignals,
    pub bullish: f64,
    pub bearish: f64,
}

/// Computes every indicator and the weighted vote once, so the short- and
/// long-term analyzers can share the same snapshot.
pub fn technical_signals(data: &PriceData) -> TechnicalSignals {
    let (macd_line, signal_line) = indicators::compute_macd(&data.closes);
    let macd_bullish = match (macd_line, signal_line) {
        (Some(m), Some(s)) => m > s,
        _ => false,
    };
    let histogram = (macd_line.unwrap_or(0.0) - signal_line.unwrap_or(0.0)).abs();
    let macd_weight = histogram / (1.0 + histogram);

    let rsi = indicators::compute_rsi(&data.closes, RSI_PERIOD).unwrap_or(50.0);
    let rsi_bullish = rsi > 50.0;
    let rsi_weight = (rsi - 50.0).abs() / 50.0;

    let adx =
        indicators::compute_adx(&data.highs, &data.lows, &data.closes, ADX_PERIOD).unwrap_or(0.0);
    let trending = adx > 25.0;

    let bollinger_b = indicators::bollinger_b_percent(&data.closes, 20, 2.0).unwrap_or(0.5);
    let bollinger_bw = indicators::bollinger_bandwidth(&data.closes, 20, 2.0).unwrap_or(0.0);

    let stochastic_k = indicators::stochastic(&data.highs, &data.lows, &data.closes, 14).unwrap_or(50.0);

    let (supports, resistances) = indicators::support_resistance(&data.highs, &data.lows, 10, 0.005);
    let latest = *data.closes.last().unwrap_or(&0.0);
    let near_support = indicators::nearest_level_below(&supports, latest);
    let near_resistance = indicators::nearest_level_above(&resistances, latest);
    let nearest_support = near_support;
    let nearest_resistance = near_resistance;

    let signals = IndicatorSignals {
        macd_bullish,
        macd_line: macd_line.unwrap_or(0.0),
        signal_line: signal_line.unwrap_or(0.0),
        rsi,
        rsi_bullish,
        adx,
        trending,
        bollinger_b,
        bollinger_bandwidth: bollinger_bw,
        stochastic_k,
        nearest_support,
        nearest_resistance,
    };

    let mut bullish = 0.0;
    let mut bearish = 0.0;
    if macd_bullish {
        bullish += macd_weight;
    } else {
        bearish += macd_weight;
    }
    if rsi_bullish {
        bullish += rsi_weight;
    } else {
        bearish += rsi_weight;
    }

    let bb_weight = (bollinger_b - 0.5).abs() * 0.4;
    if bollinger_b < 0.5 {
        bullish += bb_weight;
    } else {
        bearish += bb_weight;
    }

    let stoch_weight = (stochastic_k - 50.0).abs() / 50.0 * 0.3;
    if stochastic_k < 50.0 {
        bullish += stoch_weight;
    } else {
        bearish += stoch_weight;
    }

    if let Some(sup) = nearest_support {
        let dist = (latest - sup) / latest;
        if dist < 0.02 { bullish += 0.4; }
        else if dist < 0.05 { bullish += 0.2; }
    }
    if let Some(res) = nearest_resistance {
        let dist = (res - latest) / latest;
        if dist < 0.02 { bearish += 0.4; }
        else if dist < 0.05 { bearish += 0.2; }
    }

    TechnicalSignals {
        indicators: signals,
        bullish,
        bearish,
    }
}

/// Number of candles that cover ~1 trading month for a given Yahoo Finance
/// interval. Intraday sessions are assumed to last 6.5 hours. Used as a
/// fallback when the data cadence cannot be measured.
pub fn horizon_bars(interval: &str) -> usize {
    let bars_per_day: f64 = match interval {
        "1m" => 390.0,
        "2m" => 195.0,
        "5m" => 78.0,
        "15m" => 26.0,
        "30m" => 13.0,
        "60m" | "1h" => 6.5,
        "90m" => 26.0 / 6.0,
        "5d" | "1wk" => 1.0 / 5.0,
        "1mo" => 1.0 / LONG_TERM_HORIZON_DAYS as f64,
        _ => 1.0, // daily bars and unknown intervals
    };
    ((LONG_TERM_HORIZON_DAYS as f64 * bars_per_day).ceil() as usize).max(1)
}

/// Number of candles covering ~1 trading month. For intraday and daily data it
/// is derived from the actual cadence (24h forex sessions and 6.5h equity
/// sessions both map to ~1 month); longer intervals use the static table.
pub fn horizon_bars_for(data: &PriceData, interval: &str) -> usize {
    if matches!(interval, "5d" | "1wk" | "1mo") {
        return horizon_bars(interval);
    }
    let fallback = horizon_bars(interval);
    if data.timestamps.len() != data.closes.len() || data.closes.len() < 2 {
        return fallback;
    }
    let days: HashSet<i64> = data
        .timestamps
        .iter()
        .map(|ts| ts.div_euclid(86_400))
        .collect();
    if days.len() < 2 {
        return fallback;
    }
    let bars_per_day = data.closes.len() as f64 / days.len() as f64;
    let estimate = (LONG_TERM_HORIZON_DAYS as f64 * bars_per_day).ceil() as usize;
    estimate.max(1)
}

fn decide(
    bullish: f64,
    bearish: f64,
    trending: bool,
    ml_contrib: MlContribution,
) -> (Direction, String) {
    let direction = if bullish > bearish {
        Direction::Bullish
    } else if bearish > bullish {
        Direction::Bearish
    } else {
        Direction::Neutral
    };

    let is_high = matches!(ml_contrib, MlContribution::High);

    let strength = match (direction, trending, is_high) {
        (Direction::Bullish, true, true) => "ALTA confianza: tendencia alcista con ML".into(),
        (Direction::Bullish, true, false) => "tendencia alcista, señales alineadas".into(),
        (Direction::Bullish, false, true) => "ALTA confianza: señal alcista con ML".into(),
        (Direction::Bullish, false, false) => "señal debil (ADX bajo, mercado lateral)".into(),
        (Direction::Bearish, true, true) => "ALTA confianza: tendencia bajista con ML".into(),
        (Direction::Bearish, true, false) => "tendencia bajista, señales alineadas".into(),
        (Direction::Bearish, false, true) => "ALTA confianza: señal bajista con ML".into(),
        (Direction::Bearish, false, false) => "señal debil (ADX bajo, mercado lateral)".into(),
        (Direction::Neutral, _, _) => "sin consenso entre indicadores".into(),
    };

    (direction, strength)
}

/// Least-squares slope of a series over time (units per candle).
fn linear_regression_slope(values: &[f64]) -> f64 {
    let n = values.len();
    if n < 2 {
        return 0.0;
    }
    let x_mean = (n - 1) as f64 / 2.0;
    let y_mean = values.iter().sum::<f64>() / n as f64;
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (i, &y) in values.iter().enumerate() {
        let x = i as f64 - x_mean;
        numerator += x * (y - y_mean);
        denominator += x * x;
    }
    if denominator == 0.0 { 0.0 } else { numerator / denominator }
}

/// Mean log return per candle of the close series.
fn log_slope(closes: &[f64]) -> f64 {
    let logs: Vec<f64> = closes.iter().map(|c| c.max(1e-12).ln()).collect();
    linear_regression_slope(&logs)
}

#[allow(dead_code)]
pub fn analyze(data: &PriceData) -> Option<Forecast> {
    let technical = technical_signals(data);
    analyze_short(data, &technical, None, None)
}

/// Short-term forecast (next candle) combining the technical vote and ML.
pub fn analyze_short(
    data: &PriceData,
    technical: &TechnicalSignals,
    ml_bullish: Option<bool>,
    ml_accuracy: Option<f64>,
) -> Option<Forecast> {
    if data.closes.len() < MIN_DATA_POINTS {
        return None;
    }
    let latest_price = data.latest_close()?;

    let mut bullish = technical.bullish;
    let mut bearish = technical.bearish;

    let ml_contrib = match ml_accuracy {
        Some(acc) if acc > 0.60 => {
            let w = if acc > 0.70 { 3.0 } else { 2.0 };
            if ml_bullish == Some(true) {
                bullish += w;
            } else {
                bearish += w;
            }
            MlContribution::High
        }
        Some(acc) if acc >= 0.50 => {
            if ml_bullish == Some(true) {
                bullish += 1.0;
            } else {
                bearish += 1.0;
            }
            MlContribution::Low
        }
        _ => MlContribution::Ignored,
    };

    let (direction, strength) = decide(bullish, bearish, technical.indicators.trending, ml_contrib);
    Some(Forecast {
        direction,
        strength,
        signals: technical.indicators.clone(),
        latest_price,
        ml_used: ml_contrib != MlContribution::Ignored,
        horizon: ForecastHorizon::Short,
        projected_price: None,
    })
}

/// Long-term forecast (~1 month) from a log-linear trend fitted on the most
/// recent `LONG_TERM_WINDOW_FACTOR × horizon` candles.
pub fn analyze_long_term(
    data: &PriceData,
    technical: &TechnicalSignals,
    horizon: usize,
) -> Option<Forecast> {
    let required = MIN_DATA_POINTS.max(horizon.saturating_mul(LONG_TERM_WINDOW_FACTOR));
    if horizon == 0 || data.closes.len() < required {
        return None;
    }
    let latest_price = data.latest_close()?;

    let window = horizon * LONG_TERM_WINDOW_FACTOR;
    let recent = &data.closes[data.closes.len() - window..];
    let slope = log_slope(recent);
    let projected_return = (slope * horizon as f64).exp() - 1.0;

    let direction = if projected_return > LONG_TERM_MIN_RETURN {
        Direction::Bullish
    } else if projected_return < -LONG_TERM_MIN_RETURN {
        Direction::Bearish
    } else {
        Direction::Neutral
    };

    let pct = projected_return * 100.0;
    let strength = match direction {
        Direction::Bullish => format!("tendencia alcista: +{pct:.2}%"),
        Direction::Bearish => format!("tendencia bajista: {pct:.2}%"),
        Direction::Neutral => format!("sin tendencia clara: {pct:.2}%"),
    };

    Some(Forecast {
        direction,
        strength,
        signals: technical.indicators.clone(),
        latest_price,
        ml_used: false,
        horizon: ForecastHorizon::Long,
        projected_price: Some(latest_price * (1.0 + projected_return)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_data(closes: Vec<f64>) -> PriceData {
        let n = closes.len();
        PriceData {
            timestamps: (0..n).map(|i| i as i64).collect(),
            highs: closes.iter().map(|&c| c + 1.0).collect(),
            lows: closes.iter().map(|&c| c - 1.0).collect(),
            volume: vec![1000; n],
            closes,
        }
    }

    #[test]
    fn test_compute_signals_returns_f64_scores() {
        let data = make_data((0..50).map(|i| 100.0 + (i as f64).sin() * 10.0).collect());
        let technical = technical_signals(&data);
        assert!(technical.bullish >= 0.0 && technical.bearish >= 0.0);
        assert!((technical.bullish + technical.bearish) > 0.0);
    }

    #[test]
    fn test_decide_bullish_wins() {
        let (dir, _) = decide(1.5, 0.3, true, MlContribution::Low);
        assert_eq!(dir, Direction::Bullish);
    }

    #[test]
    fn test_decide_bearish_wins() {
        let (dir, _) = decide(0.2, 1.8, false, MlContribution::High);
        assert_eq!(dir, Direction::Bearish);
    }

    #[test]
    fn test_decide_tie() {
        let (dir, _) = decide(0.5, 0.5, false, MlContribution::Ignored);
        assert_eq!(dir, Direction::Neutral);
    }

    #[test]
    fn test_decide_high_ml_with_trend() {
        let (_, strength) = decide(2.0, 0.0, true, MlContribution::High);
        assert!(strength.contains("ALTA"));
        assert!(strength.contains("ML"));
    }

    #[test]
    fn test_analyze_with_ml_high_confidence() {
        let data = make_data((0..50).map(|i| 100.0 + (i as f64).sin() * 10.0).collect());
        let technical = technical_signals(&data);
        let result = analyze_short(&data, &technical, Some(true), Some(0.75));
        assert!(result.is_some());
        let f = result.unwrap();
        assert!(f.ml_used);
    }

    #[test]
    fn test_analyze_with_ml_low_accuracy_ignored() {
        let data = make_data((0..50).map(|i| 100.0 + (i as f64).sin() * 10.0).collect());
        let technical = technical_signals(&data);
        let result = analyze_short(&data, &technical, Some(true), Some(0.40));
        assert!(result.is_some());
        let f = result.unwrap();
        assert!(!f.ml_used);
    }

    #[test]
    fn test_analyze_with_ml_insufficient_data() {
        let data = make_data(vec![100.0; 20]);
        let technical = technical_signals(&data);
        let result = analyze_short(&data, &technical, Some(true), Some(0.75));
        assert!(result.is_none());
    }

    #[test]
    fn test_short_term_forecast_horizon_is_short() {
        let data = make_data((0..50).map(|i| 100.0 + (i as f64).sin() * 10.0).collect());
        let technical = technical_signals(&data);
        let forecast = analyze_short(&data, &technical, None, None).unwrap();
        assert_eq!(forecast.horizon, ForecastHorizon::Short);
        assert!(forecast.projected_price.is_none());
    }

    #[test]
    fn test_long_term_horizon_is_one_month() {
        assert!(LONG_TERM_HORIZON_DAYS >= 21);
    }

    #[test]
    fn test_horizon_bars_daily_is_one_month() {
        assert_eq!(horizon_bars("1d"), LONG_TERM_HORIZON_DAYS);
    }

    #[test]
    fn test_horizon_bars_intraday_scales() {
        assert_eq!(horizon_bars("1h"), 137);
        assert_eq!(horizon_bars("30m"), 273);
        assert!(horizon_bars("30m") > horizon_bars("1h"));
        assert!(horizon_bars("1h") > horizon_bars("1d"));
    }

    #[test]
    fn test_horizon_bars_unknown_defaults_to_daily() {
        assert_eq!(horizon_bars("weird"), LONG_TERM_HORIZON_DAYS);
    }

    fn make_daily_data(n: usize) -> PriceData {
        let mut data = make_data((0..n).map(|i| 100.0 + i as f64 * 0.1).collect());
        data.timestamps = (0..n as i64).map(|i| i * 86_400).collect();
        data
    }

    #[test]
    fn test_horizon_bars_for_daily_data_is_one_month() {
        let data = make_daily_data(100);
        assert_eq!(horizon_bars_for(&data, "1d"), LONG_TERM_HORIZON_DAYS);
    }

    #[test]
    fn test_horizon_bars_for_24h_forex_scales_to_month() {
        // 22 contiguous 24h sessions of hourly candles.
        let n = 22 * 24;
        let mut data = make_data((0..n).map(|i| 1.0 + i as f64 * 1e-5).collect());
        data.timestamps = (0..n).map(|i| i as i64 * 3_600).collect();
        assert_eq!(horizon_bars_for(&data, "1h"), LONG_TERM_HORIZON_DAYS * 24);
    }

    #[test]
    fn test_horizon_bars_for_falls_back_without_cadence() {
        let data = make_data(vec![100.0; 10]);
        assert_eq!(horizon_bars_for(&data, "1h"), horizon_bars("1h"));
    }

    #[test]
    fn test_analyze_long_term_returns_none_for_insufficient_data() {
        let data = make_data(vec![100.0; 20]);
        let technical = technical_signals(&data);
        assert!(analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS).is_none());
    }

    #[test]
    fn test_analyze_long_term_requires_data_for_horizon() {
        let data = make_data((0..100).map(|i| 100.0 + i as f64 * 0.1).collect());
        let technical = technical_signals(&data);
        assert!(analyze_long_term(&data, &technical, horizon_bars("1h")).is_none());
    }

    #[test]
    fn test_analyze_long_term_bullish_on_uptrend() {
        let data = make_data((0..100).map(|i| 100.0 + i as f64 * 0.05).collect());
        let technical = technical_signals(&data);
        let result = analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS)
            .expect("uptrend must produce a long-term forecast");
        assert_eq!(result.direction, Direction::Bullish);
        assert_eq!(result.horizon, ForecastHorizon::Long);
        assert!(result.projected_price.is_some());
    }

    #[test]
    fn test_analyze_long_term_bearish_on_downtrend() {
        let data = make_data((0..100).map(|i| 100.0 - i as f64 * 0.05).collect());
        let technical = technical_signals(&data);
        let result = analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS)
            .expect("downtrend must produce a long-term forecast");
        assert_eq!(result.direction, Direction::Bearish);
        assert_eq!(result.horizon, ForecastHorizon::Long);
    }

    #[test]
    fn test_analyze_long_term_neutral_on_flat_market() {
        let data = make_data(vec![100.0; 100]);
        let technical = technical_signals(&data);
        let result = analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS)
            .expect("flat market must produce a long-term forecast");
        assert_eq!(result.direction, Direction::Neutral);
    }

    #[test]
    fn test_long_term_projection_uses_log_returns() {
        let growth = 0.01;
        let data = make_data((0..100).map(|i| 100.0 * (growth * i as f64).exp()).collect());
        let technical = technical_signals(&data);
        let forecast = analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS).unwrap();

        let expected_return = (growth * LONG_TERM_HORIZON_DAYS as f64).exp() - 1.0;
        assert!(forecast
            .strength
            .contains(&format!("+{:.2}%", expected_return * 100.0)));

        let expected_price = 100.0 * (growth * 99.0).exp() * (1.0 + expected_return);
        let projected = forecast.projected_price.unwrap();
        assert!((projected - expected_price).abs() / expected_price < 1e-6);
    }

    #[test]
    fn test_long_term_uses_recent_window_not_full_history() {
        let mut closes: Vec<f64> = (0..100).map(|i| 100.0 + i as f64).collect();
        closes.extend((0..100).map(|i| 199.0 - i as f64));
        let data = make_data(closes);
        let technical = technical_signals(&data);
        let forecast = analyze_long_term(&data, &technical, LONG_TERM_HORIZON_DAYS).unwrap();
        // Full history is roughly symmetric (flat); the recent window is falling.
        assert_eq!(forecast.direction, Direction::Bearish);
    }

    #[test]
    fn test_linear_regression_slope_linear_series() {
        let closes: Vec<f64> = (0..10).map(|i| 100.0 + i as f64 * 2.0).collect();
        assert!((linear_regression_slope(&closes) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_linear_regression_slope_flat_series() {
        let closes = vec![100.0; 10];
        assert_eq!(linear_regression_slope(&closes), 0.0);
    }

    #[test]
    fn test_log_slope_exponential_series() {
        let closes: Vec<f64> = (0..10).map(|i| 100.0 * (0.02 * i as f64).exp()).collect();
        assert!((log_slope(&closes) - 0.02).abs() < 1e-12);
    }
}
