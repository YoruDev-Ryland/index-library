use super::{GroupKind::*, ParserKind::*, ProbeKind, SystemDef};

const CART_T: &str = "{title}{ ($regions)}{ ($langs)}{ (Rev $rev)}{ (v$version)}{ ($tags)}";
const DISC_T: &str = "{title}{ ($regions)}{ ($langs)}{ (Rev $rev)}{ ($tags)}{ (Disc $disc)}";
const NI: &[super::ParserKind] = &[NoIntro, GoodTools];

macro_rules! sys {
    ($id:expr, $name:expr, [$($f:expr),*], [$($e:expr),*], $parsers:expr, $probe:expr, $group:expr, $tpl:expr, $igdb:expr, $ss:expr, $sgdb:expr) => {
        SystemDef { id: $id, name: $name, folders: &[$($f),*], exts: &[$($e),*], parsers: $parsers, probe: $probe,
            group: $group, template: $tpl, igdb_platform: $igdb, ss_system: $ss, sgdb_platform: $sgdb }
    };
}

pub static SYSTEMS: &[SystemDef] = &[
    sys!("atari2600", "Atari 2600", ["Atari - 2600"], ["zip","a26","bin"], NI, None, Single, CART_T, Some(59), Some(26), None),
    sys!("atari5200", "Atari 5200", ["Atari - 5200"], ["zip","a52","bin"], NI, None, Single, CART_T, Some(66), Some(40), None),
    sys!("atari7800", "Atari 7800", ["Atari - 7800"], ["zip","a78","bin"], NI, None, Single, CART_T, Some(60), Some(41), None),
    sys!("lynx", "Atari Lynx", ["Atari - Lynx"], ["zip","lnx"], NI, None, Single, CART_T, Some(61), Some(28), None),
    sys!("neogeo", "Neo-Geo", ["Neo-Geo", "SNK - Neo-Geo"], ["zip","7z"], &[Mame], None, Single, "{title}", Some(80), Some(142), None),
    sys!("ngpc", "Neo-Geo Pocket Color", ["Neo-Geo Pocket Color"], ["zip","ngc","ngp"], NI, None, Single, CART_T, Some(120), Some(82), None),
    sys!("n3ds", "Nintendo 3DS", ["Nintendo - 3DS"], ["3ds","cia","cci","cxi"], NI, Some(ProbeKind::N3ds), Single, CART_T, Some(37), Some(17), None),
    sys!("nds", "Nintendo DS", ["Nintendo - DS"], ["zip","nds","7z"], NI, Some(ProbeKind::Nds), Single, CART_T, Some(20), Some(15), None),
    sys!("gb", "Game Boy", ["Nintendo - Gameboy", "Nintendo - Game Boy"], ["zip","gb","7z"], NI, None, Single, CART_T, Some(33), Some(9), None),
    sys!("gba", "Game Boy Advance", ["Nintendo - Gameboy Advance", "Nintendo - Game Boy Advance"], ["zip","gba","7z"], NI, Some(ProbeKind::Gba), Single, CART_T, Some(24), Some(12), None),
    sys!("gbc", "Game Boy Color", ["Nintendo - Gameboy Color", "Nintendo - Game Boy Color"], ["zip","gbc","7z"], NI, None, Single, CART_T, Some(22), Some(10), None),
    sys!("gamecube", "GameCube", ["Nintendo - Gamecube", "Nintendo - GameCube"], ["iso","gcm","ciso","rvz","gcz","wbfs","nkit.iso"], NI, None, Single, DISC_T, Some(21), Some(13), None),
    sys!("nes", "NES", ["Nintendo - NES"], ["zip","nes","7z","fds"], NI, None, Single, CART_T, Some(18), Some(3), None),
    sys!("snes", "SNES", ["Nintendo - SNES"], ["zip","smc","sfc","7z"], NI, None, Single, CART_T, Some(19), Some(4), None),
    sys!("switch", "Nintendo Switch", ["Nintendo - Switch"], ["nsp","xci","nsz","xcz","rar","zip","7z"], &[Switch, NoIntro], Some(ProbeKind::Switch), SwitchTitle, "{title}{ - $subtitle}{ [$title_id]}{[v$version]}", Some(130), Some(225), None),
    sys!("virtualboy", "Virtual Boy", ["Nintendo - Virtual Boy"], ["zip","vb"], NI, None, Single, CART_T, Some(87), Some(11), None),
    sys!("pc", "PC", ["PC"], ["zip","7z","rar","iso","exe"], &[NoIntro], None, Single, "{title}", Some(6), Some(135), None),
    sys!("pce", "PC Engine / TurboGrafx-16", ["PC Engine - TurboGrafx 16"], ["zip","pce"], NI, None, Single, CART_T, Some(86), Some(31), None),
    sys!("sega32x", "Sega 32X", ["Sega - 32X"], ["zip","32x"], NI, None, Single, CART_T, Some(30), Some(19), None),
    sys!("gamegear", "Game Gear", ["Sega - Game Gear"], ["zip","gg"], NI, None, Single, CART_T, Some(35), Some(21), None),
    sys!("genesis", "Genesis / Mega Drive", ["Sega - Genesis", "Sega - Mega Drive"], ["zip","md","gen","bin","smd"], NI, None, Single, CART_T, Some(29), Some(1), None),
    sys!("sms", "Master System", ["Sega - Mastersystem", "Sega - Master System"], ["zip","sms"], NI, None, Single, CART_T, Some(64), Some(2), None),
    sys!("sg1000", "SG-1000", ["Sega - SG1000", "Sega - SG-1000"], ["zip","sg"], NI, None, Single, CART_T, Some(84), Some(109), None),
    sys!("ps1", "PlayStation", ["Sony - PS1", "Sony - PlayStation"], ["cue","bin","chd","pbp","iso","img","m3u","7z","zip","rar"], &[NoIntro, GoodTools, Serial], None, CueBin, DISC_T, Some(7), Some(57), None),
    sys!("psp", "PlayStation Portable", ["Sony - PSP"], ["iso","cso","pbp","pkg","chd","zip","7z","rar"], &[NoIntro, GoodTools, Serial], Some(ProbeKind::PspPkg), Single, CART_T, Some(38), Some(61), None),
    sys!("psvita", "PlayStation Vita", ["Sony - PS Vita"], ["vpk","zip","pkg","7z"], &[NoIntro, Serial], Some(ProbeKind::PspPkg), PkgTitle, CART_T, Some(46), Some(62), None),
    sys!("vectrex", "Vectrex", ["Vectrex"], ["zip","vec","bin"], NI, None, Single, CART_T, Some(70), Some(102), None),
    sys!("gbahacks", "GBA ROM Hacks", ["Hacked Pokemon Roms"], ["zip","gba","7z"], NI, None, Single, CART_T, Some(24), Some(12), None),
];
