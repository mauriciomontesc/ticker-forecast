use crate::indicators;
use crate::models::{Direction, Forecast, IndicatorSignals, MlContribution, PriceData};

const RSI_PERIOD: usize = 14;
const ADX_PERIOD: usize = 14;
const MIN_DATA_POINTS: usize = 35;

fn compute_signals(data: &PriceData) -> (IndicatorSignals, f64, f64) {
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

    let adx = indicators::compute_adx(&data.highs, &data.lows, &data.closes, ADX_PERIOD).unwrap_or(0.0);
    let trending = adx > 25.0;

    let signals = IndicatorSignals {
        macd_bullish,
        macd_line: macd_line.unwrap_or(0.0),
        signal_line: signal_line.unwrap_or(0.0),
        rsi,
        rsi_bullish,
        adx,
        trending,
    };

    let mut bullish = 0.0;
    let mut bearish = 0.0;
    if macd_bullish { bullish += macd_weight; } else { bearish += macd_weight; }
    if rsi_bullish { bullish += rsi_weight; } else { bearish += rsi_weight; }

    (signals, bullish, bearish)
}

fn decide(bullish: f64, bearish: f64, trending: bool, ml_contrib: MlContribution) -> (Direction, String) {
    let direction = if bullish > bearish { Direction::Bullish }
                    else if bearish > bullish { Direction::Bearish }
                    else { Direction::Neutral };

    let is_high = matches!(ml_contrib, MlContribution::High);

    let strength = match (direction, trending, is_high) {
        (Direction::Bullish, true, true) => "ALTA confianza: tendencia alcista con ML".into(),
        (Direction::Bullish, true, false) => "tendencia alcista, señales alineadas".into(),
        (Direction::Bullish, false, true) => "ALTA confianza: senhal alcista con ML".into(),
        (Direction::Bullish, false, false) => "senal debil (ADX bajo, mercado lateral)".into(),
        (Direction::Bearish, true, true) => "ALTA confianza: tendencia bajista con ML".into(),
        (Direction::Bearish, true, false) => "tendencia bajista, senhales alineadas".into(),
        (Direction::Bearish, false, true) => "ALTA confianza: senhal bajista con ML".into(),
        (Direction::Bearish, false, false) => "senal debil (ADX bajo, mercado lateral)".into(),
        (Direction::Neutral, _, _) => "sin consenso entre indicadores".into(),
    };

    (direction, strength)
}

#[allow(dead_code)]
pub fn analyze(data: &PriceData) -> Option<Forecast> {
    if data.closes.len() < MIN_DATA_POINTS {
        return None;
    }
    let latest_price = data.latest_close()?;
    let (signals, bullish, bearish) = compute_signals(data);
    let (direction, strength) = decide(bullish, bearish, signals.trending, MlContribution::Low);
    Some(Forecast { direction, strength, signals, latest_price, ml_used: false })
}

pub fn analyze_with_ml(
    data: &PriceData,
    ml_bullish: Option<bool>,
    ml_accuracy: Option<f64>,
) -> Option<Forecast> {
    if data.closes.len() < MIN_DATA_POINTS {
        return None;
    }
    let latest_price = data.latest_close()?;
    let (signals, mut bullish, mut bearish) = compute_signals(data);

    let ml_contrib = match ml_accuracy {
        Some(acc) if acc > 0.60 => {
            let w = if acc > 0.70 { 3.0 } else { 2.0 };
            if ml_bullish == Some(true) { bullish += w; } else { bearish += w; }
            MlContribution::High
        }
        Some(acc) if acc >= 0.50 => {
            if ml_bullish == Some(true) { bullish += 1.0; } else { bearish += 1.0; }
            MlContribution::Low
        }
        _ => MlContribution::Ignored,
    };

    let (direction, strength) = decide(bullish, bearish, signals.trending, ml_contrib);
    Some(Forecast { direction, strength, signals, latest_price, ml_used: ml_contrib != MlContribution::Ignored })
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
        let (_signals, bullish, bearish) = compute_signals(&data);
        assert!(bullish >= 0.0 && bearish >= 0.0);
        assert!((bullish + bearish) > 0.0);
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
        let result = analyze_with_ml(&data, Some(true), Some(0.75));
        assert!(result.is_some());
        let f = result.unwrap();
        assert!(f.ml_used);
    }

    #[test]
    fn test_analyze_with_ml_low_accuracy_ignored() {
        let data = make_data((0..50).map(|i| 100.0 + (i as f64).sin() * 10.0).collect());
        let result = analyze_with_ml(&data, Some(true), Some(0.40));
        assert!(result.is_some());
        let f = result.unwrap();
        assert!(!f.ml_used);
    }

    #[test]
    fn test_analyze_with_ml_insufficient_data() {
        let data = make_data(vec![100.0; 20]);
        let result = analyze_with_ml(&data, Some(true), Some(0.75));
        assert!(result.is_none());
    }
}
