use clap::Parser;

#[derive(Parser)]
#[command(name = "ticker-forecast", version)]
struct Cli {
    #[arg(default_value = "EURUSD=X")]
    symbol: String,

    #[arg(long)]
    refresh: bool,

    #[arg(long)]
    intraday: bool,

    #[arg(long)]
    range: Option<String>,

    #[arg(long)]
    interval: Option<String>,
}

pub struct Config {
    pub symbol: String,
    pub refresh: bool,
    pub intraday: bool,
    pub range: String,
    pub interval: String,
}

impl Config {
    pub fn parse() -> Self {
        let cli = Cli::parse();
        let (range, interval) = if cli.intraday {
            (
                cli.range.unwrap_or_else(|| "6mo".into()),
                cli.interval.unwrap_or_else(|| "1h".into()),
            )
        } else {
            (
                cli.range.unwrap_or_else(|| "3y".into()),
                cli.interval.unwrap_or_else(|| "1d".into()),
            )
        };
        Self {
            symbol: cli.symbol,
            refresh: cli.refresh,
            intraday: cli.intraday,
            range,
            interval,
        }
    }
}
