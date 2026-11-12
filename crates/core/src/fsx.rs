use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// nanoseconds since the Unix epoch
    pub mtime: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub is_dir: bool,
    pub size: u64,
    /// nanoseconds since the Unix epoch
    pub mtime: i64,
}

/// All file access goes through this so devices (ADB/MTP) can plug in later.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

pub trait Transport: Send + Sync {
    fn list(&self, dir: &Path) -> io::Result<Vec<Entry>>;
    fn stat(&self, p: &Path) -> io::Result<Meta>;
    fn exists(&self, p: &Path) -> bool { self.stat(p).is_ok() }
    fn read_range(&self, p: &Path, off: u64, len: usize) -> io::Result<Vec<u8>>;
    fn open_read(&self, p: &Path) -> io::Result<Box<dyn Read + Send>>;
    fn open_seek(&self, p: &Path) -> io::Result<Box<dyn ReadSeek>>;
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()>;
    fn copy(&self, a: &Path, b: &Path, progress: &dyn Fn(u64)) -> io::Result<u64>;
    fn remove_file(&self, p: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, p: &Path) -> io::Result<()>;
    /// Remove an empty directory; fails if it has any entries.
    fn remove_dir(&self, p: &Path) -> io::Result<()>;
    fn create_dir_all(&self, p: &Path) -> io::Result<()>;
    fn write_atomic(&self, p: &Path, data: &[u8]) -> io::Result<()>;
    fn append(&self, p: &Path, data: &[u8]) -> io::Result<()>;
}

fn mtime_of(m: &std::fs::Metadata) -> i64 {
    m.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64).unwrap_or(0)
}

pub struct LocalFs;

impl Transport for LocalFs {
    fn list(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            // metadata() on DirEntry is cheap on Linux (statx) and free on Windows.
            // A stat error fails the whole listing (the scanner then keeps this folder's index
            // rows) rather than making the entry look deleted. Dangling symlinks are skipped.
            let m = e.metadata()?;
            let m = if m.file_type().is_symlink() { match std::fs::metadata(e.path()) { Ok(m) => m, Err(_) => continue } } else { m };
            out.push(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir: m.is_dir(),
                size: if m.is_dir() { 0 } else { m.len() },
                mtime: mtime_of(&m),
            });
        }
        Ok(out)
    }
    fn stat(&self, p: &Path) -> io::Result<Meta> {
        let m = std::fs::metadata(p)?;
        Ok(Meta { is_dir: m.is_dir(), size: m.len(), mtime: mtime_of(&m) })
    }
    fn read_range(&self, p: &Path, off: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut f = std::fs::File::open(p)?;
        f.seek(SeekFrom::Start(off))?;
        let mut buf = Vec::with_capacity(len);
        f.take(len as u64).read_to_end(&mut buf)?;
        Ok(buf)
    }
    fn open_read(&self, p: &Path) -> io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::fs::File::open(p)?))
    }
    fn open_seek(&self, p: &Path) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(std::fs::File::open(p)?))
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> { std::fs::rename(a, b) }
    fn copy(&self, a: &Path, b: &Path, progress: &dyn Fn(u64)) -> io::Result<u64> {
        let mut r = std::fs::File::open(a)?;
        let mut w = std::fs::File::create(b)?;
        let mut buf = vec![0u8; 4 << 20];
        let mut total = 0u64;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 { break; }
            w.write_all(&buf[..n])?;
            total += n as u64;
            progress(total);
        }
        w.sync_all()?;
        Ok(total)
    }
    fn remove_file(&self, p: &Path) -> io::Result<()> { std::fs::remove_file(p) }
    fn remove_dir_all(&self, p: &Path) -> io::Result<()> { std::fs::remove_dir_all(p) }
    fn remove_dir(&self, p: &Path) -> io::Result<()> { std::fs::remove_dir(p) }
    fn create_dir_all(&self, p: &Path) -> io::Result<()> { std::fs::create_dir_all(p) }
    fn write_atomic(&self, p: &Path, data: &[u8]) -> io::Result<()> {
        if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; }
        let mut tmp = p.as_os_str().to_owned();
        tmp.push(".lo-tmp");
        let tmp = std::path::PathBuf::from(tmp);
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, p)
    }
    fn append(&self, p: &Path, data: &[u8]) -> io::Result<()> {
        if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
        f.write_all(data)?;
        f.sync_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_atomic_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("m.json");
        let fs = LocalFs;
        fs.write_atomic(&p, b"one").unwrap();
        fs.write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(d.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1);
    }
    #[test]
    fn list_and_read_range() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.bin"), b"0123456789").unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let fs = LocalFs;
        let mut e = fs.list(d.path()).unwrap();
        e.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(e[0].name, "a.bin"); assert_eq!(e[0].size, 10); assert!(!e[0].is_dir);
        assert!(e[1].is_dir);
        assert_eq!(fs.read_range(&d.path().join("a.bin"), 3, 4).unwrap(), b"3456");
        assert_eq!(fs.read_range(&d.path().join("a.bin"), 8, 10).unwrap(), b"89");
    }
}

/// Every file under `dir` (recursive) with its size; unreadable folders are skipped.
pub fn walk_files(fs: &dyn Transport, dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs.list(&d) else { continue };
        for e in entries {
            let p = d.join(&e.name);
            if e.is_dir { stack.push(p) } else { out.push((p, e.size)) }
        }
    }
    out
}
