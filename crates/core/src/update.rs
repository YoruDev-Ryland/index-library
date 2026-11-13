//! Is there a newer Index? Asks GitHub's API for the latest release; when the API says no (60
//! requests an hour without signing in) or fails, reads the release's latest.json from GitHub's
//! download servers instead, which that limit doesn't count.
use serde::Serialize;

pub const REPO: &str = "YoruDev-Ryland/index-library";
const API: &str = "https://api.github.com/repos/YoruDev-Ryland/index-library/releases/latest";
pub const LATEST_JSON: &str = "https://github.com/YoruDev-Ryland/index-library/releases/latest/download/latest.json";

/// A newer version and where to read about it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
    /// the release's page
    pub url: String,
    /// how it was found: "api" or "latest.json"
    pub via: &'static str,
}

fn parts(v: &str) -> Option<(Vec<u64>, Option<String>)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once('-') { Some((c, p)) => (c, Some(p.to_string())), None => (v, None) };
    let nums: Option<Vec<u64>> = core.split('.').map(|n| n.parse().ok()).collect();
    nums.filter(|n| !n.is_empty()).map(|n| (n, pre))
}

/// Is version `a` newer than `b`? (Semantic versions, "v" prefix allowed; a prerelease is older
/// than its release.)
pub fn newer(a: &str, b: &str) -> bool {
    let (Some((an, ap)), Some((bn, bp))) = (parts(a), parts(b)) else { return false };
    let len = an.len().max(bn.len());
    let pad = |v: &Vec<u64>| { let mut v = v.clone(); v.resize(len, 0); v };
    match pad(&an).cmp(&pad(&bn)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => matches!((&ap, &bp), (None, Some(_))) || matches!((&ap, &bp), (Some(x), Some(y)) if x > y),
    }
}

fn release_page(version: &str) -> String { format!("https://github.com/{REPO}/releases/tag/v{}", version.trim_start_matches('v')) }

/// The check, with how to fetch a URL (status, body) given.
pub fn check_with(current: &str, fetch: &dyn Fn(&str) -> anyhow::Result<(u16, String)>) -> anyhow::Result<Option<Available>> {
    let from_api = (|| -> anyhow::Result<Available> {
        let (status, body) = fetch(API)?;
        if status != 200 { anyhow::bail!("GitHub API: HTTP {status}"); }
        let v: serde_json::Value = serde_json::from_str(&body)?;
        let tag = v["tag_name"].as_str().ok_or_else(|| anyhow::anyhow!("GitHub API: no tag"))?;
        Ok(Available {
            version: tag.trim_start_matches(['v', 'V']).to_string(),
            notes: v["body"].as_str().map(String::from).filter(|s| !s.trim().is_empty()),
            url: v["html_url"].as_str().map(String::from).unwrap_or_else(|| release_page(tag)),
            via: "api",
        })
    })();
    let found = match from_api {
        Ok(a) => a,
        Err(api_err) => {
            let (status, body) = fetch(LATEST_JSON).map_err(|e| anyhow::anyhow!("{api_err:#}; latest.json: {e:#}"))?;
            if status != 200 { anyhow::bail!("{api_err:#}; latest.json: HTTP {status}"); }
            let v: serde_json::Value = serde_json::from_str(&body)?;
            let version = v["version"].as_str().ok_or_else(|| anyhow::anyhow!("latest.json: no version"))?.trim_start_matches(['v', 'V']).to_string();
            Available { url: release_page(&version), notes: v["notes"].as_str().map(String::from).filter(|s| !s.trim().is_empty()), version, via: "latest.json" }
        }
    };
    Ok(newer(&found.version, current).then_some(found))
}

/// Check GitHub for a version newer than `current`.
pub fn check(current: &str) -> anyhow::Result<Option<Available>> {
    let http = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("Index/", env!("CARGO_PKG_VERSION"))).build()?;
    check_with(current, &|url| {
        let r = http.get(url).header("accept", "application/vnd.github+json").send()?;
        let status = r.status().as_u16();
        Ok((status, r.text()?))
    })
}

/// Links the app opens in your browser: only Index's own GitHub pages.
pub fn is_release_page(url: &str) -> bool { url.starts_with(&format!("https://github.com/{REPO}/")) }
