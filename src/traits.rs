use anyhow::Result;

use crate::models::{
    ForecastHorizon, PriceData, SignalStats, StoredSignal, TradeSignal,
};

pub trait PriceRepository {
    fn load_cached(&self, symbol: &str, range: &str, interval: &str) -> Result<Option<PriceData>>;
    fn store_ohlcv(
        &mut self,
        symbol: &str,
        range: &str,
        interval: &str,
        timestamps: &[i64],
        opens: &[Option<f64>],
        highs: &[Option<f64>],
        lows: &[Option<f64>],
        closes: &[Option<f64>],
        volumes: &[Option<i64>],
    ) -> Result<()>;
    fn log_query(&self, symbol: &str, range: &str, interval: &str) -> Result<()>;
    fn load_model_json(&self, symbol: &str, data_count: usize) -> Result<Option<String>>;
    fn save_model_json(&self, symbol: &str, model_json: &str, data_count: usize) -> Result<()>;
    fn save_signal(&self, signal: &TradeSignal) -> Result<()>;
    fn pending_signals(&self, symbol: &str, interval: &str) -> Result<Vec<StoredSignal>>;
    fn resolve_signal(&self, id: i64, outcome: &str, resolved_price: f64) -> Result<()>;
    fn signal_stats(
        &self,
        symbol: &str,
        interval: &str,
        horizon: ForecastHorizon,
    ) -> Result<SignalStats>;
}
