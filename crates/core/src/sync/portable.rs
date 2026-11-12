//! Records never hold an absolute path: a library folder's path becomes `{root:<id>}`, and each
//! device maps the ids to where it mounts those folders.
use crate::userdata::GameRef;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Roots { pub ids: Vec<(String, PathBuf)> }

fn norm(p: &Path) -> String { crate::util::p2s(p).replace('\\', "/").trim_end_matches('/').to_string() }

impl Roots {
    /// Replace every library folder path in `s` with its id (the longest folder wins where they
    /// nest); separators after it become '/'. Keys can hold several paths (kept duplicates).
    pub fn to_portable(&self, s: &str) -> String {
        let src = s.replace('\\', "/");
        let mut roots: Vec<(String, &str)> = self.ids.iter().map(|(id, p)| (norm(p), id.as_str())).filter(|(r, _)| !r.is_empty()).collect();
        roots.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
        let mut out = String::new();
        let mut i = 0;
        'outer: while i < src.len() {
            for (r, id) in &roots {
                if src[i..].starts_with(r.as_str()) {
                    let end = i + r.len();
                    let at_end = end == src.len() || src.as_bytes()[end] == b'/';
                    let at_start = i == 0 || !src.as_bytes()[i - 1].is_ascii_alphanumeric();
                    if at_end && at_start {
                        out.push_str(&format!("{{root:{id}}}"));
                        // the rest of this path, up to the end of the line
                        let stop = src[end..].find('\n').map(|n| end + n).unwrap_or(src.len());
                        out.push_str(&src[end..stop]);
                        i = stop;
                        continue 'outer;
                    }
                }
            }
            let ch = src[i..].chars().next().unwrap();
            // outside library paths, keep the original characters (a '\\' stays one)
            out.push_str(&s[i..i + ch.len_utf8()]);
            i += ch.len_utf8();
        }
        out
    }
    /// This device's form of a portable string; None when it names a folder not mapped here.
    pub fn to_local(&self, s: &str) -> Option<String> {
        let win = std::path::MAIN_SEPARATOR == '\\';
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find("{root:") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 6..];
            let j = after.find('}')?;
            let id = &after[..j];
            let (_, p) = self.ids.iter().find(|(x, _)| x == id)?;
            let base = norm(p);
            let tail_all = &after[j + 1..];
            let stop = tail_all.find('\n').unwrap_or(tail_all.len());
            let tail = &tail_all[..stop];
            if win { out.push_str(&base.replace('/', "\\")); out.push_str(&tail.replace('/', "\\")); } else { out.push_str(&base); out.push_str(tail); }
            rest = &tail_all[stop..];
        }
        out.push_str(rest);
        Some(out)
    }
    /// (root id, root-relative path with '/') of a local path.
    pub fn rel(&self, p: &Path) -> Option<(String, String)> {
        let roots: Vec<PathBuf> = self.ids.iter().map(|(_, p)| p.clone()).collect();
        crate::shared::rel_to_root(&roots, p).map(|(i, r)| (self.ids[i].0.clone(), r))
    }
    pub fn abs(&self, root: &str, rel: &str) -> Option<PathBuf> {
        self.ids.iter().find(|(id, _)| id == root).map(|(_, p)| crate::shared::abs_from(p, rel))
    }
}

pub fn game_ref_out(r: &Roots, g: &GameRef) -> GameRef { GameRef { key: r.to_portable(&g.key), ..g.clone() } }
pub fn game_ref_in(r: &Roots, g: &GameRef) -> Option<GameRef> { Some(GameRef { key: r.to_local(&g.key)?, ..g.clone() }) }
