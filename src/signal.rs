use crate::indicators;
use crate::models::{
    Direction, Forecast, PriceData, SignalSide, StoredSignal, TradeSignal,
};

const ATR_PERIOD: usize = 14;
/// Half-width of the entry zone as a fraction of ATR.
const ENTRY_ATR_FACTOR: f64 = 0.5;
/// Fallback half-width (0.1% of price) when ATR is unavailable.
const FALLBACK_BAND_PCT: f64 = 0.001;

/// Builds an actionable signal from a forecast: direction maps to buy/sell and
/// the entry zone is an ATR-based band around the reference price.
pub fn build(
    symbol: &str,
    interval: &str,
    data: &PriceData,
    forecast: &Forecast,
    horizon_bars: usize,
) -> Option<TradeSignal> {
    let side = match forecast.direction {
        Direction::Bullish => SignalSide::Buy,
        Direction::Bearish => SignalSide::Sell,
        Direction::Neutral => return None,
    };

    let reference_price = forecast.latest_price;
    if !reference_price.is_finite() || reference_price <= 0.0 {
        return None;
    }
    let reference_ts = *data.timestamps.last()?;
    if horizon_bars == 0 {
        return None;
    }

    let band = indicators::compute_atr(&data.highs, &data.lows, &data.closes, ATR_PERIOD)
        .filter(|atr| atr.is_finite() && *atr > 0.0)
        .map(|atr| atr * ENTRY_ATR_FACTOR)
        .unwrap_or(reference_price * FALLBACK_BAND_PCT);

    let (entry_low, entry_high) = match side {
        SignalSide::Buy => (reference_price - band, reference_price),
        SignalSide::Sell => (reference_price, reference_price + band),
    };

    Some(TradeSignal {
        symbol: symbol.to_string(),
        interval: interval.to_string(),
        horizon: forecast.horizon,
        side,
        entry_low,
        entry_high,
        reference_price,
        reference_ts,
        horizon_bars,
    })
}

/// Evaluates a pending signal against newer candles.
///
/// Returns `(outcome, resolved_price)` with outcome `"win"`, `"loss"` or
/// `"missed"` (price never reached the entry zone). Returns `None` while the
/// horizon has not elapsed yet.
pub fn resolve(signal: &StoredSignal, data: &PriceData) -> Option<(String, f64)> {
    if data.closes.len() != data.timestamps.len() || signal.horizon_bars == 0 {
        return None;
    }
    let reference_idx = data
        .timestamps
        .iter()
        .rposition(|&ts| ts == signal.reference_ts)?;
    let exit_idx = reference_idx + signal.horizon_bars;
    if exit_idx >= data.closes.len() {
        return None;
    }

    let touched = (reference_idx + 1..=exit_idx).any(|i| match signal.side {
        SignalSide::Buy => data.lows[i] <= signal.entry_high,
        SignalSide::Sell => data.highs[i] >= signal.entry_low,
    });

    let exit_price = data.closes[exit_idx];
    if !touched {
        return Some(("missed".to_string(), exit_price));
    }

    let fill = (signal.entry_low + signal.entry_high) / 2.0;
    let won = match signal.side {
        SignalSide::Buy => exit_price > fill,
        SignalSide::Sell => exit_price < fill,
    };

    Some((if won { "win" } else { "loss" }.to_string(), exit_price))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ForecastHorizon, IndicatorSignals};

    fn make_data(closes: Vec<f64>, highs: Vec<f64>, lows: Vec<f64>) -> PriceData {
        let n = closes.len();
        PriceData {
            timestamps: (0..n).map(|i| i as i64).collect(),
            closes,
            highs,
            lows,
            volume: vec![1000; n],
        }
    }

    fn dummy_forecast(direction: Direction, horizon: ForecastHorizon) -> Forecast {
        Forecast {
            direction,
            strength: String::new(),
            signals: IndicatorSignals {
                macd_bullish: false,
                macd_line: 0.0,
                signal_line: 0.0,
                rsi: 50.0,
                rsi_bullish: false,
                adx: 0.0,
                trending: false,
                bollinger_b: 0.5,
                bollinger_bandwidth: 0.0,
                stochastic_k: 50.0,
                nearest_support: None,
                nearest_resistance: None,
            },
            latest_price: 100.0,
            ml_used: false,
            horizon,
            projected_price: None,
        }
    }

    #[test]
    fn test_build_buy_signal_entry_below_price() {
        let data = make_data(vec![100.0; 40], vec![101.0; 40], vec![99.0; 40]);
        let forecast = dummy_forecast(Direction::Bullish, ForecastHorizon::Short);
        let signal = build("TEST", "1d", &data, &forecast, 1).unwrap();
        assert_eq!(signal.side, SignalSide::Buy);
        assert_eq!(signal.entry_high, 100.0);
        assert!(signal.entry_low < 100.0);
        assert_eq!(signal.reference_ts, 39);
    }

    #[test]
    fn test_build_sell_signal_entry_above_price() {
        let data = make_data(vec![100.0; 40], vec![101.0; 40], vec![99.0; 40]);
        let forecast = dummy_forecast(Direction::Bearish, ForecastHorizon::Long);
        let signal = build("TEST", "1d", &data, &forecast, 21).unwrap();
        assert_eq!(signal.side, SignalSide::Sell);
        assert_eq!(signal.entry_low, 100.0);
        assert!(signal.entry_high > 100.0);
        assert_eq!(signal.horizon_bars, 21);
    }

    #[test]
    fn test_build_returns_none_for_neutral_forecast() {
        let data = make_data(vec![100.0; 40], vec![101.0; 40], vec![99.0; 40]);
        let forecast = dummy_forecast(Direction::Neutral, ForecastHorizon::Short);
        assert!(build("TEST", "1d", &data, &forecast, 1).is_none());
    }

    fn stored_signal(side: SignalSide, horizon_bars: usize) -> StoredSignal {
        StoredSignal {
            id: 1,
            symbol: "TEST".into(),
            interval: "1d".into(),
            horizon: ForecastHorizon::Short,
            side,
            entry_low: 100.0,
            entry_high: 102.0,
            reference_price: 101.0,
            reference_ts: 0,
            horizon_bars,
        }
    }

    #[test]
    fn test_resolve_buy_win() {
        let data = make_data(
            vec![100.0, 101.0, 104.0, 105.0],
            vec![101.0, 102.0, 106.0, 107.0],
            vec![99.0, 99.5, 103.0, 104.0],
        );
        let signal = stored_signal(SignalSide::Buy, 2);
        let (outcome, price) = resolve(&signal, &data).unwrap();
        assert_eq!(outcome, "win");
        assert_eq!(price, 104.0);
    }

    #[test]
    fn test_resolve_buy_loss() {
        let data = make_data(
            vec![100.0, 101.0, 98.0, 97.0],
            vec![101.0, 102.0, 99.0, 98.0],
            vec![99.0, 99.5, 97.0, 96.0],
        );
        let signal = stored_signal(SignalSide::Buy, 2);
        let (outcome, price) = resolve(&signal, &data).unwrap();
        assert_eq!(outcome, "loss");
        assert_eq!(price, 98.0);
    }

    #[test]
    fn test_resolve_sell_win() {
        let data = make_data(
            vec![100.0, 101.0, 97.0, 96.0],
            vec![101.0, 103.0, 100.0, 98.0],
            vec![99.0, 99.5, 96.0, 95.0],
        );
        let mut signal = stored_signal(SignalSide::Sell, 2);
        signal.entry_low = 101.0;
        signal.entry_high = 103.0;
        let (outcome, price) = resolve(&signal, &data).unwrap();
        assert_eq!(outcome, "win");
        assert_eq!(price, 97.0);
    }

    #[test]
    fn test_resolve_missed_when_zone_never_touched() {
        let data = make_data(
            vec![100.0, 103.0, 104.0, 105.0],
            vec![101.0, 104.0, 105.0, 106.0],
            vec![99.0, 102.5, 103.5, 104.5],
        );
        let signal = stored_signal(SignalSide::Buy, 2);
        let (outcome, _) = resolve(&signal, &data).unwrap();
        assert_eq!(outcome, "missed");
    }

    #[test]
    fn test_resolve_pending_before_horizon_elapses() {
        let data = make_data(
            vec![100.0, 101.0, 102.0],
            vec![101.0, 102.0, 103.0],
            vec![99.0, 100.0, 101.0],
        );
        let signal = stored_signal(SignalSide::Buy, 5);
        assert!(resolve(&signal, &data).is_none());
    }
}
