use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::PriceData;
use crate::traits::PriceRepository;

const CACHE_TTL_SECS: i64 = 259_200; // 3 days

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ohlcv (
                symbol TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                open REAL,
                high REAL,
                low REAL,
                close REAL,
                volume INTEGER,
                fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(symbol, timestamp)
            );
            CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol TEXT NOT NULL,
                range_val TEXT NOT NULL,
                interval_val TEXT NOT NULL,
                queried_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ml_model (
                symbol TEXT NOT NULL PRIMARY KEY,
                model_json TEXT NOT NULL,
                data_count INTEGER NOT NULL,
                trained_at TEXT NOT NULL DEFAULT (datetime('now'))
            );"
        )?;
        Ok(Self { conn })
    }
}

impl PriceRepository for Database {
    fn load_cached(&self, symbol: &str) -> Result<Option<PriceData>> {
        let max_ts: Option<i64> = self.conn.query_row(
            "SELECT MAX(timestamp) FROM ohlcv WHERE symbol = ?1",
            params![symbol],
            |row| row.get(0),
        )?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        match max_ts {
            Some(ts) if now - ts < CACHE_TTL_SECS => {}
            _ => return Ok(None),
        }

        let mut stmt = self.conn.prepare(
            "SELECT timestamp, close, high, low, volume FROM ohlcv WHERE symbol = ?1 ORDER BY timestamp ASC",
        )?;

        let mut timestamps = Vec::new();
        let mut closes = Vec::new();
        let mut highs = Vec::new();
        let mut lows = Vec::new();
        let mut volume = Vec::new();

        for row in stmt.query_map(params![symbol], |row| {
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

        Ok(Some(PriceData { timestamps, closes, highs, lows, volume }))
    }

    fn store_ohlcv(
        &mut self,
        symbol: &str,
        timestamps: &[i64],
        opens: &[Option<f64>],
        highs: &[Option<f64>],
        lows: &[Option<f64>],
        closes: &[Option<f64>],
        volumes: &[Option<i64>],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        for i in 0..timestamps.len() {
            tx.execute(
                "INSERT OR REPLACE INTO ohlcv (symbol, timestamp, open, high, low, close, volume) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![symbol, timestamps[i], opens[i], highs[i], lows[i], closes[i], volumes[i]],
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
            "SELECT model_json FROM ml_model WHERE symbol = ?1 AND data_count = ?2"
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
}
