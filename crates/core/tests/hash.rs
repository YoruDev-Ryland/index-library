mod common;
use common::*;
use lo_core::fsx::LocalFs;
use lo_core::hash::{hash_files, HashLevel, Throttle};
use lo_core::scan::{scan, ScanOpts};
use std::io::Write;
use std::sync::atomic::AtomicBool;

fn ids(f: &Fixture) -> Vec<i64> {
    let mut q = f.db.conn().prepare("SELECT id FROM files ORDER BY name").unwrap();
    q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
}
fn hashes(f: &Fixture, name: &str) -> (String, String, String) {
    f.db.conn().query_row("SELECT crc32, sha1, md5 FROM files WHERE name=?1", [name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap()
}
fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        for (n, d) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn hashes_raw_and_zip_inner_rom() {
    let f = Fixture::new();
    f.put("Nintendo - NES/raw.nes", b"hello");
    f.put("Nintendo - NES/z.zip", &zip_with(&[("readme.txt", b"x"), ("game.nes", b"hello")]));
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let t = Throttle::new(HashLevel::Maximum);
    let r = hash_files(&f.db, &LocalFs, &ids(&f), &t, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.hashed, 2);
    let want = ("3610a686".to_string(), "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".to_string(), "5d41402abc4b2a76b9719d911017c592".to_string());
    assert_eq!(hashes(&f, "raw.nes"), want);
    assert_eq!(hashes(&f, "z.zip"), want);
}

#[test]
fn unchanged_files_skipped() {
    let f = Fixture::new();
    f.put("Nintendo - NES/raw.nes", b"hello");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let t = Throttle::new(HashLevel::Maximum);
    hash_files(&f.db, &LocalFs, &ids(&f), &t, &|_| {}, &AtomicBool::new(false)).unwrap();
    let r = hash_files(&f.db, &LocalFs, &ids(&f), &t, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!((r.hashed, r.skipped), (0, 1));
}

#[test]
fn bandwidth_cap_respected() {
    let f = Fixture::new();
    f.put("Nintendo - NES/big.nes", &vec![7u8; 512 * 1024]);
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let t = Throttle::new(HashLevel::Background);
    t.set_custom_cap(Some(1024 * 1024)); // 1 MiB/s
    let start = std::time::Instant::now();
    hash_files(&f.db, &LocalFs, &ids(&f), &t, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(start.elapsed().as_secs_f64() >= 0.3, "{:?}", start.elapsed());
}

#[test]
fn unreadable_file_reported_not_fatal() {
    let f = Fixture::new();
    f.put("Nintendo - NES/a.nes", b"a");
    f.put("Nintendo - NES/b.nes", b"b");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    std::fs::remove_file(f.root().join("Nintendo - NES/a.nes")).unwrap();
    let r = hash_files(&f.db, &LocalFs, &ids(&f), &Throttle::new(HashLevel::Normal), &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.hashed, 1);
    assert_eq!(r.failed.len(), 1);
}
