use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[&str] = &[include_str!("schema.sql"), include_str!("userdata.sql"), include_str!("skipped.sql"), include_str!("identify.sql"), include_str!("catalog.sql"), include_str!("catalog2.sql"), include_str!("sync.sql"), include_str!("sync2.sql"), include_str!("shelf.sql")];

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        Self::init(conn)
    }
    pub fn open_in_memory() -> anyhow::Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }
    fn init(conn: Connection) -> anyhow::Result<Db> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, m) in MIGRATIONS.iter().enumerate().skip(v as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(m)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)?;
            tx.commit()?;
        }
        Ok(Db { conn })
    }
    pub fn conn(&self) -> &Connection { &self.conn }
}

pub fn now() -> i64 { chrono::Utc::now().timestamp() }

pub fn format_day(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrate_fresh() {
        let db = Db::open_in_memory().unwrap();
        let v: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
        for t in ["dirs","files","games","status","ops","archive","journal_cursor","dats","dat_roms","meta_cache","jobs","devices","play","collections","collection_items"] {
            let n: i64 = db.conn().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1", [t], |r| r.get::<_, i64>(0)).unwrap();
            assert_eq!(n, 1, "missing table {t}");
        }
    }
}
