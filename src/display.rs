use tracing::info;

use crate::models::Forecast;

pub fn print_source(source: &str) {
    info!("({source})");
}

pub fn print_forecast(forecast: &Forecast, symbol: &str) {
    let s = &forecast.signals;
    let hist = s.macd_line - s.signal_line;

    info!("=== {symbol} ===");
    info!("Precio: ${:.5}", forecast.latest_price);
    info!("MACD: {:.5}", s.macd_line);
    info!("Signal: {:.5}", s.signal_line);
    info!("Histograma: {:.5}", hist);
    info!("RSI (14): {:.1}", s.rsi);
    info!("ADX (14): {:.1}", s.adx);

    let direction = match forecast.direction {
        crate::models::Direction::Bullish => "Subida (Bullish)",
        crate::models::Direction::Bearish => "Bajada (Bearish)",
        crate::models::Direction::Neutral => "Neutral",
    };

    info!("Pronóstico: {direction} — {}", forecast.strength);

    if forecast.ml_used {
        info!("(incluye voto ponderado de ML)");
    }
}
