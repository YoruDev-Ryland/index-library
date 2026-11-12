//! Read-only scan timing: cargo run --release -p lo-core --example scan_real -- <root> <db>
use std::sync::atomic::AtomicBool;
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let mut s = lo_core::config::Settings::default();
    s.library_roots = vec![a[1].clone().into()];
    let db = lo_core::db::Db::open(std::path::Path::new(&a[2]))?;
    for full in [false, false] {
        let t = std::time::Instant::now();
        let r = lo_core::scan::scan(&db, &lo_core::fsx::LocalFs, &s, lo_core::scan::ScanOpts { full }, &|_| {}, &AtomicBool::new(false))?;
        println!("{:?} listed={} skipped={} added={} removed={} errors={} unknown={:?}", t.elapsed(), r.dirs_listed, r.dirs_skipped, r.files_added, r.files_removed, r.errors.len(), r.unknown_folders);
    }
    let n: i64 = db.conn().query_row("SELECT count(*) FROM files", [], |r| r.get(0))?;
    let b: i64 = db.conn().query_row("SELECT sum(size) FROM files", [], |r| r.get(0))?;
    println!("files={n} bytes={:.1}GB", b as f64 / 1e9);
    Ok(())
}
