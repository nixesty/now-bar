//! The Galaxy Buds2's other settings (equalizer, ambient sound, calls, edge taps, seamless
//! connection), read from the extended status and set by request; the Buds' health and firmware;
//! find my earbuds; what touch and hold tells the PC; how long the battery lasts. Pure:
//! `buds_worker.rs` sends and reads. Restated from GalaxyBudsClient 754b6fd (no code copied):
//! ExtendedStatusUpdateDecoder.cs L320-541, the encoders and view models named below.
//!
//! - 0x61 on Buds2 (Buds2's own offsets past [23]): [9] equalizer, [19] seamless connection
//!   (0 = on), [23] ambient level 0-2, [28] noise control with one earbud (revision 3),
//!   [29] / [30] / [31] customised ambient sound: on, left and right (nibbles, 0-2), tone 0-4
//!   (revision 5), [32] double tap on the edge for volume (5), [33] hear your own voice in calls (6).
//! - Requests: 0x86 [preset], 0x84 [level], 0x82 [on, left, right, tone], 0x6F / 0x95 / 0x8B
//!   [on], 0xAF [0 = on]. The Buds answer with 0x42 ACK [id, value…].
//! - 0x26 GET_ALL_DATA (empty request): i16 LE thermistors at 38 / 40 (×0.1 °C), cells at 44 / 50
//!   (×0.01 V); bytes 6-17 are the two Bluetooth addresses. 0x63 VERSION_INFO: [2..5] / [5..8] the
//!   left and right firmware (variant, year << 4 | month, release). 0x41 METERING_REPORT [2..4]
//!   the battery's capacity in mAh.
//! - Find my earbuds: 0xA0 start, 0xA1 stop (also the Buds' own "stopped"), 0xA2 [left, right]
//!   mute, 0xA3 their mute state. 0xA6 (rings while worn) is never sent.
//! - Touch and hold set to 5 (left) / 6 (right): a long press sends 0x93 [5 | 6].

use super::buds::{encode, model_id, Frame, Worn};

const VERSION_INFO: u8 = 0x63;
const DEBUG_GET_ALL_DATA: u8 = 0x26;
const METERING_REPORT: u8 = 0x41;
const SET_ANC_WITH_ONE_EARBUD: u8 = 0x6F;
const CUSTOMIZE_AMBIENT_SOUND: u8 = 0x82;
const AMBIENT_VOLUME: u8 = 0x84;
const EQUALIZER: u8 = 0x86;
const SET_SIDETONE: u8 = 0x8B;
const SET_TOUCHPAD_OTHER_OPTION: u8 = 0x93;
const OUTSIDE_DOUBLE_TAP: u8 = 0x95;
const FIND_MY_EARBUDS_START: u8 = 0xA0;
const FIND_MY_EARBUDS_STOP: u8 = 0xA1;
const MUTE_EARBUD: u8 = 0xA2;
const MUTE_EARBUD_STATUS_UPDATED: u8 = 0xA3;
const SET_SEAMLESS_CONNECTION: u8 = 0xAF;
const EXTENDED_STATUS_UPDATED: u8 = 0x61;
/// Buds2's top ambient level, and the customised sound's sides and tone (GBC's Buds2 spec and
/// sliders).
const AMBIENT_MAX: u8 = 2;
const SIDE_MAX: u8 = 2;
const TONE_MAX: u8 = 4;

/// Samsung's equalizer presets; Normal is the equalizer off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equalizer {
    Normal,
    BassBoost,
    Soft,
    Dynamic,
    Clear,
    TrebleBoost,
}

impl Equalizer {
    pub const ALL: [Equalizer; 6] = [Equalizer::Normal, Equalizer::BassBoost, Equalizer::Soft, Equalizer::Dynamic, Equalizer::Clear, Equalizer::TrebleBoost];

    pub fn byte(self) -> u8 {
        Equalizer::ALL.iter().position(|e| *e == self).unwrap_or(0) as u8
    }

    /// 7 is the custom equalizer later models have.
    pub fn from_byte(b: u8) -> Option<Equalizer> {
        Equalizer::ALL.get(b as usize).copied()
    }

    pub fn label(self) -> &'static str {
        use crate::i18n::tr;
        match self {
            Equalizer::Normal => tr("Normal"),
            Equalizer::BassBoost => tr("Bas güçlendirme"),
            Equalizer::Soft => tr("Yumuşak"),
            Equalizer::Dynamic => tr("Dinamik"),
            Equalizer::Clear => tr("Net"),
            Equalizer::TrebleBoost => tr("Tiz güçlendirme"),
        }
    }
}

/// The customised ambient sound: on, how much on each side (0-2), the tone (0 soft to 4 clear).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AmbientCustom {
    pub on: bool,
    pub left: u8,
    pub right: u8,
    pub tone: u8,
}

/// The Buds2's settings beyond battery, noise control and touch. Those a revision lacks are `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Buds2Settings {
    pub eq: Equalizer,
    pub ambient: u8,
    pub seamless: bool,
    pub anc_one: Option<bool>,
    pub custom: Option<AmbientCustom>,
    pub edge_tap: Option<bool>,
    pub sidetone: Option<bool>,
}

/// The settings that are a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    /// Noise control with one earbud in.
    AncOne,
    /// Double tap on the edge: volume down on the left, up on the right.
    EdgeTap,
    /// Hear your own voice in calls.
    Sidetone,
    /// The Buds move between Galaxy devices by themselves.
    Seamless,
}

impl Buds2Settings {
    pub fn get(&self, t: Toggle) -> Option<bool> {
        match t {
            Toggle::AncOne => self.anc_one,
            Toggle::EdgeTap => self.edge_tap,
            Toggle::Sidetone => self.sidetone,
            Toggle::Seamless => Some(self.seamless),
        }
    }

    /// With `t` switched, where this revision has it.
    pub fn with(mut self, t: Toggle, on: bool) -> Buds2Settings {
        let set = |v: &mut Option<bool>| *v = v.map(|_| on);
        match t {
            Toggle::AncOne => set(&mut self.anc_one),
            Toggle::EdgeTap => set(&mut self.edge_tap),
            Toggle::Sidetone => set(&mut self.sidetone),
            Toggle::Seamless => self.seamless = on,
        }
        self
    }
}

/// The settings an extended status carries, on a Buds2 (whose offsets these are).
pub fn parse_settings(f: &Frame) -> Option<Buds2Settings> {
    if f.id != EXTENDED_STATUS_UPDATED || !matches!(model_id(f)?, 313..=321) {
        return None;
    }
    let p = &f.payload;
    let rev = *p.first()?;
    let at = |i: usize, from: u8| p.get(i).copied().filter(|_| rev >= from);
    let custom = match (at(29, 5), at(30, 5), at(31, 5)) {
        (Some(on), Some(sides), Some(tone)) => Some(AmbientCustom { on: on == 1, left: sides >> 4, right: sides & 0x0F, tone }),
        _ => None,
    };
    Some(Buds2Settings {
        eq: Equalizer::from_byte(*p.get(9)?).unwrap_or(Equalizer::Normal),
        ambient: (*p.get(23)?).min(AMBIENT_MAX),
        seamless: *p.get(19)? == 0,
        anc_one: at(28, 3).map(|b| b == 1),
        custom,
        edge_tap: at(32, 5).map(|b| b == 1),
        sidetone: at(33, 6).map(|b| b == 1),
    })
}

pub fn eq_frame(e: Equalizer) -> Vec<u8> {
    encode(EQUALIZER, &[e.byte()])
}

pub fn ambient_frame(level: u8) -> Vec<u8> {
    encode(AMBIENT_VOLUME, &[level.min(AMBIENT_MAX)])
}

pub fn custom_frame(c: AmbientCustom) -> Vec<u8> {
    encode(CUSTOMIZE_AMBIENT_SOUND, &[c.on as u8, c.left.min(SIDE_MAX), c.right.min(SIDE_MAX), c.tone.min(TONE_MAX)])
}

/// A switch's request; seamless connection takes 0 for on.
pub fn toggle_frame(t: Toggle, on: bool) -> Vec<u8> {
    match t {
        Toggle::AncOne => encode(SET_ANC_WITH_ONE_EARBUD, &[on as u8]),
        Toggle::EdgeTap => encode(OUTSIDE_DOUBLE_TAP, &[on as u8]),
        Toggle::Sidetone => encode(SET_SIDETONE, &[on as u8]),
        Toggle::Seamless => encode(SET_SEAMLESS_CONNECTION, &[!on as u8]),
    }
}

/// Each bud's temperature (°C) and cell voltage (V).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Health {
    pub temp: [f32; 2],
    pub volt: [f32; 2],
}

pub fn health_frame() -> Vec<u8> {
    encode(DEBUG_GET_ALL_DATA, &[])
}

/// The Buds' debug data; both thermistors at 0 is a reading they didn't fill (GBC drops those).
pub fn parse_health(f: &Frame) -> Option<Health> {
    if f.id != DEBUG_GET_ALL_DATA {
        return None;
    }
    let i16_at = |i: usize| f.payload.get(i..i + 2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32);
    let temp = [i16_at(38)? / 10.0, i16_at(40)? / 10.0];
    let volt = [i16_at(44)? / 100.0, i16_at(50)? / 100.0];
    (temp.iter().any(|t| *t > 0.0)).then_some(Health { temp, volt })
}

/// A frame that must not reach the log: the debug data holds both Bluetooth addresses.
pub fn is_private(f: &Frame) -> bool {
    f.id == DEBUG_GET_ALL_DATA
}

/// "35,9 °C"
pub fn temp_text(t: f32) -> String {
    format!("{} °C", crate::i18n::decimal1(t as f64))
}

/// "4,30 V", for a reading a lithium cell can have.
pub fn volt_text(v: f32) -> Option<String> {
    (2.5..=4.6).contains(&v).then(|| format!("{:.2} V", v).replace('.', &crate::i18n::decimal_sep().to_string()))
}

/// The firmware as Samsung writes it ("R177XXU0AXL3"), from the left bud's version; Buds2's prefix
/// only (GBC's DebugModeVersionDecoder.cs: year letters from O for 2015, months from A, the
/// release 0-9 then A-Z).
pub fn parse_firmware(f: &Frame, model: Option<u16>) -> Option<String> {
    if f.id != VERSION_INFO || !matches!(model?, 313..=321) {
        return None;
    }
    let v = f.payload.get(2..5)?;
    let variant = if v[0] == 0 { 'E' } else { 'U' };
    let year = (b'O' + (v[1] >> 4)).min(b'Z') as char;
    let month = (b'A' + (v[1] & 0x0F)).min(b'Z') as char;
    let release = std::char::from_digit(v[2] as u32, 36)?.to_ascii_uppercase();
    Some(format!("R177XX{variant}0A{year}{month}{release}"))
}

/// The battery's capacity (mAh) from the Buds' usage report.
pub fn parse_capacity(f: &Frame) -> Option<u16> {
    if f.id != METERING_REPORT {
        return None;
    }
    let b = f.payload.get(2..4)?;
    Some(u16::from_le_bytes([b[0], b[1]])).filter(|c| *c > 0)
}

/// Find my earbuds stops by itself after this long.
pub const FIND_MAX_MS: u64 = 3 * 60_000;

/// Start (0xA0: only out of the ear) or stop.
pub fn find_frame(on: bool) -> Vec<u8> {
    encode(if on { FIND_MY_EARBUDS_START } else { FIND_MY_EARBUDS_STOP }, &[])
}

pub fn mute_frame(muted: [bool; 2]) -> Vec<u8> {
    encode(MUTE_EARBUD, &[muted[0] as u8, muted[1] as u8])
}

/// Which buds the Buds say are muted while ringing.
pub fn parse_muted(f: &Frame) -> Option<[bool; 2]> {
    if f.id != MUTE_EARBUD_STATUS_UPDATED {
        return None;
    }
    Some([*f.payload.first()? != 0, *f.payload.get(1)? != 0])
}

/// The Buds stopped ringing by themselves.
pub fn find_stopped(f: &Frame) -> bool {
    f.id == FIND_MY_EARBUDS_STOP
}

/// Ringing is for buds out of the ear: it is loud.
pub fn find_can_start(worn: Worn) -> bool {
    worn.count() == 0
}

/// The card's find my earbuds: since when it rings, and which side is muted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindState {
    pub since_ms: Option<u64>,
    pub muted: [bool; 2],
}

impl FindState {
    pub fn running(&self, now_ms: u64) -> bool {
        self.since_ms.is_some_and(|s| now_ms.saturating_sub(s) <= FIND_MAX_MS)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A long press on a bud whose touch and hold is the PC's.
pub fn parse_hold_pressed(f: &Frame) -> Option<Side> {
    match (f.id, f.payload.first()) {
        (SET_TOUCHPAD_OTHER_OPTION, Some(5)) => Some(Side::Left),
        (SET_TOUCHPAD_OTHER_OPTION, Some(6)) => Some(Side::Right),
        _ => None,
    }
}

/// How fast the worn buds' battery goes, from the last hour's readings: how long it lasts.
#[derive(Clone, Debug, Default)]
pub struct Drain {
    samples: Vec<(u64, u8)>,
}

const DRAIN_WINDOW_MS: u64 = 60 * 60_000;
const DRAIN_MIN_SPAN_MS: u64 = 15 * 60_000;
const DRAIN_MIN_DROP: u8 = 3;

impl Drain {
    /// The battery of the bud that runs out first while worn and not charging; `None` (charging,
    /// out of the ear) starts over.
    pub fn note(&mut self, now_ms: u64, level: Option<u8>) {
        let Some(level) = level else {
            self.samples.clear();
            return;
        };
        if self.samples.last().is_some_and(|(_, l)| *l == level) {
            return;
        }
        self.samples.push((now_ms, level));
        self.samples.retain(|(t, _)| now_ms.saturating_sub(*t) <= DRAIN_WINDOW_MS);
    }

    /// Minutes left, once the last hour shows a steady fall.
    pub fn remaining_min(&self) -> Option<u32> {
        let (&(t0, l0), &(t1, l1)) = (self.samples.first()?, self.samples.last()?);
        let (span, drop) = (t1.saturating_sub(t0), l0.checked_sub(l1)?);
        if span < DRAIN_MIN_SPAN_MS || drop < DRAIN_MIN_DROP {
            return None;
        }
        Some((l1 as u64 * span / drop as u64 / 60_000).min(24 * 60) as u32)
    }
}

/// What the drain follows: the worn bud that runs out first (0 is no reading); nothing while one
/// charges or none is in an ear.
pub fn drain_level(b: &crate::activities::buds::BudsSnapshot) -> Option<u8> {
    let charging = b.charging.unwrap_or_default();
    if charging.left || charging.right {
        return None;
    }
    [(b.worn.left, b.left), (b.worn.right, b.right)].into_iter().filter(|(worn, level)| *worn && *level > 0).map(|(_, level)| level).min()
}

/// Samsung's rated listening on the buds alone (minutes): noise cancelling on, off.
fn rated(model: Option<u16>) -> (u32, u32) {
    match model {
        // Buds Pro, Buds2 Pro
        Some(298..=301) | Some(325..=328) => (300, 480),
        // Buds FE
        Some(330..=331) => (360, 510),
        // Buds3
        Some(333..=334) => (300, 360),
        // Buds3 Pro
        Some(340..=341) => (360, 420),
        // Buds2, and what Samsung's figures aren't known for
        _ => (300, 450),
    }
}

/// Minutes left by Samsung's figures until the drain is measured: the worn bud that runs out
/// first, its share of the rated time in the mode it's in (ambient costs about what noise
/// cancelling does).
pub fn rated_remaining_min(b: &crate::activities::buds::BudsSnapshot) -> Option<u32> {
    let level = drain_level(b)? as u32;
    let (anc, off) = rated(b.model);
    let full = if b.noise == Some(crate::activities::buds::NoiseControl::Off) { off } else { anc };
    Some(full * level / 100)
}

/// "Yaklaşık 3 sa 20 dk", in tens of minutes.
pub fn remaining_text(minutes: u32) -> String {
    let rounded = (minutes / 10 * 10).max(10) as u64;
    crate::i18n::trf("Yaklaşık {}", &[&crate::activities::timer::format_duration_label(rounded * 60_000)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::buds::{take_frames, Frame, Worn};

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// GalaxyBudsClient's Buds2 capture (revision 10): Dynamic, ambient 1, sidetone on, edge taps on.
    const CAPTURE: &str = "FD 28 10 61 0A 03 64 64 01 01 11 33 00 03 3F 22 01 00 3C 01 3C 01 02 00 \
        03 66 00 01 00 10 00 00 00 00 11 02 01 01 00 00 00 BE A3 DD";

    fn capture() -> Frame {
        take_frames(&mut hex(CAPTURE)).remove(0)
    }

    fn with(i: usize, v: u8) -> Frame {
        let mut f = capture();
        f.payload[i] = v;
        f
    }

    fn sent(frame: Vec<u8>) -> Frame {
        take_frames(&mut frame.clone()).remove(0)
    }

    #[test]
    fn the_settings_come_from_the_extended_status() {
        let s = parse_settings(&capture()).unwrap();
        assert_eq!(s.eq, Equalizer::Dynamic);
        assert_eq!(s.ambient, 1);
        assert!(s.seamless, "[19] 0 is on");
        assert_eq!(s.anc_one, Some(false));
        assert_eq!(s.custom, Some(AmbientCustom { on: false, left: 1, right: 1, tone: 2 }));
        assert_eq!(s.edge_tap, Some(true));
        assert_eq!(s.sidetone, Some(true));
        assert!(!parse_settings(&with(19, 1)).unwrap().seamless);
        assert_eq!(parse_settings(&with(30, 0x20)).unwrap().custom.unwrap(), AmbientCustom { on: false, left: 2, right: 0, tone: 2 });
    }

    #[test]
    fn older_revisions_lack_the_later_settings() {
        let at = |rev: u8| parse_settings(&with(0, rev)).unwrap();
        assert_eq!(at(2).anc_one, None);
        assert_eq!(at(3).anc_one, Some(false));
        assert_eq!(at(4).custom, None);
        assert_eq!(at(4).edge_tap, None);
        assert_eq!(at(5).edge_tap, Some(true));
        assert_eq!(at(5).sidetone, None);
        assert_eq!(at(6).sidetone, Some(true));
    }

    #[test]
    fn only_the_buds2_layout_is_read() {
        let mut f = capture();
        f.payload[14..16].copy_from_slice(&326u16.to_le_bytes());
        assert_eq!(parse_settings(&f), None, "a Buds2 Pro keeps them elsewhere");
        assert_eq!(parse_settings(&Frame { id: 0x60, payload: capture().payload }), None);
        let mut short = capture();
        short.payload.truncate(30);
        assert!(parse_settings(&short).unwrap().edge_tap.is_none(), "a short status: what it has");
    }

    #[test]
    fn the_requests() {
        assert_eq!(sent(eq_frame(Equalizer::BassBoost)), Frame { id: 0x86, payload: vec![1] });
        assert_eq!(sent(eq_frame(Equalizer::Normal)).payload, vec![0]);
        assert_eq!(sent(ambient_frame(1)), Frame { id: 0x84, payload: vec![1] });
        assert_eq!(sent(ambient_frame(9)).payload, vec![2], "Buds2's top level");
        assert_eq!(sent(custom_frame(AmbientCustom { on: true, left: 0, right: 7, tone: 9 })), Frame { id: 0x82, payload: vec![1, 0, 2, 4] }, "clamped");
        assert_eq!(sent(toggle_frame(Toggle::AncOne, true)), Frame { id: 0x6F, payload: vec![1] });
        assert_eq!(sent(toggle_frame(Toggle::EdgeTap, false)), Frame { id: 0x95, payload: vec![0] });
        assert_eq!(sent(toggle_frame(Toggle::Sidetone, true)), Frame { id: 0x8B, payload: vec![1] });
        assert_eq!(sent(toggle_frame(Toggle::Seamless, true)), Frame { id: 0xAF, payload: vec![0] }, "0 turns it on");
        assert_eq!(sent(toggle_frame(Toggle::Seamless, false)).payload, vec![1]);
    }

    #[test]
    fn a_switch_flips_in_the_settings() {
        let s = parse_settings(&capture()).unwrap();
        assert_eq!(s.get(Toggle::Sidetone), Some(true));
        assert_eq!(s.with(Toggle::Sidetone, false).get(Toggle::Sidetone), Some(false));
        assert!(!s.with(Toggle::Seamless, false).seamless);
        let old = parse_settings(&with(0, 2)).unwrap();
        assert_eq!(old.with(Toggle::AncOne, true).get(Toggle::AncOne), None, "not on this revision");
    }

    #[test]
    fn equalizer_presets() {
        assert_eq!(Equalizer::from_byte(5), Some(Equalizer::TrebleBoost));
        assert_eq!(Equalizer::from_byte(7), None, "custom: not on Buds2");
        assert_eq!(Equalizer::ALL.len(), 6);
        assert_eq!(Equalizer::BassBoost.label(), "Bas güçlendirme");
    }

    /// A made-up GET_ALL_DATA answer: thermistors 35.9 / 34.1 °C, cells 4.30 / 4.01 V.
    fn health_payload(temp: [i16; 2], volt: [i16; 2]) -> Vec<u8> {
        let mut p = vec![0u8; 84];
        p[38..40].copy_from_slice(&temp[0].to_le_bytes());
        p[40..42].copy_from_slice(&temp[1].to_le_bytes());
        p[44..46].copy_from_slice(&volt[0].to_le_bytes());
        p[50..52].copy_from_slice(&volt[1].to_le_bytes());
        p
    }

    #[test]
    fn health_from_the_debug_data() {
        let h = parse_health(&Frame { id: 0x26, payload: health_payload([359, 341], [430, 401]) }).unwrap();
        assert!((h.temp[0] - 35.9).abs() < 1e-4 && (h.temp[1] - 34.1).abs() < 1e-4);
        assert!((h.volt[0] - 4.30).abs() < 1e-4 && (h.volt[1] - 4.01).abs() < 1e-4);
        assert_eq!(parse_health(&Frame { id: 0x26, payload: health_payload([0, 0], [430, 401]) }), None, "all zero: a reading the Buds didn't fill");
        assert_eq!(parse_health(&Frame { id: 0x26, payload: vec![0; 40] }), None);
        assert_eq!(sent(health_frame()), Frame { id: 0x26, payload: vec![] });
        assert!(is_private(&Frame { id: 0x26, payload: vec![] }), "it carries both Bluetooth addresses");
    }

    #[test]
    fn health_texts() {
        assert_eq!(temp_text(35.94), "35,9 °C");
        assert_eq!(volt_text(4.3), Some("4,30 V".to_string()));
        assert_eq!(volt_text(0.0), None, "not a cell's voltage");
        assert_eq!(volt_text(6.1), None);
        crate::i18n::in_language(crate::i18n::Language::English, || assert_eq!(volt_text(4.3), Some("4.30 V".to_string())));
    }

    #[test]
    fn the_firmware_from_the_version_info() {
        // Made up: variant U, December 2024 (year nibble 9 = X, month 11 = L), release 3.
        let f = Frame { id: 0x63, payload: hex("00 00 01 9B 03 01 9B 03 00 00") };
        assert_eq!(parse_firmware(&f, Some(316)), Some("R177XXU0AXL3".to_string()));
        let e = Frame { id: 0x63, payload: hex("00 00 00 6B 0B 00 6B 0B 00 00") };
        assert_eq!(parse_firmware(&e, Some(313)), Some("R177XXE0AULB".to_string()), "variant E, release 11 = B");
        assert_eq!(parse_firmware(&f, Some(326)), None, "another model's prefix isn't known");
        assert_eq!(parse_firmware(&f, None), None);
        assert_eq!(parse_firmware(&Frame { id: 0x63, payload: vec![0; 4] }, Some(316)), None);
    }

    #[test]
    fn the_capacity_from_the_metering_report() {
        let f = Frame { id: 0x41, payload: hex("02 11 3A 00 50 00 00 00") };
        assert_eq!(parse_capacity(&f), Some(58));
        assert_eq!(parse_capacity(&Frame { id: 0x41, payload: hex("02 11 00 00") }), None);
        assert_eq!(parse_capacity(&Frame { id: 0x40, payload: hex("02 11 3A 00") }), None);
    }

    #[test]
    fn find_my_earbuds() {
        assert_eq!(sent(find_frame(true)), Frame { id: 0xA0, payload: vec![] }, "never 0xA6: that one rings in the ear");
        assert_eq!(sent(find_frame(false)), Frame { id: 0xA1, payload: vec![] });
        assert_eq!(sent(mute_frame([true, false])), Frame { id: 0xA2, payload: vec![1, 0] });
        assert_eq!(parse_muted(&Frame { id: 0xA3, payload: vec![0, 1] }), Some([false, true]));
        assert!(find_stopped(&Frame { id: 0xA1, payload: vec![] }));
        assert!(!find_can_start(Worn { left: true, right: false }), "not with a bud in the ear");
        assert!(find_can_start(Worn::default()));
    }

    #[test]
    fn finding_stops_by_itself() {
        let f = FindState { since_ms: Some(1_000), muted: [false, false] };
        assert!(f.running(1_000 + FIND_MAX_MS));
        assert!(!f.running(1_000 + FIND_MAX_MS + 1));
        assert!(!FindState::default().running(0));
    }

    #[test]
    fn a_hold_on_the_pc() {
        assert_eq!(parse_hold_pressed(&Frame { id: 0x93, payload: vec![5] }), Some(Side::Left));
        assert_eq!(parse_hold_pressed(&Frame { id: 0x93, payload: vec![6] }), Some(Side::Right));
        assert_eq!(parse_hold_pressed(&Frame { id: 0x93, payload: vec![2] }), None);
    }

    #[test]
    fn the_battery_lasts_as_long_as_it_drains() {
        let min = 60_000;
        let mut d = Drain::default();
        d.note(0, Some(90));
        d.note(10 * min, Some(88));
        assert_eq!(d.remaining_min(), None, "too soon, too little");
        d.note(20 * min, Some(86));
        // 4 % in 20 min: 86 % lasts 430 min.
        assert_eq!(d.remaining_min(), Some(430));
        d.note(21 * min, None);
        assert_eq!(d.remaining_min(), None, "charging or out of the ear starts over");
        d.note(22 * min, Some(86));
        d.note(40 * min, Some(87));
        assert_eq!(d.remaining_min(), None, "rising: charged in between");
    }

    #[test]
    fn the_drain_looks_at_the_last_hour() {
        let min = 60_000;
        let mut d = Drain::default();
        d.note(0, Some(100));
        d.note(5 * min, Some(70));
        d.note(120 * min, Some(68));
        d.note(150 * min, Some(64));
        // Only the last hour counts: 68 → 64 in 30 min.
        assert_eq!(d.remaining_min(), Some(480));
    }

    #[test]
    fn the_drain_follows_the_worn_bud_that_runs_out_first() {
        use crate::activities::buds::{BudsSnapshot, Charging};
        let both = BudsSnapshot { left: 70, right: 64, worn: Worn { left: true, right: true }, ..Default::default() };
        assert_eq!(drain_level(&both), Some(64));
        assert_eq!(drain_level(&BudsSnapshot { worn: Worn { left: true, right: false }, ..both }), Some(70), "only the one in the ear");
        assert_eq!(drain_level(&BudsSnapshot { worn: Worn::default(), ..both }), None);
        assert_eq!(drain_level(&BudsSnapshot { right: 0, ..both }), Some(70), "0 is no reading");
        assert_eq!(drain_level(&BudsSnapshot { charging: Some(Charging { right: true, ..Default::default() }), ..both }), None);
    }

    #[test]
    fn a_rated_estimate_until_the_drain_is_known() {
        use crate::activities::buds::{BudsSnapshot, NoiseControl};
        let worn = BudsSnapshot { left: 80, right: 60, worn: Worn { left: true, right: true }, model: Some(316), noise: Some(NoiseControl::Anc), ..Default::default() };
        // Buds2: 5 h with noise cancelling, 7.5 h without; the bud at 60 % runs out first.
        assert_eq!(rated_remaining_min(&worn), Some(180));
        assert_eq!(rated_remaining_min(&BudsSnapshot { noise: Some(NoiseControl::Off), ..worn }), Some(270));
        assert_eq!(rated_remaining_min(&BudsSnapshot { noise: Some(NoiseControl::Ambient), ..worn }), Some(180), "ambient sound costs about what noise cancelling does");
        assert_eq!(rated_remaining_min(&BudsSnapshot { model: Some(331), ..worn }), Some(216), "Buds FE: 6 h");
        assert_eq!(rated_remaining_min(&BudsSnapshot { model: None, ..worn }), Some(180), "unknown: Buds2's");
        assert_eq!(rated_remaining_min(&BudsSnapshot { worn: Worn::default(), ..worn }), None, "not in the ears");
    }

    #[test]
    fn the_remaining_time_in_words() {
        assert_eq!(remaining_text(430), "Yaklaşık 7 sa 10 dk");
        assert_eq!(remaining_text(44), "Yaklaşık 40 dk");
        assert_eq!(remaining_text(181), "Yaklaşık 3 sa");
    }
}
