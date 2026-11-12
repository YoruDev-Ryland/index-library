//! Switch naming, grouping and duplicates — fixtures mirror real-world library names.
mod common;
use common::*;
use lo_core::dupes::{find_dupes, DupeReason};
use lo_core::fsx::LocalFs;
use lo_core::probe::{probe_file, ProbeKind};
use lo_core::rename::{plan_renames, Problem};
use lo_core::scan::{scan, ScanOpts};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

/// Minimal PFS0 (NSP/NSZ) with the given entries.
fn pfs0(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut strs = Vec::new();
    let mut offs = Vec::new();
    for (n, _) in entries { offs.push(strs.len() as u32); strs.extend_from_slice(n.as_bytes()); strs.push(0); }
    while strs.len() % 16 != 0 { strs.push(0); }
    let mut out = b"PFS0".to_vec();
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    out.extend_from_slice(&(strs.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut data_off = 0u64;
    for (i, (_, d)) in entries.iter().enumerate() {
        out.extend_from_slice(&data_off.to_le_bytes());
        out.extend_from_slice(&(d.len() as u64).to_le_bytes());
        out.extend_from_slice(&offs[i].to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        data_off += d.len() as u64;
    }
    out.extend_from_slice(&strs);
    for (_, d) in entries { out.extend_from_slice(d); }
    out
}
fn cnmt(ty: &str, id: &str, ver: u32) -> Vec<u8> {
    format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ContentMeta>\n  <Type>{ty}</Type>\n  <Id>0x{id}</Id>\n  <Version>{ver}</Version>\n</ContentMeta>\n").into_bytes()
}
fn nsp_tik(tid: &str) -> Vec<u8> {
    let tik = format!("{}0000000000000000.tik", tid.to_lowercase());
    pfs0(&[(&tik, b"t"), ("aaaa.nca", b"nca")])
}
fn nsp_cnmt(ty: &str, tid: &str, ver: u32) -> Vec<u8> {
    let x = cnmt(ty, &tid.to_lowercase(), ver);
    pfs0(&[("bbbb.nca", b"nca"), ("cccc.cnmt.nca", b"m"), ("cccc.cnmt.xml", &x)])
}
fn xci(fill: u8) -> Vec<u8> { let mut v = vec![fill; 0x400]; v[0x100..0x104].copy_from_slice(b"HEAD"); v }

fn index(f: &Fixture) {
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let sys = lo_core::systems::by_id("switch").unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, sys, &AtomicBool::new(false)).unwrap();
    lo_core::grouping::assign_games(&f.db, sys).unwrap();
}
fn games(f: &Fixture) -> Vec<(String, i64)> {
    let mut q = f.db.conn().prepare("SELECT g.title, count(f.id) FROM games g JOIN files f ON f.game_id=g.id WHERE g.system_id='switch' GROUP BY g.id ORDER BY g.title").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
}
fn all_games(f: &Fixture) -> Vec<i64> { let mut q = f.db.conn().prepare("SELECT id FROM games").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() }
/// relative src → (relative dst, problem) for every planned change
fn plan(f: &Fixture) -> HashMap<String, (String, Option<Problem>)> {
    let rel = |p: &std::path::Path| p.strip_prefix(f.root().join("Nintendo - Switch")).unwrap().to_string_lossy().into_owned();
    plan_renames(&f.db, &LocalFs, &all_games(f)).unwrap().into_iter()
        .filter(|i| i.problem != Some(Problem::Unchanged))
        .map(|i| (rel(&i.src), (rel(&i.dst), i.problem))).collect()
}
const S: &str = "Nintendo - Switch/";

#[test]
fn probe_reads_ids_from_tickets_cnmt_and_spots_xci() {
    let f = Fixture::new();
    let a = f.put("a.nsp", &nsp_tik("0100849000BDA000"));
    let b = f.put("b.nsp", &nsp_cnmt("Application", "0100ABF008968000", 0));
    let c = f.put("c.nsp", &nsp_cnmt("Patch", "010074F013262800", 458752));
    let d = f.put("d.nsp", &xci(7));
    let pa = probe_file(ProbeKind::Switch, &LocalFs, &a).unwrap();
    assert_eq!(pa.id.as_deref(), Some("0100849000BDA000"));
    let pb = probe_file(ProbeKind::Switch, &LocalFs, &b).unwrap();
    assert_eq!(pb.id.as_deref(), Some("0100ABF008968000"));
    assert_eq!(pb.extra["version"], "0");
    assert_eq!(pb.extra["type"], "Application");
    let pc = probe_file(ProbeKind::Switch, &LocalFs, &c).unwrap();
    assert_eq!(pc.extra["version"], "458752");
    assert_eq!(pc.extra["type"], "Patch");
    let pd = probe_file(ProbeKind::Switch, &LocalFs, &d).unwrap();
    assert_eq!(pd.extra["container"], "xci");
    assert_eq!(pd.id, None);
    let pg = probe_file(ProbeKind::Switch, &LocalFs, &f.put("g.nsp", b"garbage")).unwrap();
    assert_eq!(pg.id, None);
}

#[test]
fn no_empty_brackets_and_unbracketed_ids_parsed() {
    let f = Fixture::new();
    f.put(&format!("{S}Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu 010003F003A34000 v0.nsp"), b"x");
    f.put(&format!("{S}Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu 010003F003A34800 v131072.nsp"), b"y");
    f.put(&format!("{S}Keys&Firm/Firmware.22.5.0.zip"), b"z");
    f.put(&format!("{S}Keys&Firm/ProdKeys.NET-v22.5.0.zip"), b"z");
    f.put(&format!("{S}Xenoblade Chronicles 3/60fps wip8.rar"), b"z");
    f.put(&format!("{S}Xenoblade Chronicles 3/Xenoblade-Chronicles-3-Switch-NSP-Update-2.2.0.rar"), b"z");
    f.put(&format!("{S}Xenoblade Chronicles 3/Xenoblade Chronicles 3 [4DLC][US] NSP.rar"), b"z");
    f.put(&format!("{S}TheLegendofZeldaTearsoftheKingdom-Update1.2.1-NSP-Romsim.com.rar"), b"z");
    index(&f);
    let p = plan(&f);
    for (src, (dst, _)) in &p { assert!(!dst.contains("[]") && !dst.contains("[ ]"), "{src} → {dst}"); }
    assert_eq!(p["Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu 010003F003A34000 v0.nsp"].0, "Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu [010003F003A34000][v0].nsp");
    assert_eq!(p["Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu 010003F003A34800 v131072.nsp"].0, "Pokemon Let's Go Pikachu/Pokemon Let's Go Pikachu [010003F003A34800][v131072].nsp");
    // things that aren't plainly a game, or whose name carries info we can't rebuild, are left alone
    for keep in ["Keys&Firm/Firmware.22.5.0.zip", "Keys&Firm/ProdKeys.NET-v22.5.0.zip", "Xenoblade Chronicles 3/60fps wip8.rar",
                 "Xenoblade Chronicles 3/Xenoblade-Chronicles-3-Switch-NSP-Update-2.2.0.rar", "Xenoblade Chronicles 3/Xenoblade Chronicles 3 [4DLC][US] NSP.rar",
                 "TheLegendofZeldaTearsoftheKingdom-Update1.2.1-NSP-Romsim.com.rar"] {
        assert!(!p.contains_key(keep), "{keep} → {:?}", p.get(keep));
    }
}

#[test]
fn scene_tags_dropped_dlc_names_kept_multipart_extensions_kept() {
    let f = Fixture::new();
    f.put(&format!("{S}Fire Emblem™ Engage [0100A6301214E000]/Fire Emblem™ Engage [0100A6301214E000][v0].xci"), &xci(1));
    f.put(&format!("{S}Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part1.rar"), b"1");
    f.put(&format!("{S}Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part2.rar"), b"2");
    f.put(&format!("{S}Fire Emblem Engage [0100A6301214E800][v327680][US].nsp.rar"), b"3");
    f.put(&format!("{S}Pokemon Sword/Pokemon Sword  [2 DLC Expansion Pass]/Pokemon Sword [The Crown Tundra][0100ABF008969002][US][v0].nsp"), b"4");
    f.put(&format!("{S}Pokemon Sword/Pokemon Sword [0100ABF008968800][v458752](nsw2u).nsp"), b"5");
    f.put(&format!("{S}Legend of Zelda Tears of the Kingdom/The Legend of Zelda Tears of the Kingdom [0100F2C0115B6000][v0][US](nsw2u.com).xci"), &xci(2));
    f.put(&format!("{S}Pokemon Shield/Pokemon Shield [DLC][01008DB008C2D001][v0].nsp"), b"6");
    index(&f);
    let p = plan(&f);
    assert_eq!(p["Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part1.rar"].0, "Fire Emblem™ Engage [0100A6301214E000][v0].nsp.part1.rar");
    assert_eq!(p["Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part2.rar"].0, "Fire Emblem™ Engage [0100A6301214E000][v0].nsp.part2.rar");
    assert_eq!(p["Fire Emblem Engage [0100A6301214E800][v327680][US].nsp.rar"].0, "Fire Emblem™ Engage [0100A6301214E800][v327680].nsp.rar");
    for (_, (_, prob)) in &p { assert_eq!(*prob, None); }
    assert_eq!(p["Pokemon Sword/Pokemon Sword  [2 DLC Expansion Pass]/Pokemon Sword [The Crown Tundra][0100ABF008969002][US][v0].nsp"].0,
               "Pokemon Sword/Pokemon Sword  [2 DLC Expansion Pass]/Pokemon Sword - The Crown Tundra [0100ABF008969002][v0].nsp");
    assert_eq!(p["Legend of Zelda Tears of the Kingdom/The Legend of Zelda Tears of the Kingdom [0100F2C0115B6000][v0][US](nsw2u.com).xci"].0,
               "Legend of Zelda Tears of the Kingdom/The Legend of Zelda Tears of the Kingdom [0100F2C0115B6000][v0].xci");
    assert_eq!(p["Pokemon Shield/Pokemon Shield [DLC][01008DB008C2D001][v0].nsp"].0, "Pokemon Shield/Pokemon Shield [01008DB008C2D001][v0].nsp");
}

#[test]
fn header_beats_filename_and_misnamed_xci_gets_xci_extension() {
    let f = Fixture::new();
    // base mislabelled with the update id; the header says Application 0100ABF008968000
    f.put(&format!("{S}Pokemon Sword/Pokemon Sword [0100ABF008968800][v0](nsw2u.com).nsp"), &nsp_cnmt("Application", "0100ABF008968000", 0));
    f.put(&format!("{S}Xenoblade Chronicles™ 3 [010074F013262000]/Xenoblade Chronicles™ 3 [010074F013262000][v0].nsp"), &nsp_tik("010074F013262000"));
    f.put(&format!("{S}Xenoblade Chronicles 3/Xenoblade20Chronicles20320v2.2.1.nsp"), &nsp_cnmt("Patch", "010074F013262800", 458752));
    f.put(&format!("{S}Pokemon Violet/Pokemon Violet.nsp"), &nsp_cnmt("Application", "01008F6008C5E000", 0));
    f.put(&format!("{S}I Am Setsuna [0100849000BDA000]/I Am Setsuna [0100849000BDA000][v0].nsp"), &nsp_tik("0100849000BDA000"));
    f.put(&format!("{S}i-am-setsuna.nsp"), &xci(3));
    index(&f);
    let p = plan(&f);
    assert_eq!(p["Pokemon Sword/Pokemon Sword [0100ABF008968800][v0](nsw2u.com).nsp"].0, "Pokemon Sword/Pokemon Sword [0100ABF008968000][v0].nsp");
    assert_eq!(p["Xenoblade Chronicles 3/Xenoblade20Chronicles20320v2.2.1.nsp"].0, "Xenoblade Chronicles 3/Xenoblade Chronicles™ 3 [010074F013262800][v458752].nsp");
    assert_eq!(p["Pokemon Violet/Pokemon Violet.nsp"].0, "Pokemon Violet/Pokemon Violet [01008F6008C5E000][v0].nsp");
    assert_eq!(p["i-am-setsuna.nsp"].0, "I Am Setsuna.xci");
}

#[test]
fn copies_under_other_names_join_the_game() {
    let f = Fixture::new();
    f.put(&format!("{S}I Am Setsuna [0100849000BDA000]/I Am Setsuna [0100849000BDA000][v0].nsp"), &nsp_tik("0100849000BDA000"));
    f.put(&format!("{S}I Am Setsuna [0100849000BDA000]/I Am Setsuna [0100849000BDA800][v65536].nsp"), b"upd");
    f.put(&format!("{S}I Am Setsuna/i-am-setsuna.nsp"), &xci(3));
    f.put(&format!("{S}i-am-setsuna.nsp"), &xci(3));
    f.put(&format!("{S}Pokémon™ Legends꞉ Arceus [01001F5010DFA000]/Pokémon™ Legends꞉ Arceus [01001F5010DFA000][v0].xci"), &xci(4));
    f.put(&format!("{S}Pokemon Legends Arceus/Pokemon Legends Arceus.xci"), &xci(5));
    f.put(&format!("{S}Xenoblade Chronicles 3/Xenoblade Chronicles 3 [010074F013262000][v0][US](nsw2u.in).nsp"), b"b");
    f.put(&format!("{S}Xenoblade Chronicles 3/60fps wip8.rar"), b"z");
    index(&f);
    assert_eq!(games(&f), vec![
        ("I Am Setsuna".into(), 4),
        ("Pokémon™ Legends꞉ Arceus".into(), 2),
        ("Xenoblade Chronicles 3".into(), 2),
    ]);
    // group keys keep the 12-digit form so metadata already fetched stays attached
    let keys: Vec<String> = { let mut q = f.db.conn().prepare("SELECT group_key FROM games ORDER BY group_key").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    assert_eq!(keys, vec!["tid:01001F5010DF", "tid:010074F01326", "tid:0100849000BD"]);
}

#[test]
fn switch_duplicates_across_folders() {
    let f = Fixture::new();
    // the user's three copies of I Am Setsuna
    f.put(&format!("{S}I Am Setsuna [0100849000BDA000]/I Am Setsuna [0100849000BDA000][v0].nsp"), &nsp_tik("0100849000BDA000"));
    f.put(&format!("{S}I Am Setsuna [0100849000BDA000]/I Am Setsuna [0100849000BDA800][v65536].nsp"), b"upd");
    f.put(&format!("{S}I Am Setsuna/i-am-setsuna.nsp"), &xci(3));
    f.put(&format!("{S}i-am-setsuna.nsp"), &xci(3));
    // same base in two folders; an older update next to a newer one
    f.put(&format!("{S}Pokemon Scarlet/Pokemon Scarlet [0100A3D008C5C000].nsp"), &nsp_cnmt("Application", "0100A3D008C5C000", 0));
    f.put(&format!("{S}Pokemon Scarlett/Pokemon Scarlet [0100A3D008C5C000].nsp"), &nsp_cnmt("Application", "0100A3D008C5C000", 0));
    f.put(&format!("{S}Pokémon™ Scarlet [0100A3D008C5C000]/Pokémon™ Scarlet [0100A3D008C5C800][v786432].nsz"), b"new");
    f.put(&format!("{S}Pokemon Scarlet/Pokemon Scarlet [0100A3D008C5C800][v131072].nsp"), b"old");
    // a multi-part archive of a base that also exists unpacked: both parts are extras, together
    f.put(&format!("{S}Fire Emblem™ Engage [0100A6301214E000]/Fire Emblem™ Engage [0100A6301214E000][v0].xci"), &xci(1));
    f.put(&format!("{S}Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part1.rar"), b"1");
    f.put(&format!("{S}Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part2.rar"), b"2");
    // base + update + DLC of one game are not duplicates of each other
    f.put(&format!("{S}Atelier [0100D9D00EE8C000]/Atelier [0100D9D00EE8C000][v0].nsp"), b"a");
    f.put(&format!("{S}Atelier [0100D9D00EE8C000]/Atelier [0100D9D00EE8C800][v65536].nsp"), b"b");
    f.put(&format!("{S}Atelier [0100D9D00EE8C000]/Atelier [0100D9D00EE8D001][v0].nsp"), b"c");
    // equal copies: the one filed in a "Title [id]" folder is kept
    f.put(&format!("{S}Pokemon Let's Go Eevee/Pokemon Let's Go Eevee 0100187003A36800 v131072.nsp"), b"upd-bigger-copy");
    f.put(&format!("{S}Pokémon™꞉ Let’s Go, Eevee! [0100187003A36000]/Pokémon™꞉ Let’s Go, Eevee! [0100187003A36800][v131072].nsp"), b"upd");
    // a lone multi-part archive is one copy, not two
    f.put(&format!("{S}Solo [0100000000010000][v0].nsp.part1.rar"), b"1");
    f.put(&format!("{S}Solo [0100000000010000][v0].nsp.part2.rar"), b"2");
    index(&f);
    let d = find_dupes(&f.db, Some("switch")).unwrap();
    let name = |id: i64| -> String { f.db.conn().query_row("SELECT name FROM files WHERE id=?1", [id], |r| r.get(0)).unwrap() };
    let by_title = |t: &str| d.iter().filter(|g| g.title == t).collect::<Vec<_>>();

    let s = by_title("I Am Setsuna");
    assert_eq!(s.len(), 1, "{d:#?}");
    assert_eq!(s[0].files.len(), 3);
    assert_eq!(name(s[0].keep), "I Am Setsuna [0100849000BDA000][v0].nsp");
    assert_eq!(s[0].reason, DupeReason::SameTitleDiffFormat, "an xci with no known version is not an older update");

    let sc = by_title("Pokémon™ Scarlet");
    assert_eq!(sc.len(), 2, "{d:#?}");
    let base = sc.iter().find(|g| g.files.len() == 2 && g.reason != DupeReason::OlderUpdate).expect("base copies");
    assert!(base.files.iter().all(|f| name(*f) == "Pokemon Scarlet [0100A3D008C5C000].nsp"));
    let upd = sc.iter().find(|g| g.reason == DupeReason::OlderUpdate).expect("older update");
    assert_eq!(name(upd.keep), "Pokémon™ Scarlet [0100A3D008C5C800][v786432].nsz");

    let fe = by_title("Fire Emblem™ Engage");
    assert_eq!(fe.len(), 1, "{d:#?}");
    assert_eq!(name(fe[0].keep), "Fire Emblem™ Engage [0100A6301214E000][v0].xci");
    assert_eq!(fe[0].files.len(), 3);
    assert!(fe[0].keep_also.is_empty());

    let ev = by_title("Pokémon™꞉ Let’s Go, Eevee!");
    assert_eq!(ev.len(), 1, "{d:#?}");
    assert_eq!(name(ev[0].keep), "Pokémon™꞉ Let’s Go, Eevee! [0100187003A36800][v131072].nsp");

    assert!(by_title("Atelier").is_empty(), "{d:#?}");
    assert!(by_title("Solo").is_empty(), "{d:#?}");
}

/// A RAR5 archive storing (not compressing) the given files, as the scene releases do.
fn rar5_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    fn vint(mut v: u64, out: &mut Vec<u8>) { loop { let b = (v & 0x7f) as u8; v >>= 7; if v == 0 { out.push(b); break; } out.push(b | 0x80); } }
    fn block(fields: Vec<u8>, out: &mut Vec<u8>) { out.extend_from_slice(&[0, 0, 0, 0]); vint(fields.len() as u64, out); out.extend_from_slice(&fields); }
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    block(vec![1, 0, 0], &mut out);
    for (name, data) in files {
        let mut f = Vec::new();
        vint(2, &mut f); vint(2, &mut f); vint(data.len() as u64, &mut f); // file header, has data
        vint(0, &mut f); vint(data.len() as u64, &mut f); vint(0, &mut f); // flags, size, attributes
        vint(0, &mut f); vint(0, &mut f); // stored, host os
        vint(name.len() as u64, &mut f); f.extend_from_slice(name.as_bytes());
        block(f, &mut out);
        out.extend_from_slice(data);
    }
    block(vec![5, 0, 0], &mut out);
    out
}

#[test]
fn archives_are_compared_by_their_content_and_unique_data_is_never_archived() {
    let f = Fixture::new();
    let dir = "Nintendo - Switch/Game [0100000000010000]";
    // the update unpacked, and again inside a stored RAR with its pieces in another order
    let upd = pfs0(&[("u1.nca", b"one"), ("u2.cnmt.nca", b"m"), ("0100000000010800000000000000000e.tik", b"t")]);
    let upd_repacked = pfs0(&[("0100000000010800000000000000000e.tik", b"t"), ("u2.cnmt.nca", b"m"), ("u1.nca", b"one")]);
    f.put(&format!("{dir}/Game [0100000000010000][v0].nsp"), &pfs0(&[("b1.nca", b"base"), ("0100000000010000000000000000000e.tik", b"t")]));
    f.put(&format!("{dir}/Game [0100000000010800][v65536].nsp"), &upd);
    f.put("Nintendo - Switch/Game [0100000000010800][v65536][US].nsp.rar", &rar5_stored(&[("Game [0100000000010800][v65536][US].nsp", &upd_repacked)]));
    // two DLC unpacked, and both again in one archive
    let d1 = pfs0(&[("d1.nca", b"d1"), ("0100000000011001000000000000000e.tik", b"t")]);
    let d2 = pfs0(&[("d2.nca", b"d2"), ("0100000000011002000000000000000e.tik", b"t")]);
    f.put(&format!("{dir}/Game [0100000000011001][v0].nsp"), &d1);
    f.put(&format!("{dir}/Game [0100000000011002][v0].nsp"), &d2);
    f.put("Nintendo - Switch/Game [2DLC][US] NSP.rar", &rar5_stored(&[("Game NSP/Game [DLC 1][0100000000011001][v0].nsp", &d1), ("Game NSP/Game [DLC 2][0100000000011002][v0].nsp", &d2)]));
    // the base game again, but a different build (other pieces): unique, must be kept
    f.put("Nintendo - Switch/Game [0100000000010000][v0][US].nsp.rar", &rar5_stored(&[("Game [0100000000010000][v0][US].nsp", &pfs0(&[("b9.nca", b"other"), ("0100000000010000000000000000000e.tik", b"t")]))]));
    // a DLC archive missing one of its pieces from the unpacked files: unique, must be kept
    // other games filed one per folder, so the system's layout is clear
    f.put("Nintendo - Switch/Other [0100000000020000]/Other [0100000000020000][v0].nsp", &pfs0(&[("o.nca", b"o"), ("0100000000020000000000000000000e.tik", b"t")]));
    f.put("Nintendo - Switch/Third [0100000000030000]/Third [0100000000030000][v0].nsp", &pfs0(&[("p.nca", b"p"), ("0100000000030000000000000000000e.tik", b"t")]));
    f.put("Nintendo - Switch/Game [3DLC][US] NSP.rar", &rar5_stored(&[("x/Game [0100000000011001][v0].nsp", &d1), ("x/Game [0100000000011003][v0].nsp", &pfs0(&[("d3.nca", b"d3"), ("0100000000011003000000000000000e.tik", b"t")]))]));
    index(&f);

    let d = find_dupes(&f.db, Some("switch")).unwrap();
    let proof_of = |name: &str| d.iter().find_map(|g| g.names.iter().position(|n| n.ends_with(name)).map(|i| g.proofs[i]));
    use lo_core::dupes::Proof;
    assert_eq!(proof_of("[v65536][US].nsp.rar"), Some(Proof::SameContent), "{d:#?}");
    assert_eq!(proof_of("[2DLC][US] NSP.rar"), Some(Proof::SameContent), "{d:#?}");
    assert_eq!(proof_of("[0100000000010000][v0][US].nsp.rar"), Some(Proof::Different), "{d:#?}");
    assert_ne!(proof_of("[3DLC][US] NSP.rar").map(|p| p.safe()), Some(true), "{d:#?}");

    // tidying the folders archives only the proven copies; the rest moves in with the game
    let r = lo_core::layout::check(&f.db, &f.settings, Some("switch")).unwrap();
    let item = r[0].items.iter().find(|i| i.title == "Game").expect("split game");
    let spare: Vec<String> = item.duplicates.iter().map(|s| s.src.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let moved: Vec<String> = item.moves.iter().map(|m| m.src.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(spare.len(), 2, "{spare:?}");
    assert!(spare.iter().all(|n| n.contains("[v65536][US]") || n.contains("[2DLC]")), "{spare:?}");
    assert!(moved.iter().any(|n| n.contains("[v0][US].nsp.rar")) && moved.iter().any(|n| n.contains("[3DLC]")), "{moved:?}");
}
