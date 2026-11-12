#![allow(dead_code)]
use lo_core::config::Settings;
use lo_core::db::Db;
use lo_core::fsx::{Entry, LocalFs, Meta, Transport};
use std::io;
use std::path::{Path, PathBuf};

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub settings: Settings,
    pub db: Db,
}

impl Fixture {
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.library_roots = vec![dir.path().to_path_buf()];
        settings.scan_concurrency = 4;
        Fixture { dir, settings, db: Db::open_in_memory().unwrap() }
    }
    pub fn root(&self) -> &Path { self.dir.path() }
    pub fn put(&self, rel: &str, data: &[u8]) -> PathBuf {
        let p = self.root().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, data).unwrap();
        p
    }
    pub fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

/// Transport that fails listing for any dir whose name matches.
pub struct FailingFs { pub fail_dir_name: String }
impl Transport for FailingFs {
    fn list(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        if dir.file_name().map(|n| n.to_string_lossy() == self.fail_dir_name).unwrap_or(false) {
            return Err(io::Error::new(io::ErrorKind::Other, "simulated NAS error"));
        }
        LocalFs.list(dir)
    }
    fn stat(&self, p: &Path) -> io::Result<Meta> { LocalFs.stat(p) }
    fn read_range(&self, p: &Path, o: u64, l: usize) -> io::Result<Vec<u8>> { LocalFs.read_range(p, o, l) }
    fn open_read(&self, p: &Path) -> io::Result<Box<dyn io::Read + Send>> { LocalFs.open_read(p) }
    fn open_seek(&self, p: &Path) -> io::Result<Box<dyn lo_core::fsx::ReadSeek>> { LocalFs.open_seek(p) }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> { LocalFs.rename(a, b) }
    fn copy(&self, a: &Path, b: &Path, f: &dyn Fn(u64)) -> io::Result<u64> { LocalFs.copy(a, b, f) }
    fn remove_file(&self, p: &Path) -> io::Result<()> { LocalFs.remove_file(p) }
    fn remove_dir_all(&self, p: &Path) -> io::Result<()> { LocalFs.remove_dir_all(p) }
    fn remove_dir(&self, p: &Path) -> io::Result<()> { LocalFs.remove_dir(p) }
    fn create_dir_all(&self, p: &Path) -> io::Result<()> { LocalFs.create_dir_all(p) }
    fn write_atomic(&self, p: &Path, d: &[u8]) -> io::Result<()> { LocalFs.write_atomic(p, d) }
    fn append(&self, p: &Path, d: &[u8]) -> io::Result<()> { LocalFs.append(p, d) }
}
