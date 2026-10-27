use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::{
    ForecastHorizon, PriceData, SignalSide, SignalStats, StoredSignal, TradeSignal,
};
use crate::traits::PriceRepository;

const CACHE_TTL_SECS: i64 = 259_200; // 3 days

pub struct Database {
    conn: Connection,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn migrate(conn: &Connection) -> Result<()> {
    if table_has_column(conn, "ohlcv", "interval_val")? {
        return Ok(());
    }

    conn.execute_batch(
        "CREATE TABLE ohlcv_migrated (
            symbol TEXT NOT NULL,
            range_val TEXT NOT NULL,
            interval_val TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            open REAL,
            high REAL,
            low REAL,
            close REAL,
            volume INTEGER,
            fetched_at INTEGER NOT NULL,
            UNIQUE(symbol, range_val, interval_val, timestamp)
        );",
    )?;

    conn.execute_batch(
        "INSERT INTO ohlcv_migrated (symbol, range_val, interval_val, timestamp, open, high, low, close, volume, fetched_at)
         SELECT symbol, '3y', '1d', timestamp, open, high, low, close, volume,
                COALESCE(CAST(strftime('%s', fetched_at) AS INTEGER), CAST(strftime('%s', 'now') AS INTEGER))
         FROM ohlcv;",
    )?;

    conn.execute_batch("DROP TABLE ohlcv; ALTER TABLE ohlcv_migrated RENAME TO ohlcv;")?;
    Ok(())
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ohlcv (
                symbol TEXT NOT NULL,
                range_val TEXT NOT NULL,
                interval_val TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                open REAL,
                high REAL,
                low REAL,
                close REAL,
                volume INTEGER,
                fetched_at INTEGER NOT NULL,
                UNIQUE(symbol, range_val, interval_val, timestamp)
            );
            CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                range_val TEXT NOT NULL,
                interval_val TEXT NOT NULL,
                queried_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ml_model (
                symbol TEXT NOT NULL PRIMARY KEY,
                model_json TEXT NOT NULL,
                data_count INTEGER NOT NULL,
                trained_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS trade_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                interval_val TEXT NOT NULL,
                horizon TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_low REAL NOT NULL,
                entry_high REAL NOT NULL,
                reference_price REAL NOT NULL,
                reference_ts INTEGER NOT NULL,
                horizon_bars INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                resolved_at TEXT,
                resolved_price REAL,
                outcome TEXT NOT NULL DEFAULT 'pending',
                UNIQUE(symbol, interval_val, horizon, reference_ts, side)
            );",
        )?;
        migrate(&conn)?;
        Ok(Self { conn })
    }
}

impl PriceRepository for Database {
    fn load_cached(&self, symbol: &str, range: &str, interval: &str) -> Result<Option<PriceData>> {
        let latest_fetch: Option<i64> = self.conn.query_row(
            "SELECT MAX(fetched_at) FROM ohlcv WHERE symbol = ?1 AND range_val = ?2 AND interval_val = ?3",
            params![symbol, range, interval],
            |row| row.get(0),
        )?;

        let now = now_unix();
        match latest_fetch {
            Some(fetched_at) if now - fetched_at < CACHE_TTL_SECS => {}
            _ => return Ok(None),
        }

        let mut stmt = self.conn.prepare(
            "SELECT timestamp, close, high, low, volume FROM ohlcv
             WHERE symbol = ?1 AND range_val = ?2 AND interval_val = ?3
             ORDER BY timestamp ASC",
        )?;

        let mut timestamps = Vec::new();
        let mut closes = Vec::new();
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut volume = Vec::new();

        for row in stmt.query_map(params![symbol, range, interval], |row| {
            let ts = row.get::<_, i64>(0)?;
            let c = row.get::<_, Option<f64>>(1)?;
            let h = row.get::<_, Option<f64>>(2)?;
            let l = row.get::<_, Option<f64>>(3)?;
            let v = row.get::<_, Option<i64>>(4)?;
            Ok((ts, c, h, l, v))
        })? {
            let (ts, c, h, l, v) = row?;
            if let (Some(c), Some(h), Some(l)) = (c, h, l) {
                timestamps.push(ts);
                closes.push(c);
                highs.push(h);
                lows.push(l);
                volume.push(v.unwrap_or(0));
            }
        }

        if closes.is_empty() {
            return Ok(None);
        }

        Ok(Some(PriceData {
            timestamps,
            closes,
            highs,
            lows,
            volume,
        }))
    }

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
    ) -> Result<()> {
        let fetched_at = now_unix();
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM ohlcv WHERE symbol = ?1 AND range_val = ?2 AND interval_val = ?3",
            params![symbol, range, interval],
        )?;
        for i in 0..timestamps.len() {
            tx.execute(
                "INSERT INTO ohlcv (symbol, range_val, interval_val, timestamp, open, high, low, close, volume, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    symbol,
                    range,
                    interval,
                    timestamps[i],
                    opens[i],
                    highs[i],
                    lows[i],
                    closes[i],
                    volumes[i],
                    fetched_at,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn log_query(&self, symbol: &str, range: &str, interval: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO query_history (symbol, range_val, interval_val) VALUES (?1, ?2, ?3)",
            params![symbol, range, interval],
        )?;
        Ok(())
    }

    fn load_model_json(&self, symbol: &str, data_count: usize) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT model_json FROM ml_model WHERE symbol = ?1 AND data_count = ?2",
        )?;
        let mut rows = stmt.query_map(params![symbol, data_count as i64], |row| {
            row.get::<_, String>(0)
        })?;
        match rows.next() {
            Some(Ok(json)) => Ok(Some(json)),
            _ => Ok(None),
        }
    }

    fn save_model_json(&self, symbol: &str, model_json: &str, data_count: usize) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ml_model (symbol, model_json, data_count) VALUES (?1, ?2, ?3)",
            params![symbol, model_json, data_count as i64],
        )?;
        Ok(())
    }

    fn save_signal(&self, signal: &TradeSignal) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO trade_signals
                (symbol, interval_val, horizon, side, entry_low, entry_high,
                 reference_price, reference_ts, horizon_bars)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                signal.symbol,
                signal.interval,
                signal.horizon.as_str(),
                signal.side.as_str(),
                signal.entry_low,
                signal.entry_high,
                signal.reference_price,
                signal.reference_ts,
                signal.horizon_bars as i64,
            ],
        )?;
        Ok(())
    }

    fn pending_signals(&self, symbol: &str, interval: &str) -> Result<Vec<StoredSignal>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol, interval_val, horizon, side, entry_low, entry_high,
                    reference_price, reference_ts, horizon_bars
             FROM trade_signals
             WHERE symbol = ?1 AND interval_val = ?2 AND outcome = 'pending'
             ORDER BY id ASC",
        )?;

        let rows = stmt.query_map(params![symbol, interval], |row| {
            let horizon: String = row.get(3)?;
            let side: String = row.get(4)?;
            Ok(StoredSignal {
                id: row.get(0)?,
                symbol: row.get(1)?,
                interval: row.get(2)?,
                horizon: ForecastHorizon::from_db(&horizon)
                    .unwrap_or(ForecastHorizon::Short),
                side: SignalSide::from_db(&side).unwrap_or(SignalSide::Buy),
                entry_low: row.get(5)?,
                entry_high: row.get(6)?,
                reference_price: row.get(7)?,
                reference_ts: row.get(8)?,
                horizon_bars: row.get::<_, i64>(9)? as usize,
            })
        })?;

        let mut signals = Vec::new();
        for row in rows {
            signals.push(row?);
        }
        Ok(signals)
    }

    fn resolve_signal(&self, id: i64, outcome: &str, resolved_price: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE trade_signals
             SET outcome = ?1, resolved_price = ?2, resolved_at = datetime('now')
             WHERE id = ?3 AND outcome = 'pending'",
            params![outcome, resolved_price, id],
        )?;
        Ok(())
    }

    fn signal_stats(
        &self,
        symbol: &str,
        interval: &str,
        horizon: ForecastHorizon,
    ) -> Result<SignalStats> {
        let mut stmt = self.conn.prepare(
            "SELECT outcome, COUNT(*) FROM trade_signals
             WHERE symbol = ?1 AND interval_val = ?2 AND horizon = ?3
             GROUP BY outcome",
        )?;

        let mut stats = SignalStats::default();
        for row in stmt.query_map(
            params![symbol, interval, horizon.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )? {
            let (outcome, count) = row?;
            match outcome.as_str() {
                "win" => stats.wins = count as usize,
                "loss" => stats.losses = count as usize,
                "missed" => stats.missed = count as usize,
                "pending" => stats.pending = count as usize,
                _ => {}
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_isolated_by_interval() {
        let path = std::env::temp_dir().join("ticker_forecast_cache_test.db");
        let _ = std::fs::remove_file(&path);
        let mut db = Database::open(path.to_str().unwrap()).unwrap();

        db.store_ohlcv(
            "TEST",
            "3y",
            "1d",
            &[1, 2],
            &[Some(1.0), Some(2.0)],
            &[Some(1.1), Some(2.1)],
            &[Some(0.9), Some(1.9)],
            &[Some(1.0), Some(2.0)],
            &[Some(100), Some(200)],
        )
        .unwrap();

        let daily = db.load_cached("TEST", "3y", "1d").unwrap();
        assert!(daily.is_some());
        assert_eq!(daily.unwrap().closes.len(), 2);

        let intraday = db.load_cached("TEST", "6mo", "1h").unwrap();
        assert!(intraday.is_none());
    }

    #[test]
    fn test_cache_ttl_uses_fetched_at() {
        let path = std::env::temp_dir().join("ticker_forecast_ttl_test.db");
        let _ = std::fs::remove_file(&path);
        let db = Database::open(path.to_str().unwrap()).unwrap();

        let stale = now_unix() - CACHE_TTL_SECS - 1;
        db.conn
            .execute(
                "INSERT INTO ohlcv (symbol, range_val, interval_val, timestamp, close, high, low, volume, fetched_at)
                 VALUES ('TEST', '3y', '1d', 1000, 1.0, 1.1, 0.9, 10, ?1)",
                params![stale],
            )
            .unwrap();

        assert!(db.load_cached("TEST", "3y", "1d").unwrap().is_none());
    }

    fn test_signal() -> TradeSignal {
        TradeSignal {
            symbol: "TEST".into(),
            interval: "1d".into(),
            horizon: ForecastHorizon::Short,
            side: SignalSide::Buy,
            entry_low: 99.0,
            entry_high: 100.0,
            reference_price: 100.0,
            reference_ts: 1_700_000_000,
            horizon_bars: 1,
        }
    }

    #[test]
    fn test_signal_persistence_and_dedup() {
        let path = std::env::temp_dir().join("ticker_forecast_signals_test.db");
        let _ = std::fs::remove_file(&path);
        let db = Database::open(path.to_str().unwrap()).unwrap();

        let signal = test_signal();
        db.save_signal(&signal).unwrap();
        db.save_signal(&signal).unwrap(); // duplicate (same key) is ignored

        let pending = db.pending_signals("TEST", "1d").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].side, SignalSide::Buy);
        assert_eq!(pending[0].horizon, ForecastHorizon::Short);
        assert_eq!(pending[0].horizon_bars, 1);
        assert_eq!(pending[0].entry_low, 99.0);

        let stats = db.signal_stats("TEST", "1d", ForecastHorizon::Short).unwrap();
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.win_rate(), None);
    }

    #[test]
    fn test_signal_resolution_updates_stats() {
        let path = std::env::temp_dir().join("ticker_forecast_signals_resolve_test.db");
        let _ = std::fs::remove_file(&path);
        let db = Database::open(path.to_str().unwrap()).unwrap();

        db.save_signal(&test_signal()).unwrap();
        let pending = db.pending_signals("TEST", "1d").unwrap();
        db.resolve_signal(pending[0].id, "win", 102.0).unwrap();

        assert!(db.pending_signals("TEST", "1d").unwrap().is_empty());
        let stats = db.signal_stats("TEST", "1d", ForecastHorizon::Short).unwrap();
        assert_eq!(stats.wins, 1);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.win_rate(), Some(1.0));
    }

    #[test]
    fn test_signal_stats_are_isolated_by_interval_and_horizon() {
        let path = std::env::temp_dir().join("ticker_forecast_signals_isolation_test.db");
        let _ = std::fs::remove_file(&path);
        let db = Database::open(path.to_str().unwrap()).unwrap();

        db.save_signal(&test_signal()).unwrap();
        let mut daily_long = test_signal();
        daily_long.horizon = ForecastHorizon::Long;
        daily_long.horizon_bars = 21;
        db.save_signal(&daily_long).unwrap();

        let mut hourly = test_signal();
        hourly.interval = "1h".into();
        db.save_signal(&hourly).unwrap();

        let short_daily = db.signal_stats("TEST", "1d", ForecastHorizon::Short).unwrap();
        let long_daily = db.signal_stats("TEST", "1d", ForecastHorizon::Long).unwrap();
        let short_hourly = db.signal_stats("TEST", "1h", ForecastHorizon::Short).unwrap();
        assert_eq!(short_daily.pending, 1);
        assert_eq!(long_daily.pending, 1);
        assert_eq!(short_hourly.pending, 1);
        assert_eq!(db.pending_signals("TEST", "1d").unwrap().len(), 2);
        assert_eq!(db.pending_signals("TEST", "1h").unwrap().len(), 1);
    }
}
