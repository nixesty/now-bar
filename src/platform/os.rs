//! Which Windows this is. Windows 11 still says "10.0": only its build (22000 and up) tells it
//! apart, and with it the taskbar, the icon font and the card's blur.

use std::sync::OnceLock;

use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generation {
    Windows10,
    Windows11,
}

/// The first Windows 11 build.
const WINDOWS_11: u32 = 22000;
/// Windows 10 1809: the media sessions (GlobalSystemMediaTransportControls) and composition in
/// desktop windows, which Pebbly is built on, start here.
pub const MIN_BUILD: u32 = 17763;
/// 20H1's builds took the dark title bar attribute from 19 to 20.
const DARK_MODE_20: u32 = 18985;

pub fn generation_of(build: u32) -> Generation {
    if build >= WINDOWS_11 {
        Generation::Windows11
    } else {
        Generation::Windows10
    }
}

pub fn supported(build: u32) -> bool {
    build >= MIN_BUILD
}

/// `DWMWA_USE_IMMERSIVE_DARK_MODE` on this build.
pub fn dark_mode_attribute(build: u32) -> u32 {
    if build >= DARK_MODE_20 {
        20
    } else {
        19
    }
}

/// The card's material is the blurred desktop when the `setting` and Windows' `transparency`
/// effects allow it and Windows can give it the host backdrop; else flat.
pub fn card_blur(generation: Generation, setting: bool, transparency: bool) -> bool {
    generation == Generation::Windows11 && setting && transparency
}

fn parse_build(s: &str) -> Option<u32> {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\0').parse().ok()
}

/// This PC's build, from the registry (no compatibility shim changes it; GetVersionEx answers
/// by the exe's manifest, and a test runner has none). None when it can't be read.
pub fn build() -> Option<u32> {
    static BUILD: OnceLock<Option<u32>> = OnceLock::new();
    *BUILD.get_or_init(|| {
        let mut buf = [0u16; 16];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
                w!("CurrentBuildNumber"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        let len = (size as usize / 2).min(buf.len());
        status.is_ok().then(|| String::from_utf16_lossy(&buf[..len])).as_deref().and_then(parse_build)
    })
}

/// Unknown counts as Windows 11, where everything was made.
pub fn generation() -> Generation {
    build().map_or(Generation::Windows11, generation_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_11_starts_at_build_22000() {
        assert_eq!(generation_of(19045), Generation::Windows10, "Windows 10 22H2");
        assert_eq!(generation_of(21999), Generation::Windows10);
        assert_eq!(generation_of(22000), Generation::Windows11, "Windows 11 21H2");
        assert_eq!(generation_of(26300), Generation::Windows11);
    }

    #[test]
    fn oldest_supported_build_is_1809() {
        assert!(!supported(17134), "1803 has no media sessions");
        assert!(supported(17763));
        assert!(supported(26100));
    }

    /// The dark title bar attribute moved from 19 to 20 in 20H1's builds.
    #[test]
    fn dark_mode_attribute_by_build() {
        assert_eq!(dark_mode_attribute(17763), 19);
        assert_eq!(dark_mode_attribute(18363), 19);
        assert_eq!(dark_mode_attribute(19041), 20);
        assert_eq!(dark_mode_attribute(22631), 20);
    }

    /// The card's blur needs DWMWA_USE_HOSTBACKDROPBRUSH: a desktop window gets the host
    /// backdrop only from Windows 11 on.
    #[test]
    fn the_card_blurs_on_windows_11_only() {
        assert!(card_blur(Generation::Windows11, true, true));
        assert!(!card_blur(Generation::Windows10, true, true));
        assert!(!card_blur(Generation::Windows11, false, true), "the setting off");
        assert!(!card_blur(Generation::Windows11, true, false), "Windows' transparency effects off");
    }

    #[test]
    fn build_number_from_the_registry_string() {
        assert_eq!(parse_build("19045"), Some(19045));
        assert_eq!(parse_build(" 26300\0"), Some(26300));
        assert_eq!(parse_build(""), None);
        assert_eq!(parse_build("abc"), None);
    }

    /// The machine the tests run on answers something real.
    #[test]
    fn this_pc_has_a_build() {
        assert!(build().is_some_and(supported));
    }
}
