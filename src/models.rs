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

impl ForecastHorizon {
    pub fn as_str(&self) -> &'static str {
        match self {
            ForecastHorizon::Short => "short",
            ForecastHorizon::Long => "long",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "short" => Some(ForecastHorizon::Short),
            "long" => Some(ForecastHorizon::Long),
            _ => None,
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalSide {
    Buy,
    Sell,
}

impl SignalSide {
    pub fn as_str(&self) -> &'static str {
        match self {
            SignalSide::Buy => "buy",
            SignalSide::Sell => "sell",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "buy" => Some(SignalSide::Buy),
            "sell" => Some(SignalSide::Sell),
            _ => None,
        }
    }
}

/// Actionable trade signal: side plus the price range where the position
/// should be opened.
#[derive(Debug, Clone)]
pub struct TradeSignal {
    pub symbol: String,
    pub interval: String,
    pub horizon: ForecastHorizon,
    pub side: SignalSide,
    pub entry_low: f64,
    pub entry_high: f64,
    pub reference_price: f64,
    /// Timestamp of the last candle used to build the signal.
    pub reference_ts: i64,
    /// Candles that must pass before the signal can be evaluated.
    pub horizon_bars: usize,
}

/// A signal loaded from the database (adds the row id).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StoredSignal {
    pub id: i64,
    pub symbol: String,
    pub interval: String,
    pub horizon: ForecastHorizon,
    pub side: SignalSide,
    pub entry_low: f64,
    pub entry_high: f64,
    pub reference_price: f64,
    pub reference_ts: i64,
    pub horizon_bars: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SignalStats {
    pub wins: usize,
    pub losses: usize,
    pub missed: usize,
    pub pending: usize,
}

impl SignalStats {
    pub fn resolved(&self) -> usize {
        self.wins + self.losses
    }

    pub fn win_rate(&self) -> Option<f64> {
        let resolved = self.resolved();
        if resolved == 0 {
            None
        } else {
            Some(self.wins as f64 / resolved as f64)
        }
    }
}
