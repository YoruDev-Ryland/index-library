//! HTTP with per-host rate limits and an on-disk (SQLite) response cache.
use crate::db::Db;
use rusqlite::OptionalExtension;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter { interval: Duration, next: Mutex<Option<Instant>> }

impl RateLimiter {
    pub fn new(interval: Duration) -> RateLimiter { RateLimiter { interval, next: Mutex::new(None) } }
    /// Reserve a slot at `now`; returns how long the caller must wait before sending.
    pub fn reserve(&self, now: Instant) -> Duration {
        let mut n = self.next.lock().unwrap();
        let slot = match *n { Some(t) if t > now => t, _ => now };
        *n = Some(slot + self.interval);
        slot - now
    }
}

pub struct Http { client: reqwest::blocking::Client, limits: HashMap<&'static str, RateLimiter> }

const CACHE_SECS: i64 = 30 * 86_400;

impl Default for Http { fn default() -> Self { Self::new() } }

impl Http {
    pub fn new() -> Http {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("index-library/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30)).build().expect("http client");
        let mut limits = HashMap::new();
        limits.insert("www.steamgriddb.com", RateLimiter::new(Duration::from_millis(1000)));
        limits.insert("api.igdb.com", RateLimiter::new(Duration::from_millis(260)));
        limits.insert("api.screenscraper.fr", RateLimiter::new(Duration::from_millis(1200)));
        limits.insert("en.wikipedia.org", RateLimiter::new(Duration::from_millis(200)));
        limits.insert("upload.wikimedia.org", RateLimiter::new(Duration::from_millis(1100)));
        limits.insert("thumb.wikimedia.org", RateLimiter::new(Duration::from_millis(1100)));
        limits.insert("thumbnails.libretro.com", RateLimiter::new(Duration::from_millis(150)));
        Http { client, limits }
    }

    fn wait_for(&self, url: &str) {
        let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        if let Some(l) = self.limits.get(host.as_str()) {
            let d = l.reserve(Instant::now());
            if !d.is_zero() { std::thread::sleep(d); }
        }
    }

    fn cached(&self, db: &Db, source: &str, key: &str) -> anyhow::Result<Option<serde_json::Value>> {
        let r: Option<(String, i64)> = db.conn().query_row("SELECT json, fetched_at FROM meta_cache WHERE source=?1 AND key=?2", (source, key), |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        Ok(r.filter(|(_, at)| crate::db::now() - at < CACHE_SECS).and_then(|(j, _)| serde_json::from_str(&j).ok()))
    }
    fn store(&self, db: &Db, source: &str, key: &str, v: &serde_json::Value) -> anyhow::Result<()> {
        db.conn().execute("INSERT INTO meta_cache(source, key, json, fetched_at) VALUES(?1,?2,?3,?4) ON CONFLICT(source,key) DO UPDATE SET json=excluded.json, fetched_at=excluded.fetched_at",
            (source, key, v.to_string(), crate::db::now()))?;
        Ok(())
    }

    /// Send a request, waiting for the site's turn; when the site says to slow down (429), wait and
    /// try again a few times instead of losing the answer.
    fn send(&self, url: &str, build: impl Fn() -> reqwest::blocking::RequestBuilder) -> anyhow::Result<reqwest::blocking::Response> {
        for attempt in 1..=5u64 {
            self.wait_for(url);
            let resp = build().send()?;
            if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS || attempt == 5 { return Ok(resp); }
            let after = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()).unwrap_or(attempt);
            std::thread::sleep(Duration::from_secs(after.min(30)));
        }
        unreachable!()
    }

    /// GET JSON, cached by URL (secrets in `cache_key` should be stripped by the caller).
    pub fn get_json(&self, db: &Db, source: &str, url: &str, cache_key: &str, headers: &[(&str, String)]) -> anyhow::Result<serde_json::Value> {
        if let Some(v) = self.cached(db, source, cache_key)? { return Ok(v); }
        let resp = self.send(url, || { let mut rq = self.client.get(url); for (k, v) in headers { rq = rq.header(*k, v); } rq })?;
        let status = resp.status();
        if status.as_u16() == 404 { let v = serde_json::Value::Null; self.store(db, source, cache_key, &v)?; return Ok(v); }
        if !status.is_success() { anyhow::bail!("{source}: HTTP {status}"); }
        let v: serde_json::Value = resp.json()?;
        self.store(db, source, cache_key, &v)?;
        Ok(v)
    }

    pub fn post_json(&self, db: &Db, source: &str, url: &str, body: &str, headers: &[(&str, String)]) -> anyhow::Result<serde_json::Value> {
        let key = format!("{url}\n{body}");
        if let Some(v) = self.cached(db, source, &key)? { return Ok(v); }
        let resp = self.send(url, || { let mut rq = self.client.post(url).body(body.to_string()); for (k, v) in headers { rq = rq.header(*k, v); } rq })?;
        if !resp.status().is_success() { anyhow::bail!("{source}: HTTP {}", resp.status()); }
        let v: serde_json::Value = resp.json()?;
        self.store(db, source, &key, &v)?;
        Ok(v)
    }

    pub fn get_uncached_json(&self, url: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self.send(url, || self.client.get(url))?;
        if !resp.status().is_success() { anyhow::bail!("HTTP {}", resp.status()); }
        Ok(resp.json()?)
    }

    /// A POST answered fresh every time (large listings that aren't worth keeping as they come).
    pub fn post_json_uncached(&self, url: &str, body: &str, headers: &[(&str, String)]) -> anyhow::Result<serde_json::Value> {
        let resp = self.send(url, || { let mut rq = self.client.post(url).body(body.to_string()); for (k, v) in headers { rq = rq.header(*k, v); } rq })?;
        if !resp.status().is_success() { anyhow::bail!("HTTP {}", resp.status()); }
        Ok(resp.json()?)
    }

    pub fn post_form_uncached(&self, url: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self.client.post(url).send()?;
        if !resp.status().is_success() { anyhow::bail!("HTTP {}", resp.status()); }
        Ok(resp.json()?)
    }

    /// Download a large file to `dest` (written to a temp name, then renamed), reporting bytes.
    pub fn download(&self, url: &str, dest: &Path, progress: &dyn Fn(u64, u64), cancel: &std::sync::atomic::AtomicBool) -> anyhow::Result<()> {
        use std::io::{Read, Write};
        let client = reqwest::blocking::Client::builder().user_agent(concat!("index-library/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30)).timeout(Duration::from_secs(1800)).build()?;
        let mut r = client.get(url).send()?.error_for_status()?;
        let total = r.content_length().unwrap_or(0);
        if let Some(d) = dest.parent() { std::fs::create_dir_all(d)?; }
        let tmp = dest.with_extension("part");
        let mut f = std::fs::File::create(&tmp)?;
        let mut buf = vec![0u8; 1 << 20];
        let mut got = 0u64;
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) { drop(f); let _ = std::fs::remove_file(&tmp); anyhow::bail!("cancelled"); }
            let n = r.read(&mut buf)?;
            if n == 0 { break; }
            f.write_all(&buf[..n])?;
            got += n as u64;
            progress(got, total);
        }
        f.sync_all()?;
        std::fs::rename(&tmp, dest)?;
        Ok(())
    }

    /// A file's bytes; None when the server doesn't have it (404).
    pub fn get_bytes(&self, url: &str) -> anyhow::Result<Option<Vec<u8>>> {
        self.wait_for(url);
        let r = self.client.get(url).send()?;
        if r.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
        Ok(Some(r.error_for_status()?.bytes()?.to_vec()))
    }

    /// Download an image into `<cache>/art/<source>/<id>.<ext>` (skipped if present).
    pub fn fetch_art(&self, cache_dir: &Path, source: &str, id: &str, url: &str) -> anyhow::Result<PathBuf> {
        let path_part = url.split(['?', '#']).next().unwrap_or(url);
        let ext = path_part.rsplit('.').next().filter(|e| e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("jpg");
        let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        let p = cache_dir.join("art").join(source).join(format!("{safe}.{ext}"));
        if p.exists() { return Ok(p); }
        std::fs::create_dir_all(p.parent().unwrap())?;
        self.wait_for(url);
        let bytes = self.client.get(url).send()?.error_for_status()?.bytes()?;
        std::fs::write(&p, &bytes)?;
        Ok(p)
    }
}
