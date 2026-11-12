//! Identifying a file exactly: its fingerprint in the system's No-Intro list.
use lo_core::jobs::JobEvent;
use lo_core::library::{HashScope, Library};
use sha1::Digest;
use std::sync::Arc;
use std::time::Duration;

fn sha1_hex(b: &[u8]) -> String { sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

#[test]
fn headered_roms_are_identified_by_their_data() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    // an iNES file: 16-byte header, then the ROM data No-Intro lists
    let data: Vec<u8> = (0..40960u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut file = b"NES\x1a\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
    file.extend_from_slice(&data);
    std::fs::create_dir_all(root.path().join("Nintendo - NES")).unwrap();
    std::fs::write(root.path().join("Nintendo - NES/Super Huey UH-IX (USA).nes"), &file).unwrap();
    // and an SNES file with a 512-byte copier header
    let snes: Vec<u8> = (0..131072u32).map(|i| (i * 13 % 241) as u8).collect();
    let mut sfile = vec![0u8; 512];
    sfile.extend_from_slice(&snes);
    std::fs::create_dir_all(root.path().join("Nintendo - SNES")).unwrap();
    std::fs::write(root.path().join("Nintendo - SNES/Hit the Ice (USA).smc"), &sfile).unwrap();

    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));

    let dat = |sys: &str, game: &str, rom: &str, body: &[u8]| {
        let p = home.path().join(format!("{sys}.dat"));
        std::fs::write(&p, format!("clrmamepro ( name \"{sys}\" version \"1\" )\ngame ( name \"{game}\" rom ( name \"{rom}\" size {} crc {:08x} sha1 {} ) )\n",
            body.len(), crc32fast::hash(body), sha1_hex(body))).unwrap();
        p
    };
    let db = l.db().unwrap();
    lo_core::dat::import_dat(&db, "nes", &dat("nes", "Super Huey UH-IX (USA)", "Super Huey UH-IX (USA).nes", &data)).unwrap();
    lo_core::dat::import_dat(&db, "snes", &dat("snes", "Hit the Ice (USA)", "Hit the Ice (USA).sfc", &snes)).unwrap();
    assert_eq!(lo_core::dat::verify(&db, "nes").unwrap().verified, 1, "the NES ROM by its data");
    assert_eq!(lo_core::dat::verify(&db, "snes").unwrap().verified, 1, "the SNES ROM by its data");
    let canon: Vec<String> = db.conn().prepare("SELECT canonical_id FROM games WHERE canonical_id IS NOT NULL ORDER BY 1").unwrap()
        .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(canon, ["Hit the Ice (USA)", "Super Huey UH-IX (USA)"]);
}

#[test]
fn header_lengths() {
    use lo_core::hash::header_len;
    assert_eq!(header_len("nes", b"NES\x1a\x02", Some(40976)), 16);
    assert_eq!(header_len("nes", b"\x00\x00", Some(40960)), 0);
    assert_eq!(header_len("lynx", b"LYNX\x00", Some(262208)), 64);
    let mut a78 = vec![1u8]; a78.extend_from_slice(b"ATARI7800");
    assert_eq!(header_len("atari7800", &a78, None), 128);
    assert_eq!(header_len("snes", b"", Some(1024 * 1024 + 512)), 512);
    assert_eq!(header_len("snes", b"", Some(1024 * 1024)), 0);
    assert_eq!(header_len("gba", b"NES\x1a", None), 0);
}

#[test]
fn a_game_is_compared_by_the_release_its_fingerprint_names() {
    use lo_core::library::{GameFilter, GameSort};
    use lo_core::metadata::MetaCandidate;
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..65536u32).map(|i| (i * 11 % 239) as u8).collect();
    std::fs::create_dir_all(root.path().join("Nintendo - SNES")).unwrap();
    std::fs::write(root.path().join("Nintendo - SNES/C2 - Judgment Clay.sfc"), &data).unwrap();
    std::fs::write(root.path().join("Nintendo - SNES/Rayman 3 - Hoodlum Havoc.sfc"), b"not in the list").unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let p = home.path().join("snes.dat");
    std::fs::write(&p, format!("clrmamepro ( name \"snes\" version \"1\" )\ngame ( name \"Clay Fighter 2 - Judgment Clay (Europe)\" rom ( name \"x.sfc\" size {} crc {:08x} sha1 {} ) )\n",
        data.len(), crc32fast::hash(&data), sha1_hex(&data))).unwrap();
    let db = l.db().unwrap();
    lo_core::dat::import_dat(&db, "snes", &p).unwrap();
    lo_core::dat::verify(&db, "snes").unwrap();

    let id = |t: &str| l.games(&GameFilter { search: Some(t.into()), ..Default::default() }, GameSort::Title, 0, 10).unwrap().rows[0].id;
    let (clay, rayman) = (id("Judgment"), id("Rayman"));
    assert_eq!(lo_core::metadata::release_name(&db, clay).unwrap().as_deref(), Some("Clay Fighter 2 - Judgment Clay"));
    assert_eq!(lo_core::metadata::release_name(&db, rayman).unwrap(), None, "not identified");
    let q = lo_core::metadata::query_for(&db, clay).unwrap();
    assert_eq!(q.names(), ["C2 - Judgment Clay", "Clay Fighter 2 - Judgment Clay"]);

    // waiting in review with the right game among the options, but far from the file's name
    let cand = |t: &str| MetaCandidate { source: "igdb".into(), source_id: t.into(), title: t.into(), score: 0.5, console: Some("match".into()), ..Default::default() };
    lo_core::metadata::set_review(&db, clay, &[cand("Clay Fighter 2: Judgment Clay"), cand("Clay Fighter")]).unwrap();
    // the release's box scan is already stored: it becomes the cover
    let scan = home.path().join("cache/art/libretro/snes/Clay Fighter 2 - Judgment Clay (Europe).jpg");
    std::fs::create_dir_all(scan.parent().unwrap()).unwrap();
    std::fs::write(&scan, b"jpeg").unwrap();
    assert_eq!(l.settle_review(&|_, _, _| true).unwrap(), (1, 0));
    let d = l.game_detail(clay).unwrap();
    assert_eq!(d.meta.unwrap()["title"], "Clay Fighter 2: Judgment Clay");
    let row = l.games(&GameFilter { search: Some("Judgment".into()), ..Default::default() }, GameSort::Title, 0, 10).unwrap().rows.remove(0);
    assert_eq!(row.cover.as_deref(), Some(scan.to_string_lossy().as_ref()));
}

#[test]
fn arcade_sets_are_named_by_the_arcade_list() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Neo-Geo")).unwrap();
    std::fs::write(root.path().join("Neo-Geo/aof.zip"), b"x").unwrap();
    std::fs::write(root.path().join("Neo-Geo/kof98n.zip"), b"y").unwrap();
    std::fs::write(root.path().join("Neo-Geo/gg-bios.zip"), b"z").unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let list = home.path().join("arcade.dat");
    std::fs::write(&list, "clrmamepro ( name \"FBNeo - Arcade Games\" )\n\
        game ( name \"Art of Fighting / Ryuuko no Ken (NGM-044)(NGH-044)\" rom ( name aof.zip size 1 crc 00000000 ) )\n\
        game ( name \"The King of Fighters '98 - The Slugfest / King of Fighters '98 - dream match never ends (NGM-2420)\" rom ( name kof98.zip size 1 crc 00000000 ) )\n").unwrap();
    let db = l.db().unwrap();
    assert_eq!(lo_core::metadata::offline::arcade_import(&db, &list).unwrap(), 2);
    let names: Vec<(String, Option<String>)> = db.conn().prepare("SELECT id, title FROM games ORDER BY title").unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).unwrap().map(|r| r.unwrap())
        .map(|(id, t)| (t, lo_core::metadata::release_name(&db, id).unwrap())).collect();
    assert_eq!(names, [
        ("aof".to_string(), Some("Art of Fighting".to_string())),
        ("gg-bios".to_string(), None),
        ("kof98n".to_string(), Some("The King of Fighters '98 - The Slugfest".to_string())), // a variant of kof98
    ]);
}

#[test]
fn box_scans_are_named_and_stored_like_libretro_s() {
    use lo_core::metadata::boxart;
    assert_eq!(boxart::url("snes", "Super Mario World (USA)").unwrap(),
        "https://thumbnails.libretro.com/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts/Super%20Mario%20World%20%28USA%29.png");
    assert_eq!(boxart::file_name("Q*bert's Qubes: Part 1/2 (USA)"), "Q_bert's Qubes_ Part 1_2 (USA)");
    assert_eq!(boxart::url("neogeo", "aof"), None, "no scans for arcade sets");
    // a tall scan is stored as a smaller JPEG
    let dir = tempfile::tempdir().unwrap();
    let img = image::RgbImage::from_pixel(600, 1200, image::Rgb([200, 30, 30]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let p = boxart::store(&png, &dir.path().join("Game (USA)")).unwrap();
    assert_eq!(p.extension().unwrap(), "jpg");
    let back = image::open(&p).unwrap();
    assert_eq!((back.width(), back.height()), (400, 800));
}
