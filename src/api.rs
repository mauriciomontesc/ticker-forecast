use anyhow::{Context, Result};
use serde::Deserialize;

use crate::models::PriceData;

#[derive(Deserialize, Debug)]
pub struct ChartResponse {
    pub chart: ChartData,
}

#[derive(Deserialize, Debug)]
pub struct ChartData {
    pub result: Vec<ChartResult>,
}

#[derive(Deserialize, Debug)]
pub struct ChartResult {
    pub timestamp: Vec<i64>,
    pub indicators: Indicators,
}

#[derive(Deserialize, Debug)]
pub struct Indicators {
    pub quote: Vec<QuoteData>,
}

#[allow(dead_code)]
#[derive(Deserialize, Debug)]
pub struct QuoteData {
    pub open: Vec<Option<f64>>,
    pub high: Vec<Option<f64>>,
    pub low: Vec<Option<f64>>,
    pub close: Vec<Option<f64>>,
    pub volume: Vec<Option<i64>>,
}

impl ChartResponse {
    pub fn extract(&self) -> Result<PriceData> {
        let result = self.chart.result.first().context("No chart data")?;
        let quote = result.indicators.quote.first().context("No quote data")?;

        let mut timestamps = Vec::new();
        let mut closes = Vec::new();
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut volume = Vec::new();

        for i in 0..result.timestamp.len() {
            if let (Some(c), Some(h), Some(l)) = (quote.close[i], quote.high[i], quote.low[i]) {
                timestamps.push(result.timestamp[i]);
                closes.push(c);
                highs.push(h);
                lows.push(l);
                volume.push(quote.volume[i].unwrap_or(0));
            }
        }

        Ok(PriceData { timestamps, closes, highs, lows, volume })
    }
}

pub async fn fetch(symbol: &str, range: &str, interval: &str) -> Result<ChartResponse> {
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?range={range}&interval={interval}");
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()?;
    let response = client.get(&url).send().await?;
    let status = response.status();
    let text = response.text().await?;

    if !status.is_success() || text.trim().is_empty() {
        anyhow::bail!("Yahoo API error {status}");
    }

    Ok(serde_json::from_str(&text)?)
}
