//! Icons are glyphs of the system "Segoe Fluent Icons" font, so they stay sharp at any size.
//! Windows 10 has only its predecessor, "Segoe MDL2 Assets", with the same code points for
//! nearly all of them ([`in_mdl2`]).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Shuffle,
    Repeat,
    Close,
    Timer,
    Stopwatch,
    Flag,
    Headphones,
    Cast,
    Speaker,
    Earbuds,
    Headset,
    Monitor,
    Volume,
    Mute,
    Check,
    Back,
    Settings,
    Sync,
    Pencil,
    Prompt,
    Search,
    People,
    Wrench,
    Document,
    Folder,
    Image,
    Video,
    Music,
    Pdf,
    Archive,
    TextDoc,
    Code,
    Download,
    /// An app's notification without an icon of its own.
    Bell,
    /// "Sen yokken": what happened while away.
    History,
    /// The shelf's footer: Paylaş, Metni kopyala, Küçült.
    Share,
    Copy,
    Shrink,
    /// The Buds card's rows: a page's chevron, the equalizer, touch, the system page, charging.
    ChevronRight,
    Equalizer,
    Touch,
    Info,
    Bolt,
}

impl Icon {
    pub const ALL: [Icon; 47] = [
        Icon::Play,
        Icon::Pause,
        Icon::Prev,
        Icon::Next,
        Icon::Shuffle,
        Icon::Repeat,
        Icon::Close,
        Icon::Timer,
        Icon::Stopwatch,
        Icon::Flag,
        Icon::Headphones,
        Icon::Cast,
        Icon::Speaker,
        Icon::Earbuds,
        Icon::Headset,
        Icon::Monitor,
        Icon::Volume,
        Icon::Mute,
        Icon::Check,
        Icon::Back,
        Icon::Settings,
        Icon::Sync,
        Icon::Pencil,
        Icon::Prompt,
        Icon::Search,
        Icon::People,
        Icon::Wrench,
        Icon::Document,
        Icon::Folder,
        Icon::Image,
        Icon::Video,
        Icon::Music,
        Icon::Pdf,
        Icon::Archive,
        Icon::TextDoc,
        Icon::Code,
        Icon::Download,
        Icon::Bell,
        Icon::History,
        Icon::Share,
        Icon::Copy,
        Icon::Shrink,
        Icon::ChevronRight,
        Icon::Equalizer,
        Icon::Touch,
        Icon::Info,
        Icon::Bolt,
    ];
}

/// Segoe Fluent Icons code point for an icon (checked visually with the `glyphsheet` tool).
/// Transport controls use the Solid variants, which match One UI's filled glyphs.
pub fn glyph(i: Icon) -> char {
    match i {
        Icon::Play => '\u{F5B0}',
        Icon::Pause => '\u{F8AE}',
        Icon::Prev => '\u{F8AC}',
        Icon::Next => '\u{F8AD}',
        Icon::Shuffle => '\u{E8B1}',
        Icon::Repeat => '\u{E8EE}',
        Icon::Close => '\u{E711}',
        Icon::Timer => '\u{E823}',
        Icon::Stopwatch => '\u{E916}',
        Icon::Flag => '\u{E7C1}',
        Icon::Headphones => '\u{E7F6}',
        Icon::Cast => '\u{EC15}',
        Icon::Speaker => '\u{E7F5}',
        Icon::Earbuds => '\u{F4C0}',
        Icon::Headset => '\u{E95B}',
        Icon::Monitor => '\u{E7F4}',
        Icon::Volume => '\u{E767}',
        Icon::Mute => '\u{E74F}',
        Icon::Check => '\u{E73E}',
        Icon::Back => '\u{E76B}',
        Icon::Settings => '\u{E713}',
        Icon::Sync => '\u{E895}',
        Icon::Pencil => '\u{E70F}',
        Icon::Prompt => '\u{E756}',
        Icon::Search => '\u{E721}',
        Icon::People => '\u{E716}',
        Icon::Wrench => '\u{E90F}',
        Icon::Document => '\u{E8A5}',
        Icon::Folder => '\u{E8B7}',
        Icon::Image => '\u{EB9F}',
        Icon::Video => '\u{E714}',
        Icon::Music => '\u{EC4F}',
        Icon::Pdf => '\u{EA90}',
        Icon::Archive => '\u{F012}',
        Icon::TextDoc => '\u{F000}',
        Icon::Code => '\u{E943}',
        Icon::Download => '\u{E896}',
        Icon::Bell => '\u{EA8F}',
        Icon::History => '\u{E81C}',
        Icon::Share => '\u{E72D}',
        Icon::Copy => '\u{E8C8}',
        // BackToWindow: two arrows coming in.
        Icon::Shrink => '\u{E73F}',
        Icon::ChevronRight => '\u{E76C}',
        Icon::Equalizer => '\u{E9E9}',
        Icon::Touch => '\u{E815}',
        Icon::Info => '\u{E946}',
        Icon::Bolt => '\u{E945}',
    }
}

/// The code point to draw `c` with in Segoe MDL2 Assets: its own, but where Windows 10's MDL2
/// lacks a newer Fluent glyph (Microsoft's MDL2 list has no U+F012) or draws it otherwise.
/// ui/kit.slint's Icon does the same for the folder.
pub fn in_mdl2(c: char) -> char {
    match c {
        // ZipFolder: MDL2's Package, a box.
        '\u{F012}' => '\u{E7B8}',
        // MDL2's Folder stands upright; its FolderHorizontal is Fluent's Folder.
        '\u{E8B7}' => '\u{F12B}',
        _ => c,
    }
}

/// The size to draw `c` at in Segoe MDL2 Assets, as a factor on Fluent's: its batteries (made
/// for a phone's status bar) are wider than Fluent's 1 em, and come down to it.
pub fn mdl2_scale(c: char) -> f32 {
    match c {
        '\u{EBA0}'..='\u{EBAA}' => 1.0 / 1.3125,
        '\u{EBAB}'..='\u{EBB5}' => 1.0 / 1.4375,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything the app draws checked in both fonts with `glyphsheet` (`PEBBLY_FONTS=win10`).
    /// MDL2's batteries (made for a phone's status bar) are 1.31 em wide, charging 1.44 em;
    /// Fluent's 1 em, which the pills are laid out for.
    #[test]
    fn mdl2_batteries_draw_as_wide_as_fluents() {
        assert_eq!(mdl2_scale('\u{EBA0}'), 1.0 / 1.3125, "Battery0");
        assert_eq!(mdl2_scale('\u{EBAA}'), 1.0 / 1.3125, "Battery10");
        assert_eq!(mdl2_scale('\u{EBAB}'), 1.0 / 1.4375, "BatteryCharging0");
        assert_eq!(mdl2_scale('\u{EBB5}'), 1.0 / 1.4375, "BatteryCharging10");
        assert_eq!(mdl2_scale(glyph(Icon::Volume)), 1.0);
        assert_eq!(mdl2_scale('\u{EBB6}'), 1.0);
    }

    #[test]
    fn mdl2_draws_its_own_where_it_lacks_fluents() {
        assert_eq!(in_mdl2(glyph(Icon::Archive)), '\u{E7B8}', "the zip folder: a box");
        assert_eq!(in_mdl2(glyph(Icon::Folder)), '\u{F12B}', "lying, like Fluent's");
        for i in Icon::ALL.iter().filter(|i| ![Icon::Archive, Icon::Folder].contains(i)) {
            assert_eq!(in_mdl2(glyph(*i)), glyph(*i), "{i:?}");
        }
        assert_eq!(in_mdl2('\u{EBA5}'), '\u{EBA5}', "a battery level");
        assert_eq!(in_mdl2('a'), 'a');
    }

    #[test]
    fn all_distinct() {
        let all = Icon::ALL;
        for (i, a) in all.iter().enumerate() {
            let g = glyph(*a) as u32;
            assert!((0xE000..=0xF8FF).contains(&g), "{a:?} outside the private use area");
            for b in &all[i + 1..] {
                assert_ne!(glyph(*a), glyph(*b), "{a:?} and {b:?} share a glyph");
            }
        }
    }
}
