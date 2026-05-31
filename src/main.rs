mod api;
mod cli;
mod db;
mod display;
mod forecast;
mod indicators;
mod ml;
mod models;
mod traits;

use anyhow::Result;
use tracing_subscriber;

use crate::db::Database;
use crate::traits::PriceRepository;
use models::PriceData;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .without_time()
        .init();

    let config = cli::Config::parse();
    let mut db = Database::open("trade_cache.db")?;

    let data = if config.intraday || config.refresh {
        fetch_and_store(&config, &mut db).await?
    } else {
        match db.load_cached(&config.symbol)? {
            Some(d) => {
                display::print_source("datos desde cache");
                d
            }
            None => fetch_and_store(&config, &mut db).await?,
        }
    };

    if data.is_empty() {
        tracing::error!("No se recibieron datos de precio");
        return Ok(());
    }

    if !config.intraday && !config.refresh {
        db.log_query(&config.symbol, &config.range, &config.interval)?;
    }

    let ml_result = ml::predict_next(&data, &config.symbol, Some(&db as &dyn PriceRepository));
    let (ml_bullish, ml_accuracy) = ml_result.map(|(d, _c, a)| (d, a)).unzip();
    match forecast::analyze_with_ml(&data, ml_bullish, ml_accuracy) {
        Some(result) => display::print_forecast(&result, &config.symbol),
        None => tracing::error!("No se pudo generar pronóstico: datos insuficientes"),
    }

    Ok(())
}

async fn fetch_and_store(config: &cli::Config, db: &mut impl PriceRepository) -> Result<PriceData> {
    let raw = api::fetch(&config.symbol, &config.range, &config.interval).await?;

    if let Some(result) = raw.chart.result.first() {
        if let Some(quote) = result.indicators.quote.first() {
            db.store_ohlcv(
                &config.symbol,
                &result.timestamp,
                &quote.open,
                &quote.high,
                &quote.low,
                &quote.close,
                &quote.volume,
            )?;
        }
    }

    let data = raw.extract()?;
    display::print_source("datos desde yahoo finance");
    Ok(data)
}
