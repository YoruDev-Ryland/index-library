//! Talking to an Index sync server.
use super::record::Record;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

pub struct Client { base: String, token: String, http: reqwest::blocking::Client }

#[derive(Debug, Deserialize)]
pub struct Health { pub version: String, pub api: u32, pub time_ms: i64 }

#[derive(Debug, Deserialize)]
pub struct PushResult { pub accepted: usize, pub duplicates: usize, pub last_seq: i64 }

#[derive(Debug, Deserialize)]
pub struct Seq { pub seq: i64, pub record: Record }

#[derive(Debug, Deserialize)]
pub struct Page { pub records: Vec<Seq>, pub next: i64 }

#[derive(Debug, Deserialize)]
pub struct StatePage { pub records: Vec<Seq>, pub next_key: Option<String> }

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder().timeout(Duration::from_secs(120)).connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("Index/", env!("CARGO_PKG_VERSION"))).build().expect("http client")
}

/// The server's address as typed, without a trailing slash; http:// is assumed when no scheme is given.
pub fn normalize_url(u: &str) -> String {
    let u = u.trim().trim_end_matches('/');
    if u.contains("://") { u.to_string() } else { format!("http://{u}") }
}

fn check(r: reqwest::blocking::Response) -> anyhow::Result<reqwest::blocking::Response> {
    let s = r.status();
    if s.is_success() { return Ok(r); }
    let msg = r.json::<Value>().ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| s.to_string());
    if s == reqwest::StatusCode::UNAUTHORIZED { anyhow::bail!("the sync server refused this device: {msg}") }
    anyhow::bail!("sync server: {msg}")
}

impl Client {
    pub fn new(url: &str, token: &str) -> Client { Client { base: normalize_url(url), token: token.to_string(), http: http() } }

    /// Sign this device in; returns (token, device id).
    pub fn login(url: &str, password: &str, device: &str) -> anyhow::Result<(String, String)> {
        let r = check(http().post(format!("{}/v1/login", normalize_url(url))).json(&json!({"password": password, "device_name": device})).send()?)?;
        let v: Value = r.json()?;
        Ok((v["token"].as_str().unwrap_or_default().to_string(), v["device_id"].as_str().unwrap_or_default().to_string()))
    }
    fn auth(&self, b: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder { b.bearer_auth(&self.token) }

    pub fn health(&self) -> anyhow::Result<Health> { Ok(check(self.http.get(format!("{}/v1/health", self.base)).send()?)?.json()?) }
    pub fn push(&self, recs: &[Record]) -> anyhow::Result<PushResult> {
        Ok(check(self.auth(self.http.post(format!("{}/v1/records", self.base))).json(recs).send()?)?.json()?)
    }
    pub fn pull(&self, after: i64, limit: i64) -> anyhow::Result<Page> {
        Ok(check(self.auth(self.http.get(format!("{}/v1/records", self.base))).query(&[("after", after), ("limit", limit)]).send()?)?.json()?)
    }
    pub fn state(&self, kinds: &[&str], after_key: &str, limit: i64) -> anyhow::Result<StatePage> {
        let limit = limit.to_string();
        Ok(check(self.auth(self.http.get(format!("{}/v1/state", self.base))).query(&[("kinds", kinds.join(",").as_str()), ("after_key", after_key), ("limit", limit.as_str())]).send()?)?.json()?)
    }
    pub fn has_blob(&self, sha: &str) -> anyhow::Result<bool> {
        let r = self.auth(self.http.head(format!("{}/v1/blobs/{sha}", self.base))).send()?;
        if r.status() == reqwest::StatusCode::NOT_FOUND { return Ok(false); }
        check(r)?;
        Ok(true)
    }
    pub fn put_blob(&self, sha: &str, data: Vec<u8>) -> anyhow::Result<()> {
        check(self.auth(self.http.put(format!("{}/v1/blobs/{sha}", self.base))).body(data).send()?)?;
        Ok(())
    }
    pub fn get_blob(&self, sha: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let r = self.auth(self.http.get(format!("{}/v1/blobs/{sha}", self.base))).send()?;
        if r.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
        Ok(Some(check(r)?.bytes()?.to_vec()))
    }
}
