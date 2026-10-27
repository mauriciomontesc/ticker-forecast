use tracing::info;

use crate::models::{
    Direction, Forecast, ForecastHorizon, SignalSide, SignalStats, TradeSignal,
};

pub fn print_source(source: &str) {
    info!("({source})");
}

/// Prints one report: indicators are listed once and both forecast horizons
/// (short and long) are shown below them.
pub fn print_forecasts(
    short: Option<&Forecast>,
    long: Option<&Forecast>,
    symbol: &str,
    interval: &str,
) {
    let Some(reference) = short.or(long) else {
        return;
    };
    let s = &reference.signals;

    info!("=== {symbol} ===");
    info!("Precio: ${:.5}", reference.latest_price);
    info!("MACD: {:.5}", s.macd_line);
    info!("Signal: {:.5}", s.signal_line);
    info!("Histograma: {:.5}", s.macd_line - s.signal_line);
    info!("RSI (14): {:.1}", s.rsi);
    info!("ADX (14): {:.1}", s.adx);
    info!("Bollinger %B: {:.3}", s.bollinger_b);
    info!("Bollinger BW: {:.4}", s.bollinger_bandwidth);
    info!("Stochastic %K (14): {:.1}", s.stochastic_k);
    if let Some(sup) = s.nearest_support {
        info!("Soporte cercano: ${:.5}", sup);
    }
    if let Some(res) = s.nearest_resistance {
        info!("Resistencia cercana: ${:.5}", res);
    }

    if let Some(forecast) = short {
        print_horizon(forecast, interval);
    }
    if let Some(forecast) = long {
        print_horizon(forecast, interval);
    }
}

fn print_horizon(forecast: &Forecast, interval: &str) {
    let label = match forecast.horizon {
        ForecastHorizon::Short => format!("corto plazo (próxima vela de {interval})"),
        ForecastHorizon::Long => "largo plazo (~1 mes)".to_string(),
    };
    let direction = match forecast.direction {
        Direction::Bullish => "Subida (Bullish)",
        Direction::Bearish => "Bajada (Bearish)",
        Direction::Neutral => "Neutral",
    };

    info!("Pronóstico {label}: {direction} — {}", forecast.strength);
    if let Some(price) = forecast.projected_price {
        info!("  Precio proyectado: ${price:.5}");
    }
    if forecast.ml_used {
        info!("  (incluye voto ponderado de ML)");
    }
}

/// Prints the actionable signal: side, entry range and historical hit rate.
pub fn print_signal(signal: &TradeSignal, stats: &SignalStats) {
    let horizon = match signal.horizon {
        ForecastHorizon::Short => "corto plazo",
        ForecastHorizon::Long => "largo plazo (~1 mes)",
    };
    let side = match signal.side {
        SignalSide::Buy => "Compra (BUY)",
        SignalSide::Sell => "Venta (SELL)",
    };

    info!("Señal {horizon}: {side}");
    info!(
        "  Entrada: ${:.5} – ${:.5}",
        signal.entry_low, signal.entry_high
    );
    info!("  Referencia: ${:.5}", signal.reference_price);

    match stats.win_rate() {
        Some(rate) => info!(
            "  Historial: {}/{} aciertos ({:.0}%)",
            stats.wins,
            stats.resolved(),
            rate * 100.0
        ),
        None => info!("  Historial: sin señales resueltas"),
    }
    if stats.pending > 0 {
        info!("  Señales abiertas: {}", stats.pending);
    }
    if stats.missed > 0 {
        info!("  Sin entrada (zona no alcanzada): {}", stats.missed);
    }
    if stats.resolved() >= 10 {
        if let Some(rate) = stats.win_rate() {
            if rate < 0.45 {
                info!("  Advertencia: baja fiabilidad histórica");
            }
        }
    }
}

pub fn print_no_signal(horizon: ForecastHorizon) {
    let label = match horizon {
        ForecastHorizon::Short => "corto plazo",
        ForecastHorizon::Long => "largo plazo (~1 mes)",
    };
    info!("Señal {label}: sin señal (pronóstico neutral)");
}
