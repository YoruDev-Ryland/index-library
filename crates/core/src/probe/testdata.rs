//! Synthetic 3DS headers for tests (layout per 3dbrew NCSD/NCCH/CIA/TMD).
pub fn ncch(program_id: u64, nocrypto: bool) -> Vec<u8> {
    let mut n = vec![0u8; 0x200];
    n[0x100..0x104].copy_from_slice(b"NCCH");
    n[0x118..0x120].copy_from_slice(&program_id.to_le_bytes());
    if nocrypto { n[0x18F] = 0x04; }
    n
}
pub fn ncsd(program_id: u64, nocrypto: bool) -> Vec<u8> {
    let mut d = vec![0u8; 0x4000];
    d[0x100..0x104].copy_from_slice(b"NCSD");
    d[0x120..0x124].copy_from_slice(&0x20u32.to_le_bytes()); // partition 0 at 0x20 * 0x200 = 0x4000
    d.extend(ncch(program_id, nocrypto));
    d
}
fn align64(x: usize) -> usize { (x + 63) & !63 }
pub fn cia(title_id: u64, cia_encrypted: bool, nocrypto: bool) -> Vec<u8> {
    let hdr = 0x2020usize; let cert = 0xA00usize; let tik = 0x350usize;
    let tmd_size = 0x140 + 0xC4 + 0x900 + 0x30;
    let mut d = vec![0u8; align64(hdr) + align64(cert) + align64(tik)];
    d[0..4].copy_from_slice(&(hdr as u32).to_le_bytes());
    d[0x08..0x0C].copy_from_slice(&(cert as u32).to_le_bytes());
    d[0x0C..0x10].copy_from_slice(&(tik as u32).to_le_bytes());
    d[0x10..0x14].copy_from_slice(&(tmd_size as u32).to_le_bytes());
    let mut tmd = vec![0u8; align64(tmd_size)];
    tmd[0..4].copy_from_slice(&0x10004u32.to_be_bytes());
    tmd[0x140 + 0x4C..0x140 + 0x54].copy_from_slice(&title_id.to_be_bytes());
    tmd[0x140 + 0x9E..0x140 + 0xA0].copy_from_slice(&1u16.to_be_bytes());
    let chunk = 0x140 + 0xC4 + 0x900;
    tmd[chunk + 6..chunk + 8].copy_from_slice(&(if cia_encrypted { 1u16 } else { 0 }).to_be_bytes());
    d.extend(tmd);
    d.extend(ncch(title_id, nocrypto));
    d
}
