fn wilder_smooth(values: &[f64], period: usize) -> Option<Vec<f64>> {
    if values.len() < period {
        return None;
    }
    let mut smoothed = Vec::with_capacity(values.len());
    smoothed.push(values[..period].iter().sum::<f64>() / period as f64);
    let k = 1.0 / period as f64;
    for &v in values[period..].iter() {
        let prev = *smoothed.last().unwrap();
        smoothed.push((v - prev) * k + prev);
    }
    Some(smoothed)
}

fn ema(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period {
        return None;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut result = values[..period].iter().sum::<f64>() / period as f64;
    for &v in values[period..].iter() {
        result = v * k + result * (1.0 - k);
    }
    Some(result)
}

fn true_range(high: &[f64], low: &[f64], close: &[f64]) -> Vec<f64> {
    let mut tr = Vec::with_capacity(high.len());
    tr.push(high[0] - low[0]);
    for i in 1..high.len() {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr.push(hl.max(hc).max(lc));
    }
    tr
}

pub fn compute_macd(closes: &[f64]) -> (Option<f64>, Option<f64>) {
    if closes.len() < 35 {
        return (None, None);
    }
    let mut macd_vals = Vec::with_capacity(closes.len());
    let k12 = 2.0 / 13.0;
    let k26 = 2.0 / 27.0;
    let mut ema12 = closes[..12].iter().sum::<f64>() / 12.0;
    let mut ema26 = closes[..26].iter().sum::<f64>() / 26.0;
    for &c in closes[26..].iter() {
        ema12 = c * k12 + ema12 * (1.0 - k12);
        ema26 = c * k26 + ema26 * (1.0 - k26);
        macd_vals.push(ema12 - ema26);
    }
    let signal = ema(&macd_vals, 9);
    (macd_vals.last().copied(), signal)
}

pub fn compute_rsi(closes: &[f64], period: usize) -> Option<f64> {
    if closes.len() < period + 1 {
        return None;
    }
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for i in 1..=period {
        let diff = closes[i] - closes[i - 1];
        if diff > 0.0 {
            avg_gain += diff;
        } else {
            avg_loss -= diff;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;
    let k = 1.0 / period as f64;
    for i in period + 1..closes.len() {
        let diff = closes[i] - closes[i - 1];
        if diff > 0.0 {
            avg_gain = (diff - avg_gain) * k + avg_gain;
            avg_loss = (0.0 - avg_loss) * k + avg_loss;
        } else {
            avg_gain = (0.0 - avg_gain) * k + avg_gain;
            avg_loss = (-diff - avg_loss) * k + avg_loss;
        }
    }
    if avg_loss == 0.0 {
        return if avg_gain == 0.0 { Some(50.0) } else { Some(100.0) };
    }
    Some(100.0 - 100.0 / (1.0 + avg_gain / avg_loss))
}

pub fn compute_adx(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Option<f64> {
    if high.len() < period + 1 {
        return None;
    }
    let n = high.len();
    let mut plus_dm = Vec::with_capacity(n);
    let mut minus_dm = Vec::with_capacity(n);
    for i in 1..n {
        let up = high[i] - high[i - 1];
        let down = low[i - 1] - low[i];
        if up > down && up > 0.0 {
            plus_dm.push(up);
            minus_dm.push(0.0);
        } else if down > up && down > 0.0 {
            plus_dm.push(0.0);
            minus_dm.push(down);
        } else {
            plus_dm.push(0.0);
            minus_dm.push(0.0);
        }
    }
    let tr = &true_range(high, low, close)[1..];
    let tr_smooth = wilder_smooth(tr, period)?;
    let pdm_smooth = wilder_smooth(&plus_dm, period)?;
    let mdm_smooth = wilder_smooth(&minus_dm, period)?;
    let mut dx_values = Vec::new();
    for i in 0..tr_smooth.len() {
        let pdi = 100.0 * pdm_smooth[i] / tr_smooth[i];
        let mdi = 100.0 * mdm_smooth[i] / tr_smooth[i];
        let sum = pdi + mdi;
        if sum == 0.0 {
            dx_values.push(0.0);
        } else {
            dx_values.push(100.0 * (pdi - mdi).abs() / sum);
        }
    }
    wilder_smooth(&dx_values, period).and_then(|v| v.last().copied())
}

pub fn compute_macd_series(closes: &[f64]) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let n = closes.len();
    let mut macd_line = vec![None; n];
    let mut signal_line = vec![None; n];
    if n < 26 {
        return (macd_line, signal_line);
    }
    let k12 = 2.0 / 13.0;
    let k26 = 2.0 / 27.0;
    let mut ema12 = closes[..12].iter().sum::<f64>() / 12.0;
    let mut ema26 = closes[..26].iter().sum::<f64>() / 26.0;
    for i in 26..n {
        ema12 = closes[i] * k12 + ema12 * (1.0 - k12);
        ema26 = closes[i] * k26 + ema26 * (1.0 - k26);
        macd_line[i] = Some(ema12 - ema26);
    }
    if n >= 35 {
        let k_signal = 2.0 / 10.0;
        let mut ema_signal: f64 = (26..35).filter_map(|i| macd_line[i]).sum::<f64>() / 9.0;
        signal_line[34] = Some(ema_signal);
        for i in 35..n {
            if let Some(m) = macd_line[i] {
                ema_signal = m * k_signal + ema_signal * (1.0 - k_signal);
                signal_line[i] = Some(ema_signal);
            }
        }
    }
    (macd_line, signal_line)
}

pub fn compute_rsi_series(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = closes.len();
    let mut result = vec![None; n];
    if n < period + 1 {
        return result;
    }
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for i in 1..=period {
        let diff = closes[i] - closes[i - 1];
        if diff > 0.0 { avg_gain += diff; } else { avg_loss -= diff; }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    let rsi_at = |gain: f64, loss: f64| -> f64 {
        if loss == 0.0 {
            if gain == 0.0 { 50.0 } else { 100.0 }
        } else {
            100.0 - 100.0 / (1.0 + gain / loss)
        }
    };
    result[period] = Some(rsi_at(avg_gain, avg_loss));

    let k = 1.0 / period as f64;
    for i in period + 1..n {
        let diff = closes[i] - closes[i - 1];
        if diff > 0.0 {
            avg_gain = (diff - avg_gain) * k + avg_gain;
            avg_loss = (0.0 - avg_loss) * k + avg_loss;
        } else {
            avg_gain = (0.0 - avg_gain) * k + avg_gain;
            avg_loss = (-diff - avg_loss) * k + avg_loss;
        }
        result[i] = Some(rsi_at(avg_gain, avg_loss));
    }
    result
}

pub fn compute_adx_series(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = high.len();
    let mut result = vec![None; n];
    if n < period + 1 {
        return result;
    }
    let tr = true_range(high, low, close);
    let mut plus_dm = vec![0.0; n];
    let mut minus_dm = vec![0.0; n];
    for i in 1..n {
        let up = high[i] - high[i - 1];
        let down = low[i - 1] - low[i];
        if up > down && up > 0.0 {
            plus_dm[i] = up;
        } else if down > up && down > 0.0 {
            minus_dm[i] = down;
        }
    }
    let tr_slice: Vec<f64> = tr[1..].to_vec();
    let pdm_slice: Vec<f64> = plus_dm[1..].to_vec();
    let mdm_slice: Vec<f64> = minus_dm[1..].to_vec();
    let m = tr_slice.len();

    if m < period {
        return result;
    }
    let tr_smooth = wilder_smooth(&tr_slice, period).unwrap();
    let pdm_smooth = wilder_smooth(&pdm_slice, period).unwrap();
    let mdm_smooth = wilder_smooth(&mdm_slice, period).unwrap();

    let mut dx_vals = Vec::with_capacity(tr_smooth.len());
    for i in 0..tr_smooth.len() {
        let pdi = 100.0 * pdm_smooth[i] / tr_smooth[i];
        let mdi = 100.0 * mdm_smooth[i] / tr_smooth[i];
        let sum = pdi + mdi;
        dx_vals.push(if sum == 0.0 { 0.0 } else { 100.0 * (pdi - mdi).abs() / sum });
    }
    if let Some(adx_vals) = wilder_smooth(&dx_vals, period) {
        let start_idx = 2 * period - 1;
        for (j, &v) in adx_vals.iter().enumerate() {
            let idx = start_idx + j;
            if idx < n {
                result[idx] = Some(v);
            }
        }
    }
    result
}

pub fn compute_atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Option<f64> {
    compute_atr_series(high, low, close, period)
        .into_iter()
        .flatten()
        .last()
}

pub fn compute_atr_series(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = high.len();
    let mut result = vec![None; n];
    if n < period + 1 {
        return result;
    }
    let tr = true_range(high, low, close);
    if let Some(smoothed) = wilder_smooth(&tr, period) {
        for (k, &v) in smoothed.iter().enumerate() {
            result[period - 1 + k] = Some(v);
        }
    }
    result
}

pub fn compute_log_returns(close: &[f64]) -> Vec<Option<f64>> {
    let n = close.len();
    let mut result = vec![None; n];
    for i in 1..n {
        if close[i - 1] > 0.0 && close[i] > 0.0 {
            result[i] = Some((close[i] / close[i - 1]).ln());
        }
    }
    result
}

pub fn compute_rolling_volatility(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let returns = compute_log_returns(close);
    let n = returns.len();
    let mut result = vec![None; n];
    for i in period..n {
        let mut vals = Vec::with_capacity(period);
        for j in (i - period + 1)..=i {
            if let Some(r) = returns[j] {
                vals.push(r);
            }
        }
        if vals.len() == period {
            let mean = vals.iter().sum::<f64>() / period as f64;
            let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
            result[i] = Some(variance.sqrt());
        }
    }
    result
}

pub fn compute_return_series(close: &[f64], period: usize) -> Vec<Option<f64>> {
    let n = close.len();
    let mut result = vec![None; n];
    for i in period..n {
        if close[i] > 0.0 && close[i - period] > 0.0 {
            result[i] = Some((close[i] / close[i - period]).ln());
        }
    }
    result
}

pub fn compute_obv(close: &[f64], volume: &[i64]) -> Vec<Option<f64>> {
    let n = close.len().min(volume.len());
    let mut result = vec![None; n];
    if n == 0 {
        return result;
    }
    let mut obv = 0.0;
    result[0] = Some(0.0);
    for i in 1..n {
        if close[i] > close[i - 1] {
            obv += volume[i] as f64;
        } else if close[i] < close[i - 1] {
            obv -= volume[i] as f64;
        }
        result[i] = Some(obv);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_macd_short_data() {
        let closes = vec![1.0; 20];
        let (macd, signal) = compute_macd(&closes);
        assert!(macd.is_none());
        assert!(signal.is_none());
    }

    #[test]
    fn test_macd_constant_prices() {
        let closes = vec![100.0; 50];
        let (macd, signal) = compute_macd(&closes);
        assert!(macd.is_some());
        assert!(signal.is_some());
        assert!((macd.unwrap() - 0.0).abs() < 1e-10);
    }

    #[test]
    fn test_rsi_constant_prices() {
        let closes = vec![50.0; 20];
        let rsi = compute_rsi(&closes, 14);
        assert!(rsi.is_some());
        assert!((rsi.unwrap() - 50.0).abs() < 1e-10);
    }

    #[test]
    fn test_rsi_all_up() {
        let mut closes = vec![100.0];
        for i in 1..=20 {
            closes.push(100.0 + i as f64);
        }
        let rsi = compute_rsi(&closes, 14);
        assert!(rsi.unwrap() > 50.0);
    }

    #[test]
    fn test_rsi_all_down() {
        let mut closes = vec![100.0];
        for i in 1..=20 {
            closes.push(100.0 - i as f64);
        }
        let rsi = compute_rsi(&closes, 14);
        assert!(rsi.unwrap() < 50.0);
    }

    #[test]
    fn test_adx_insufficient_data() {
        let high = vec![1.0, 2.0];
        let low = vec![0.5, 1.0];
        let close = vec![1.0, 1.5];
        assert!(compute_adx(&high, &low, &close, 14).is_none());
    }

    #[test]
    fn test_atr_series_matches_single() {
        let closes: Vec<f64> = (0..100).map(|i| 100.0 + (i as f64).sin()).collect();
        let highs: Vec<f64> = closes.iter().map(|&c| c + 3.0).collect();
        let lows: Vec<f64> = closes.iter().map(|&c| c - 3.0).collect();
        let atr_single = compute_atr(&highs, &lows, &closes, 14);
        let atr_series = compute_atr_series(&highs, &lows, &closes, 14);
        let last = closes.len() - 1;
        assert_eq!(atr_series[last], atr_single);
    }

    #[test]
    fn test_log_return_series() {
        let closes = vec![100.0, 110.0, 121.0];
        let ret = compute_return_series(&closes, 1);
        assert!(ret[1].is_some());
        assert!((ret[1].unwrap() - (110.0_f64 / 100.0_f64).ln()).abs() < 1e-10);
    }

    #[test]
    fn test_log_return_period_5() {
        let closes: Vec<f64> = (0..20).map(|i| 100.0 * (1.01f64).powi(i)).collect();
        let ret5 = compute_return_series(&closes, 5);
        for i in 5..10 {
            assert!(ret5[i].is_some());
            let expected = (closes[i] / closes[i - 5]).ln();
            assert!((ret5[i].unwrap() - expected).abs() < 1e-10);
        }
        assert!(ret5[0].is_none());
        assert!(ret5[4].is_none());
    }

    #[test]
    fn test_rolling_volatility() {
        let closes = vec![100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0,
                          110.0, 111.0, 112.0, 113.0, 114.0, 115.0, 116.0, 117.0, 118.0, 119.0,
                          120.0, 121.0, 122.0, 123.0, 124.0, 125.0];
        let vol = compute_rolling_volatility(&closes, 21);
        assert!(vol[21].is_some());
        assert!(vol[21].unwrap() > 0.0);
        assert!(vol[20].is_none());
    }

    #[test]
    fn test_obv_up_trend() {
        let closes = vec![100.0, 101.0, 102.0];
        let volume = vec![1000, 1000, 1000];
        let obv = compute_obv(&closes, &volume);
        assert_eq!(obv[0], Some(0.0));
        assert_eq!(obv[1], Some(1000.0));
        assert_eq!(obv[2], Some(2000.0));
    }

    #[test]
    fn test_obv_down_trend() {
        let closes = vec![100.0, 99.0, 98.0];
        let volume = vec![1000, 1000, 1000];
        let obv = compute_obv(&closes, &volume);
        assert_eq!(obv[0], Some(0.0));
        assert_eq!(obv[1], Some(-1000.0));
        assert_eq!(obv[2], Some(-2000.0));
    }

    #[test]
    fn test_series_match_single() {
        let closes: Vec<f64> = (0..100).map(|i| 100.0 + (i as f64).sin()).collect();
        let (macd_single, sig_single) = compute_macd(&closes);
        let (macd_series, sig_series) = compute_macd_series(&closes);

        let last = closes.len() - 1;
        assert_eq!(macd_series[last], macd_single);
        assert_eq!(sig_series[last], sig_single);

        let highs: Vec<f64> = closes.iter().map(|&c| c + 2.0).collect();
        let lows: Vec<f64> = closes.iter().map(|&c| c - 2.0).collect();
        let adx_single = compute_adx(&highs, &lows, &closes, 14);
        let adx_series = compute_adx_series(&highs, &lows, &closes, 14);
        assert_eq!(adx_series[last], adx_single);
    }
}
