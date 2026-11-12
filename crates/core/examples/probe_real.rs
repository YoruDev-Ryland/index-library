fn main() {
    for a in std::env::args().skip(1) {
        let p = std::path::Path::new(&a);
        let r = lo_core::probe::probe_file(lo_core::probe::ProbeKind::N3ds, &lo_core::fsx::LocalFs, p);
        println!("{:?} {:?}", p.file_name().unwrap(), r.map(|x| (x.id, x.encrypted, x.extra)));
    }
}
