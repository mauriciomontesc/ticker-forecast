#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PriceData {
    pub timestamps: Vec<i64>,
    pub closes: Vec<f64>,
    pub highs: Vec<f64>,
    pub lows: Vec<f64>,
    pub volume: Vec<i64>,
}

impl PriceData {
    pub fn is_empty(&self) -> bool {
        self.closes.is_empty()
    }

    pub fn latest_close(&self) -> Option<f64> {
        self.closes.last().copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MlContribution {
    High,
    Low,
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ForecastHorizon {
    /// Next candle (1 period ahead).
    Short,
    /// ~1 month ahead.
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Bullish,
    Bearish,
    Neutral,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IndicatorSignals {
    pub macd_bullish: bool,
    pub macd_line: f64,
    pub signal_line: f64,
    pub rsi: f64,
    pub rsi_bullish: bool,
    pub adx: f64,
    pub trending: bool,
    pub bollinger_b: f64,
    pub bollinger_bandwidth: f64,
    pub stochastic_k: f64,
    pub nearest_support: Option<f64>,
    pub nearest_resistance: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Forecast {
    pub direction: Direction,
    pub strength: String,
    pub signals: IndicatorSignals,
    pub latest_price: f64,
    pub ml_used: bool,
    /// Short (next candle) or long (~1 month) horizon.
    pub horizon: ForecastHorizon,
    /// Projected price at the end of the horizon (long-term forecasts only).
    pub projected_price: Option<f64>,
}
