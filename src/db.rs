use anyhow::Result;
use rusqlite::{params, Connection};

use crate::models::PriceData;
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
}
