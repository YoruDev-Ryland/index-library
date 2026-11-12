//! Which console a match is for. Sources name consoles differently ("Sega Mega Drive/Genesis",
//! "Genesis"); these are the names that count as each system.
use crate::systems::SystemDef;

pub fn names(system_id: &str) -> &'static [&'static str] {
    match system_id {
        "n3ds" => &["Nintendo 3DS", "New Nintendo 3DS"],
        "nds" => &["Nintendo DS", "Nintendo DSi"],
        "gba" | "gbahacks" => &["Game Boy Advance"],
        "gb" => &["Game Boy"],
        "gbc" => &["Game Boy Color"],
        "gamecube" => &["GameCube", "Nintendo GameCube"],
        "nes" => &["NES", "Nintendo Entertainment System", "Family Computer", "Family Computer Disk System"],
        "snes" => &["SNES", "Super Nintendo Entertainment System", "Super Famicom", "Satellaview"],
        "switch" => &["Nintendo Switch", "Nintendo Switch 2"],
        "virtualboy" => &["Virtual Boy"],
        "pc" => &["PC", "PC (Microsoft Windows)", "DOS", "Linux"],
        "pce" => &["TurboGrafx-16/PC Engine", "PC Engine", "TurboGrafx-16", "TurboGrafx-16/PC Engine CD"],
        "sega32x" => &["Sega 32X", "32X"],
        "gamegear" => &["Game Gear", "Sega Game Gear"],
        "genesis" => &["Genesis", "Sega Mega Drive/Genesis", "Sega Genesis", "Mega Drive"],
        "sms" => &["Sega Master System", "Master System", "Sega Master System/Mark III", "Mark III"],
        "sg1000" => &["SG-1000"],
        "ps1" => &["PlayStation"],
        "psp" => &["PSP", "PlayStation Portable"],
        "psvita" => &["PS Vita", "PlayStation Vita"],
        "atari2600" => &["Atari 2600"],
        "atari5200" => &["Atari 5200"],
        "atari7800" => &["Atari 7800"],
        "lynx" => &["Atari Lynx", "Lynx"],
        "neogeo" => &["Neo Geo", "Neo Geo AES", "Neo Geo MVS", "Neo-Geo", "Neo Geo CD"],
        "ngpc" => &["Neo Geo Pocket Color"],
        "vectrex" => &["Vectrex"],
        _ => &[],
    }
}

fn norm(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase() }

/// Other consoles whose games this one plays, and which its folder usually holds: PlayStation
/// classics on a PSP or Vita, PSP games on a Vita, original Game Boy games on a Game Boy Color.
fn plays(system_id: &str) -> &'static [&'static str] {
    match system_id {
        "psp" => &["PlayStation"],
        "psvita" => &["PlayStation Portable", "PSP", "PlayStation"],
        "gbc" => &["Game Boy"],
        _ => &[],
    }
}

/// Is a console name one of this system's (or one whose games it plays)?
pub fn is_system(system: &SystemDef, platform: &str) -> bool {
    let p = norm(platform);
    norm(system.name) == p || names(system.id).iter().chain(plays(system.id)).any(|n| norm(n) == p)
}

/// "match" when one of the platforms is this system's, "other" when none is, None when unknown.
pub fn verdict(system: &SystemDef, platforms: &[String]) -> Option<String> {
    if platforms.is_empty() { return None; }
    Some(if platforms.iter().any(|p| is_system(system, p)) { "match" } else { "other" }.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_names() {
        let n3ds = crate::systems::by_id("n3ds").unwrap();
        assert_eq!(verdict(n3ds, &["Nintendo 3DS".into()]).as_deref(), Some("match"));
        assert_eq!(verdict(n3ds, &["Game Boy Advance".into(), "Wii U".into()]).as_deref(), Some("other"));
        assert_eq!(verdict(n3ds, &[]), None);
        let gen = crate::systems::by_id("genesis").unwrap();
        assert!(is_system(gen, "Sega Mega Drive/Genesis"));
        assert!(is_system(crate::systems::by_id("sms").unwrap(), "Sega Master System/Mark III"));
        let psp = crate::systems::by_id("psp").unwrap();
        assert!(is_system(psp, "PlayStation") && !is_system(psp, "PlayStation 2"));
    }
}
