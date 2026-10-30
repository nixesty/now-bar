//! Galaxy Buds2 battery, read from the Buds' own RFCOMM channel. This file is the pure part
//! (frame parsing, labels); `buds_worker.rs` owns the Bluetooth connection.
//!
//! Protocol (Buds2; restated from GalaxyBudsClient's sources and real captures, no code copied —
//! see docs/superpowers/designs/buds2-protocol.md for every source):
//! - RFCOMM service `2e73a4ad-332d-41fc-90e2-16bef06523f2`. No handshake: the Buds send an
//!   EXTENDED_STATUS_UPDATED right after the connection opens, then updates as things change.
//! - Frame: `FD | header u16 LE | id | payload | crc u16 LE | DD`. The low 10 bits of the header
//!   are the length of id + payload + crc; the other bits are flags we ignore.
//! - CRC-16/XMODEM (poly 0x1021, init 0, no reflection) over id + payload.
//! - 0x60 STATUS_UPDATED: [1] left, [2] right, [5] placement, [6] case.
//!   0x61 EXTENDED_STATUS_UPDATED: [2] left, [3] right, [6] placement, [7] case.
//! - Placement: high nibble left, low nibble right; 1 = in the ear, 3 = in the open case,
//!   4 = in the closed case.
//! - Noise control: 0x61 [12], and 0x77 NOISE_CONTROLS_UPDATE [0] when it changes (touch, phone);
//!   0x78 NOISE_CONTROLS [mode] sets it. 0 off, 1 noise cancelling, 2 ambient sound. A request's
//!   header carries no flags (responses from the Buds set 0x1000).
//!   (GalaxyBudsClient: SppMessageEnums.cs, Constants.cs NoiseControlModes,
//!   ExtendedStatusUpdateDecoder.cs L361, NoiseControlUpdateDecoder.cs, SppMessage.cs L86-95.)
//! - Touch controls (Buds2 and later): 0x61 [10], bit 7 on, bits 0-5 the gestures still on
//!   (hold, triple, double, single, double in a call, hold in a call); [11] touch and hold, high
//!   nibble left, low right (1 voice command, 2 noise control, 3 volume, 4 Spotify, 5/6 an app's).
//!   0x90 LOCK_TOUCHPAD [on, single, double, triple, hold (, double in a call, hold in a call)],
//!   0x92 SET_TOUCHPAD_OPTION [left, right]. (ExtendedStatusUpdateDecoder.cs L332-353,
//!   LockTouchpadEncoder.cs, SetTouchOptionsEncoder.cs, StandardTouchMap.cs, the device specs'
//!   AdvancedTouchLock rules.)
//! - Fit test: 0x9D [1] starts it (a tone plays), [0] stops it; 0x9E [left, right] the result,
//!   0 bad, 1 good, 2 failed (FitTestPageViewModel.cs, FitTestDecoder.cs).
//! - A bud battery of 0 means "no reading"; a case value outside 1..=100 is unknown.

use windows::core::GUID;

/// Samsung's serial service on Buds2 (and later models).
pub const SPP_UUID: GUID = GUID::from_u128(0x2e73a4ad_332d_41fc_90e2_16bef06523f2);
/// The standard serial port service Buds Pro (and many other devices) offer.
pub const SPP_STANDARD_UUID: GUID = GUID::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);
/// Bytes to send after connecting: none, the Buds start talking on their own.
pub const HELLO: &[u8] = &[];

const SOM: u8 = 0xFD;
const EOM: u8 = 0xDD;
const LENGTH_MASK: u16 = 0x3FF;
const STATUS_UPDATED: u8 = 0x60;
const EXTENDED_STATUS_UPDATED: u8 = 0x61;
const NOISE_CONTROLS_UPDATE: u8 = 0x77;
const NOISE_CONTROLS: u8 = 0x78;
const ACK: u8 = 0x42;
const LOCK_TOUCHPAD: u8 = 0x90;
const TOUCH_UPDATED: u8 = 0x91;
const SET_TOUCHPAD_OPTION: u8 = 0x92;
const CHECK_FIT: u8 = 0x9D;
const FIT_RESULT: u8 = 0x9E;
/// Where 0x61 keeps the noise control mode (Buds Pro and later, Buds2 included).
const NOISE_AT: usize = 12;
/// Where 0x61 keeps the touch controls and touch and hold (Buds Live and later).
const TOUCH_AT: usize = 10;
const HOLD_AT: usize = 11;
const TOUCH_ON: u8 = 0x80;
const GESTURES: u8 = 0x3F;
const WEARING: u8 = 1;
const IN_OPEN_CASE: u8 = 3;
const IN_CLOSED_CASE: u8 = 4;

/// One message from the Buds.
#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    pub id: u8,
    pub payload: Vec<u8>,
}

/// One battery reading. `case` is known only while a bud sits in the case; `noise` once the
/// Buds have said it (an extended status or a noise control update); `touch` from an extended
/// status, on the models it's set on here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BudsSnapshot {
    pub left: u8,
    pub right: u8,
    pub case: Option<u8>,
    pub noise: Option<NoiseControl>,
    pub worn: Worn,
    pub touch: Option<Touch>,
    /// Where each bud is, left and right.
    pub place: [Place; 2],
    /// From an extended status at revision 10 and later; plain statuses follow it then.
    pub charging: Option<Charging>,
    /// The model id (with its colour), from an extended status.
    pub model: Option<u16>,
    /// A Buds2's other settings.
    pub settings: Option<crate::activities::buds_controls::Buds2Settings>,
}

/// Where a bud is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// Not linked to its pair.
    Off,
    Worn,
    /// Out of the ear and the case.
    #[default]
    Out,
    /// In the case, open or closed.
    InCase,
}

impl Place {
    fn from_nibble(n: u8) -> Place {
        match n {
            0 => Place::Off,
            WEARING => Place::Worn,
            IN_OPEN_CASE | IN_CLOSED_CASE => Place::InCase,
            _ => Place::Out,
        }
    }
}

/// What charges: 0x61 [36] / 0x60 [7], 0x10 left, 0x04 right, 0x01 the case (GBC knows no other
/// bit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Charging {
    pub left: bool,
    pub right: bool,
    pub case: bool,
}

impl Charging {
    fn from_byte(b: u8) -> Charging {
        Charging { left: b & 0x10 != 0, right: b & 0x04 != 0, case: b & 0x01 != 0 }
    }
}

/// Galaxy Wearable's touch controls: whether touching the buds does anything, which gestures
/// do, and what touching and holding each bud does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touch {
    pub on: bool,
    /// 0x61 [10]'s gesture bits, sent back as they are when touches are switched.
    pub gestures: u8,
    /// Touch and hold, left and right, as the Buds say it.
    pub hold: [u8; 2],
    /// The lock request carries the two in-a-call gestures.
    pub calls: bool,
}

impl Touch {
    /// The touch and hold the buds have together, when the card offers it.
    pub fn hold_both(&self) -> Option<HoldAction> {
        HoldAction::from_bytes(self.hold)
    }
}

/// Touch and hold as the card offers it (Spotify needs the phone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldAction {
    NoiseControl,
    /// Down on the left, up on the right.
    Volume,
    /// The phone's assistant.
    Voice,
    /// The Buds tell the PC (5 left, 6 right), which does what the settings say.
    Pc,
}

impl HoldAction {
    pub const ALL: [HoldAction; 4] = [HoldAction::NoiseControl, HoldAction::Volume, HoldAction::Voice, HoldAction::Pc];

    /// Left and right.
    pub fn bytes(self) -> [u8; 2] {
        match self {
            HoldAction::Voice => [1, 1],
            HoldAction::NoiseControl => [2, 2],
            HoldAction::Volume => [3, 3],
            HoldAction::Pc => [5, 6],
        }
    }

    pub fn from_bytes(b: [u8; 2]) -> Option<HoldAction> {
        HoldAction::ALL.into_iter().find(|a| a.bytes() == b)
    }

    pub fn label(self) -> &'static str {
        match self {
            HoldAction::NoiseControl => crate::i18n::tr("Gürültü kontrolü"),
            HoldAction::Volume => crate::i18n::tr("Ses düzeyi"),
            HoldAction::Voice => crate::i18n::tr("Sesli komut"),
            HoldAction::Pc => crate::i18n::tr("Bilgisayarda"),
        }
    }
}

/// How a bud sits in the ear, by the fit test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Good,
    Bad,
    Failed,
}

impl Fit {
    fn from_byte(b: u8) -> Option<Fit> {
        match b {
            0 => Some(Fit::Bad),
            1 => Some(Fit::Good),
            2 => Some(Fit::Failed),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Fit::Good => crate::i18n::tr("iyi oturuyor"),
            Fit::Bad => crate::i18n::tr("oturmuyor"),
            Fit::Failed => crate::i18n::tr("ölçülemedi"),
        }
    }
}

/// Without a result this long the fit test is over (the Buds' tone takes a few seconds).
pub const FIT_TIMEOUT_MS: u64 = 20_000;

/// Where the card's fit test is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FitTest {
    #[default]
    Idle,
    Running {
        since_ms: u64,
    },
    Done(Fit, Fit),
    TimedOut,
}

impl FitTest {
    /// A test still running after `FIT_TIMEOUT_MS` got no result.
    pub fn expired(self, now_ms: u64) -> FitTest {
        match self {
            FitTest::Running { since_ms } if now_ms.saturating_sub(since_ms) > FIT_TIMEOUT_MS => FitTest::TimedOut,
            other => other,
        }
    }
}

/// The fit test row's second line. The test needs both buds in; a result stays.
pub fn fit_line(t: FitTest, worn: Worn) -> String {
    use crate::i18n::{tr, trf};
    match t {
        FitTest::Running { .. } => tr("Ölçülüyor…").into(),
        FitTest::Done(Fit::Good, Fit::Good) => tr("İkisi de iyi oturuyor").into(),
        FitTest::Done(l, r) => trf("Sol: {} · Sağ: {}", &[&l.label(), &r.label()]),
        FitTest::TimedOut => tr("Test tamamlanamadı").into(),
        FitTest::Idle if worn.count() < 2 => tr("İki kulaklığı da tak").into(),
        FitTest::Idle => tr("Bir ses çalar, oturuşu ölçer").into(),
    }
}

pub fn fit_button(t: FitTest) -> &'static str {
    match t {
        FitTest::Idle => crate::i18n::tr("Başlat"),
        FitTest::Running { .. } => crate::i18n::tr("Durdur"),
        FitTest::Done(..) | FitTest::TimedOut => crate::i18n::tr("Yeniden"),
    }
}

/// A running test can always be stopped; a new one needs both buds in.
pub fn fit_can_press(t: FitTest, worn: Worn) -> bool {
    matches!(t, FitTest::Running { .. }) || worn.count() == 2
}

/// Which buds are in an ear.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worn {
    pub left: bool,
    pub right: bool,
}

impl Worn {
    pub fn count(self) -> u8 {
        self.left as u8 + self.right as u8
    }
}

/// What taking a bud out, or putting it back, does to the music.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EarAction {
    Pause,
    Resume,
}

/// Music the Buds paused: how many buds were in before, and when.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarPause {
    pub worn_before: u8,
    pub at_ms: u64,
}

/// Putting the buds back later than this doesn't start the music again.
pub const EAR_RESUME_MS: u64 = 15 * 60_000;

/// A bud coming out pauses (one is enough, as when taking one out to talk; another one out
/// pauses whatever started since); the music the Buds paused plays again once as many buds are
/// back in as before, within `EAR_RESUME_MS`. A pause older than that is no pause any more.
pub fn ear_action(prev: Worn, now: Worn, paused: Option<EarPause>, now_ms: u64) -> Option<EarAction> {
    let paused = paused.filter(|p| now_ms.saturating_sub(p.at_ms) <= EAR_RESUME_MS);
    if now.count() < prev.count() {
        return Some(EarAction::Pause);
    }
    paused.and_then(|p| (now.count() > prev.count() && now.count() >= p.worn_before).then_some(EarAction::Resume))
}

/// The Buds' noise control, in One UI 8's order: off, ambient sound, noise cancelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseControl {
    Anc,
    Off,
    Ambient,
    /// Buds3 Pro / Buds4 Pro (byte 3): shown, not offered as a button (setting it is untested).
    Adaptive,
}

impl NoiseControl {
    pub const ALL: [NoiseControl; 3] = [NoiseControl::Off, NoiseControl::Ambient, NoiseControl::Anc];

    pub fn byte(self) -> u8 {
        match self {
            NoiseControl::Off => 0,
            NoiseControl::Anc => 1,
            NoiseControl::Ambient => 2,
            NoiseControl::Adaptive => 3,
        }
    }

    pub fn from_byte(b: u8) -> Option<NoiseControl> {
        match b {
            0 => Some(NoiseControl::Off),
            1 => Some(NoiseControl::Anc),
            2 => Some(NoiseControl::Ambient),
            3 => Some(NoiseControl::Adaptive),
            _ => None,
        }
    }

    /// Samsung's icon for the mode (One UI 8, Galaxy Buds2 Manager 8.0.25061651), as SVG on a
    /// 24-unit round button: a person with a ring round the head, faint for off, dotted for
    /// ambient sound, solid for noise cancelling. Traced from the 132 px drawables in Samsung's
    /// SystemUI (soundcraft_ic_buds3_*): Samsung's art, not openly licensed.
    pub fn icon(self) -> &'static str {
        match self {
            NoiseControl::Off => r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 13.792C14.656 13.792 16.809 15.462 16.809 17.523V17.713A0.8 0.8 0 0 1 16.009 18.513H7.991A0.8 0.8 0 0 1 7.191 17.713V17.523C7.191 15.462 9.344 13.792 12 13.792ZM9.574 10.379a2.426 2.426 0 1 0 4.852 0a2.426 2.426 0 1 0 -4.852 0Z"/><path fill-opacity=".4" d="M7.072 13.715A5.78 5.78 0 1 1 16.928 13.715A0.48 0.48 0 0 1 16.11 13.213A4.82 4.82 0 1 0 7.89 13.213A0.48 0.48 0 0 1 7.072 13.715Z"/></svg>"#,
            NoiseControl::Ambient => r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 13.52C14.657 13.52 16.811 15.19 16.811 17.25V17.44A0.8 0.8 0 0 1 16.011 18.24H7.989A0.8 0.8 0 0 1 7.189 17.44V17.25C7.189 15.19 9.343 13.52 12 13.52ZM9.581 10.11a2.419 2.419 0 1 0 4.837 0a2.419 2.419 0 1 0 -4.837 0Z"/><path d="M6.881 11.833a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM6.593 9.501a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM7.423 7.302a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM9.181 5.742a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM11.463 5.179a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM13.745 5.742a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM15.502 7.302a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM16.333 9.501a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0ZM16.045 11.833a0.537 0.537 0 1 0 1.074 0a0.537 0.537 0 1 0 -1.074 0Z"/></svg>"#,
            NoiseControl::Anc | NoiseControl::Adaptive => r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 13.792C14.656 13.792 16.809 15.462 16.809 17.523V17.713A0.8 0.8 0 0 1 16.009 18.513H7.991A0.8 0.8 0 0 1 7.191 17.713V17.523C7.191 15.462 9.344 13.792 12 13.792ZM9.574 10.379a2.426 2.426 0 1 0 4.852 0a2.426 2.426 0 1 0 -4.852 0Z"/><path d="M7.072 13.715A5.78 5.78 0 1 1 16.928 13.715A0.48 0.48 0 0 1 16.11 13.213A4.82 4.82 0 1 0 7.89 13.213A0.48 0.48 0 0 1 7.072 13.715Z"/></svg>"#,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NoiseControl::Anc => crate::i18n::tr("Gürültü engelleme"),
            NoiseControl::Off => crate::i18n::tr("Kapalı"),
            NoiseControl::Ambient => crate::i18n::tr("Ortam sesi"),
            NoiseControl::Adaptive => crate::i18n::tr("Uyarlanabilir"),
        }
    }
}

/// The snapshot after `f`, from the `last` one: an extended status says the mode exactly (an
/// unknown byte is unknown, not the previous mode), a plain status has none (the last stays),
/// a noise update changes only the mode. `None` for frames that change nothing.
pub fn next_snapshot(f: &Frame, last: Option<BudsSnapshot>) -> Option<BudsSnapshot> {
    match f.id {
        EXTENDED_STATUS_UPDATED => parse_status(f),
        // What only an extended status says stays; the charging byte counts once it said the
        // revision has one.
        STATUS_UPDATED => parse_status(f).map(|s| {
            let charging = last.and_then(|l| l.charging).and(f.payload.get(7)).map(|b| Charging::from_byte(*b));
            BudsSnapshot { noise: last.and_then(|l| l.noise), touch: last.and_then(|l| l.touch), model: last.and_then(|l| l.model), settings: last.and_then(|l| l.settings), charging, ..s }
        }),
        NOISE_CONTROLS_UPDATE => last.map(|l| BudsSnapshot { noise: f.payload.first().and_then(|b| NoiseControl::from_byte(*b)), ..l }),
        _ => None,
    }
}

/// The Buds' model id (with its colour) from an extended status: [14..16], little-endian.
pub fn model_id(f: &Frame) -> Option<u16> {
    (f.id == EXTENDED_STATUS_UPDATED).then(|| f.payload.get(14..16).map(|b| u16::from_le_bytes([b[0], b[1]]))).flatten()
}

/// The model's name for the log (GalaxyBudsClient's Constants.cs DeviceIds ranges).
pub fn model_name(id: u16) -> &'static str {
    match id {
        279 => "Galaxy Buds Live",
        298 => "Galaxy Buds Pro",
        313..=321 => "Galaxy Buds2",
        322..=323 => "Galaxy Buds Core",
        325..=328 => "Galaxy Buds2 Pro",
        330..=331 => "Galaxy Buds FE",
        333..=334 => "Galaxy Buds3",
        340..=341 => "Galaxy Buds3 Pro",
        347..=348 => "Galaxy Buds3 FE",
        355..=356 => "Galaxy Buds4",
        359..=361 => "Galaxy Buds4 Pro",
        _ => crate::i18n::tr("bilinmeyen model"),
    }
}

/// Buds Pro speak the Buds2 protocol on the older serial service: their name (a renamed pair
/// keeps the model: "Ekin's Buds Pro") tells them apart from every other device offering that
/// common service.
pub fn is_buds_pro_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("buds pro") && !["pixel", "redmi", "oneplus", "nothing"].iter().any(|o| n.contains(o))
}

/// A paired device offering a serial service the Buds use: its Bluetooth address
/// ("A1:B2:C3:D4:E5:F6"), its name, and whether it offers Samsung's own service (or only the
/// standard one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paired {
    pub address: String,
    pub name: String,
    pub samsung: bool,
}

/// A Bluetooth address as the settings keep it.
pub fn address_text(a: u64) -> String {
    (0..6).rev().map(|i| format!("{:02X}", (a >> (i * 8)) & 0xFF)).collect::<Vec<_>>().join(":")
}

/// Which paired devices are the Buds: the one chosen in the settings while it's paired (one pair
/// of two, a renamed Buds Pro), else every one offering Samsung's service and the Buds Pro (by
/// name) on the standard one.
pub fn buds_among(paired: &[Paired], chosen: Option<&str>) -> Vec<usize> {
    if let Some(i) = chosen.and_then(|c| paired.iter().position(|p| p.address.eq_ignore_ascii_case(c))) {
        return vec![i];
    }
    (0..paired.len()).filter(|&i| paired[i].samsung || is_buds_pro_name(&paired[i].name)).collect()
}

/// The noise control mode a frame carries, if any.
pub fn parse_noise(f: &Frame) -> Option<NoiseControl> {
    match f.id {
        NOISE_CONTROLS_UPDATE => NoiseControl::from_byte(*f.payload.first()?),
        EXTENDED_STATUS_UPDATED => NoiseControl::from_byte(*f.payload.get(NOISE_AT)?),
        _ => None,
    }
}

/// A frame to send: `FD | length | id | payload | crc | DD`, no header flags (a request).
pub fn encode(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut body = vec![id];
    body.extend_from_slice(payload);
    let len = (body.len() + 2) as u16;
    let mut out = vec![SOM];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc16(&body).to_le_bytes());
    out.push(EOM);
    out
}

/// The request that switches the Buds' noise control.
pub fn set_noise_frame(mode: NoiseControl) -> Vec<u8> {
    encode(NOISE_CONTROLS, &[mode.byte()])
}

/// Whether touches are set here for a model at an extended status revision, and whether the
/// lock request carries the in-a-call gestures (GalaxyBudsClient's device specs). Buds3 and later
/// add swipes the request doesn't carry; Buds Pro and older have a one-byte lock.
fn touch_rules(model: u16, revision: u8) -> Option<bool> {
    match model {
        313..=321 if revision >= 4 => Some(revision >= 7),
        325..=328 => Some(revision >= 1),
        330..=331 => Some(true),
        _ => None,
    }
}

/// The touch controls an extended status carries, on the models they're set on here.
fn parse_touch(f: &Frame) -> Option<Touch> {
    let calls = touch_rules(model_id(f)?, *f.payload.first()?)?;
    let (lock, hold) = (*f.payload.get(TOUCH_AT)?, *f.payload.get(HOLD_AT)?);
    Some(Touch { on: lock & TOUCH_ON != 0, gestures: lock & GESTURES, hold: [hold >> 4, hold & 0x0F], calls })
}

/// The request that switches touches on or off, the gestures as they are.
pub fn touch_frame(t: &Touch, on: bool) -> Vec<u8> {
    let bit = |n: u8| (t.gestures >> n) & 1;
    let mut payload = vec![on as u8, bit(3), bit(2), bit(1), bit(0)];
    if t.calls {
        payload.extend([bit(4), bit(5)]);
    }
    encode(LOCK_TOUCHPAD, &payload)
}

/// The request that sets touch and hold on both buds.
pub fn hold_frame(action: HoldAction) -> Vec<u8> {
    encode(SET_TOUCHPAD_OPTION, &action.bytes())
}

/// The request that starts the fit test (its tone) or stops it.
pub fn fit_frame(on: bool) -> Vec<u8> {
    encode(CHECK_FIT, &[on as u8])
}

/// The Buds' answers to the card's requests (0x42 ACK [id, …]; not the noise control's, which
/// are many), a touch change made elsewhere (0x91), a fit result, a long press for the PC (0x93),
/// find my earbuds stopping or muting (0xA1, 0xA3): what the worker logs beyond the first frames.
pub fn is_news(f: &Frame) -> bool {
    // The card's other requests: equalizer, ambient sound, its customisation, one-earbud noise
    // control, edge taps, sidetone, seamless connection, find my earbuds and its mute.
    const SETTINGS: [u8; 10] = [0x86, 0x84, 0x82, 0x6F, 0x95, 0x8B, 0xAF, 0xA0, 0xA1, 0xA2];
    match f.id {
        ACK => f.payload.first().is_some_and(|id| matches!(*id, LOCK_TOUCHPAD | SET_TOUCHPAD_OPTION | CHECK_FIT) || SETTINGS.contains(id)),
        TOUCH_UPDATED | FIT_RESULT | 0x93 | 0xA1 | 0xA3 => true,
        _ => false,
    }
}

/// The fit test's result, left and right.
pub fn parse_fit(f: &Frame) -> Option<(Fit, Fit)> {
    if f.id != FIT_RESULT {
        return None;
    }
    Some((Fit::from_byte(*f.payload.first()?)?, Fit::from_byte(*f.payload.get(1)?)?))
}

/// CRC-16/XMODEM.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

enum Parse {
    Complete(Frame, usize),
    Incomplete,
    Invalid,
}

/// Reads one frame at the start of `b` (which starts with SOM).
fn parse_frame(b: &[u8]) -> Parse {
    if b.len() < 3 {
        return Parse::Incomplete;
    }
    let len = (u16::from_le_bytes([b[1], b[2]]) & LENGTH_MASK) as usize;
    if len < 3 {
        return Parse::Invalid;
    }
    let total = len + 4;
    if b.len() < total {
        return Parse::Incomplete;
    }
    let body = &b[3..3 + len - 2];
    let crc = u16::from_le_bytes([b[3 + len - 2], b[3 + len - 1]]);
    if b[total - 1] != EOM || crc16(body) != crc {
        return Parse::Invalid;
    }
    Parse::Complete(Frame { id: body[0], payload: body[1..].to_vec() }, total)
}

/// Takes every complete frame out of `buf`, keeping a partial frame at the end for the next read.
/// Bytes before a start marker are dropped; a start marker that doesn't begin a valid frame (0xFD
/// also occurs inside payloads) is skipped, even if its "length" asks to wait for more bytes when
/// a valid frame already follows it.
pub fn take_frames(buf: &mut Vec<u8>) -> Vec<Frame> {
    let mut frames = Vec::new();
    loop {
        match buf.iter().position(|&b| b == SOM) {
            Some(start) => {
                buf.drain(..start);
            }
            None => {
                buf.clear();
                return frames;
            }
        }
        match parse_frame(buf) {
            Parse::Complete(frame, n) => {
                frames.push(frame);
                buf.drain(..n);
            }
            Parse::Invalid => {
                buf.drain(..1);
            }
            Parse::Incomplete => {
                let later = (1..buf.len()).find(|&i| buf[i] == SOM && matches!(parse_frame(&buf[i..]), Parse::Complete(..)));
                match later {
                    Some(i) => {
                        buf.drain(..i);
                    }
                    None => return frames,
                }
            }
        }
    }
}

/// Battery from STATUS_UPDATED or EXTENDED_STATUS_UPDATED; `None` for other messages.
pub fn parse_status(f: &Frame) -> Option<BudsSnapshot> {
    let (left, right, placement, case) = match f.id {
        STATUS_UPDATED if f.payload.len() >= 7 => (1, 2, 5, 6),
        EXTENDED_STATUS_UPDATED if f.payload.len() >= 8 => (2, 3, 6, 7),
        _ => return None,
    };
    let p = &f.payload;
    let docked = |nibble: u8| nibble == IN_OPEN_CASE || nibble == IN_CLOSED_CASE;
    let in_case = docked(p[placement] >> 4) || docked(p[placement] & 0x0F);
    let case = Some(p[case]).filter(|c| in_case && (1..=100).contains(c));
    let worn = Worn { left: p[placement] >> 4 == WEARING, right: p[placement] & 0x0F == WEARING };
    let place = [Place::from_nibble(p[placement] >> 4), Place::from_nibble(p[placement] & 0x0F)];
    // Buds2's [36] from revision 10 (later models keep it elsewhere: GBC's decoder L534-593).
    let charging = (f.id == EXTENDED_STATUS_UPDATED && p[0] >= 10 && matches!(model_id(f), Some(313..=321))).then(|| p.get(36).map(|b| Charging::from_byte(*b))).flatten();
    Some(BudsSnapshot {
        left: p[left],
        right: p[right],
        case,
        noise: parse_noise(f),
        worn,
        touch: parse_touch(f),
        place,
        charging,
        model: model_id(f),
        settings: crate::activities::buds_controls::parse_settings(f),
    })
}

/// Below this (settings, 15 % by default) the Buds come to the front and their rings turn red.
pub fn low_battery() -> u8 {
    crate::settings::current().buds_low_battery_pct
}

/// A bud without a reading (0) doesn't count.
pub fn is_low(s: &BudsSnapshot) -> bool {
    let low = low_battery();
    [s.left, s.right].into_iter().filter(|&v| v > 0).any(|v| v < low)
}

/// "%85", or "—" for a bud without a reading.
pub fn percent(v: u8) -> String {
    if v == 0 { "—".to_string() } else { crate::i18n::percent(v) }
}

/// The pill's text: "%85 · %80" (left · right) and, while known, "Kutu %60".
pub fn buds_labels(s: &BudsSnapshot) -> (String, Option<String>) {
    (format!("{} · {}", percent(s.left), percent(s.right)), s.case.map(|c| crate::i18n::trf("Kutu {}", &[&percent(c)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    /// A real Buds2 EXTENDED_STATUS_UPDATED (revision 10): L 100, R 100, both worn, case 51.
    const REAL_EXTENDED: &str = "FD 28 10 61 0A 03 64 64 01 01 11 33 00 03 3F 22 01 00 3C 01 3C 01 02 00 \
        03 66 00 01 00 10 00 00 00 00 11 02 01 01 00 00 00 BE A3 DD";
    /// The same frame with L 85 (worn), R 80 (in the open case), case 60.
    const EXTENDED_IN_CASE: &str = "FD 28 10 61 0A 03 55 50 01 01 13 3C 00 03 3F 22 01 00 3C 01 3C 01 02 00 \
        03 66 00 01 00 10 00 00 00 00 11 02 01 01 00 00 04 3E 6E DD";
    /// STATUS_UPDATED with the same readings.
    const STATUS_IN_CASE: &str = "FD 0B 10 60 01 55 50 01 01 13 3C 04 F8 9A DD";

    /// REAL_EXTENDED's payload with the noise byte at [12] set to `mode`.
    fn extended_with_noise(mode: u8) -> Frame {
        let mut f = take_frames(&mut hex(REAL_EXTENDED)).remove(0);
        f.payload[12] = mode;
        f
    }

    #[test]
    fn adaptive_is_read_not_mistaken_for_the_last_mode() {
        let anc = BudsSnapshot { left: 100, right: 100, case: None, noise: Some(NoiseControl::Anc), worn: Worn::default(), ..Default::default() };
        assert_eq!(next_snapshot(&extended_with_noise(3), Some(anc)).unwrap().noise, Some(NoiseControl::Adaptive));
        assert_eq!(next_snapshot(&extended_with_noise(9), Some(anc)).unwrap().noise, None, "an unknown mode is unknown, not the last one");
        let update = |b: u8| Frame { id: 0x77, payload: vec![b] };
        assert_eq!(next_snapshot(&update(3), Some(anc)).unwrap().noise, Some(NoiseControl::Adaptive));
        assert_eq!(next_snapshot(&update(9), Some(anc)).unwrap().noise, None);
        assert_eq!(next_snapshot(&update(1), None), None, "a mode alone, before any battery reading");
        let status = take_frames(&mut hex(STATUS_IN_CASE)).remove(0);
        assert_eq!(next_snapshot(&status, Some(anc)).unwrap().noise, Some(NoiseControl::Anc), "0x60 has no mode: the last one stays");
        assert_eq!(NoiseControl::Adaptive.label(), "Uyarlanabilir");
        assert!(!NoiseControl::ALL.contains(&NoiseControl::Adaptive), "not a button: setting it is untested");
    }

    #[test]
    fn the_model_comes_from_the_extended_status() {
        assert_eq!(model_id(&extended_with_noise(1)), Some(316), "a Buds2 (GalaxyBudsClient's DeviceIds 313-321)");
        assert_eq!(model_name(316), "Galaxy Buds2");
        assert_eq!(model_name(341), "Galaxy Buds3 Pro");
        assert_eq!(model_name(1), "bilinmeyen model");
    }

    #[test]
    fn the_older_service_only_for_buds_pro() {
        assert!(is_buds_pro_name("Galaxy Buds Pro (A1B2)"));
        assert!(!is_buds_pro_name("Galaxy Buds2 Pro (A1B2)"), "on the new service already");
        assert!(!is_buds_pro_name("Galaxy Buds+ (A1B2)"), "another protocol");
        assert!(!is_buds_pro_name("JBL Tune 510BT"));
        assert!(is_buds_pro_name("Ekin's Buds Pro"), "renamed by the phone");
        assert!(is_buds_pro_name("Ekin adlı kişiye ait Buds Pro"));
        assert!(!is_buds_pro_name("Ayşe's Buds3 Pro"));
        assert!(!is_buds_pro_name("Pixel Buds Pro"), "Google's");
    }

    #[test]
    fn noise_control_from_status_and_update() {
        assert_eq!(parse_noise(&frames_of(&hex(REAL_EXTENDED))[0]), Some(NoiseControl::Anc), "the real capture had it on");
        assert_eq!(parse_status(&frames_of(&hex(REAL_EXTENDED))[0]).unwrap().noise, Some(NoiseControl::Anc));
        assert_eq!(parse_noise(&Frame { id: 0x77, payload: vec![2, 1] }), Some(NoiseControl::Ambient));
        assert_eq!(parse_noise(&Frame { id: 0x77, payload: vec![0] }), Some(NoiseControl::Off));
        assert_eq!(parse_noise(&Frame { id: 0x77, payload: vec![9] }), None, "a mode we don't know");
        assert_eq!(parse_noise(&frames_of(&hex(STATUS_IN_CASE))[0]), None, "STATUS_UPDATED doesn't carry it");
    }

    #[test]
    fn noise_control_request_frame() {
        let f = set_noise_frame(NoiseControl::Ambient);
        assert_eq!(f.get(..5), Some(&[0xFD, 0x04, 0x00, 0x78, 0x02][..]));
        assert_eq!(f.last(), Some(&0xDD));
        assert_eq!(frames_of(&f), vec![Frame { id: 0x78, payload: vec![2] }], "our own parser reads it back");
    }

    fn frames_of(bytes: &[u8]) -> Vec<Frame> {
        let mut buf = bytes.to_vec();
        take_frames(&mut buf)
    }

    #[test]
    fn crc_known_vector() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
        let frame = hex(REAL_EXTENDED);
        assert_eq!(crc16(&frame[3..frame.len() - 3]), 0xA3BE);
    }

    #[test]
    fn take_frames_split_across_reads() {
        let bytes = hex(REAL_EXTENDED);
        let mut buf = bytes[..10].to_vec();
        assert!(take_frames(&mut buf).is_empty());
        buf.extend_from_slice(&bytes[10..]);
        let frames = take_frames(&mut buf);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, 0x61);
        assert_eq!(frames[0].payload.len(), 37);
        assert!(buf.is_empty());
    }

    #[test]
    fn take_frames_skips_garbage() {
        // A stray start byte whose "length" would make us wait for hundreds of bytes.
        let mut bytes = hex("00 FD 12");
        bytes.extend(hex(STATUS_IN_CASE));
        bytes.extend(hex(REAL_EXTENDED));
        let ids: Vec<u8> = frames_of(&bytes).iter().map(|f| f.id).collect();
        assert_eq!(ids, vec![0x60, 0x61]);
    }

    #[test]
    fn take_frames_rejects_bad_crc() {
        let mut bytes = hex(REAL_EXTENDED);
        bytes[6] ^= 0x01;
        let mut buf = bytes.clone();
        assert!(take_frames(&mut buf).is_empty());
        assert!(buf.len() < bytes.len(), "a bad frame must not block the buffer");
    }

    const BOTH_IN: Worn = Worn { left: true, right: true };
    const LEFT_IN: Worn = Worn { left: true, right: false };
    const NONE_IN: Worn = Worn { left: false, right: false };

    #[test]
    fn a_bud_coming_out_pauses_and_putting_it_back_resumes() {
        use EarAction::*;
        assert_eq!(ear_action(BOTH_IN, LEFT_IN, None, 1_000), Some(Pause), "one out is enough");
        let paused = Some(EarPause { worn_before: 2, at_ms: 1_000 });
        assert_eq!(ear_action(LEFT_IN, NONE_IN, paused, 2_000), Some(Pause), "the other one too: whatever started since pauses");
        assert_eq!(ear_action(NONE_IN, LEFT_IN, paused, 3_000), None, "one of two back isn't enough");
        assert_eq!(ear_action(LEFT_IN, BOTH_IN, paused, 4_000), Some(Resume));
        // Listening with one bud: taking it out pauses, putting it back resumes.
        assert_eq!(ear_action(LEFT_IN, NONE_IN, None, 1_000), Some(Pause));
        assert_eq!(ear_action(NONE_IN, LEFT_IN, Some(EarPause { worn_before: 1, at_ms: 1_000 }), 5_000), Some(Resume));
    }

    #[test]
    fn putting_buds_in_without_a_pause_or_too_late_does_nothing() {
        assert_eq!(ear_action(NONE_IN, BOTH_IN, None, 1_000), None, "nothing to resume");
        assert_eq!(ear_action(BOTH_IN, BOTH_IN, None, 1_000), None);
        let paused = Some(EarPause { worn_before: 2, at_ms: 1_000 });
        assert_eq!(ear_action(LEFT_IN, BOTH_IN, paused, 1_000 + EAR_RESUME_MS + 1), None, "too late");
    }

    #[test]
    fn a_stale_pause_doesnt_stop_the_next_one() {
        // Paused by the Buds, then the user came back after 20 min and played something else.
        let stale = Some(EarPause { worn_before: 2, at_ms: 1_000 });
        assert_eq!(ear_action(BOTH_IN, LEFT_IN, stale, 1_000 + 20 * 60_000), Some(EarAction::Pause));
    }

    /// The capture's touch controls: off, every gesture on, noise control on both buds.
    const CAPTURE_TOUCH: Touch = Touch { on: false, gestures: 0x3F, hold: [2, 2], calls: true };

    /// REAL_EXTENDED with its revision [0], touch controls [10] and touch and hold [11] set.
    fn extended_with_touch(revision: u8, touch: u8, hold: u8) -> Frame {
        let mut f = take_frames(&mut hex(REAL_EXTENDED)).remove(0);
        f.payload[0] = revision;
        f.payload[10] = touch;
        f.payload[11] = hold;
        f
    }

    #[test]
    fn touch_controls_come_from_the_extended_status() {
        assert_eq!(parse_status(&frames_of(&hex(REAL_EXTENDED))[0]).unwrap().touch, Some(CAPTURE_TOUCH));
        let on = parse_status(&extended_with_touch(11, 0xBF, 0x23)).unwrap().touch.unwrap();
        assert!(on.on);
        assert_eq!(on.gestures, 0x3F);
        assert_eq!(on.hold, [2, 3], "high nibble left, low nibble right");
        assert!(!parse_status(&extended_with_touch(6, 0xBF, 0x22)).unwrap().touch.unwrap().calls, "Buds2 before revision 7: no in-a-call gestures");
        assert_eq!(parse_status(&extended_with_touch(3, 0xBF, 0x22)).unwrap().touch, None, "Buds2 before revision 4: the old lock, not set here");
    }

    #[test]
    fn touch_only_on_the_models_it_is_set_on() {
        let with_model = |id: u16| {
            let mut f = extended_with_touch(10, 0xBF, 0x22);
            f.payload[14..16].copy_from_slice(&id.to_le_bytes());
            parse_status(&f).unwrap().touch
        };
        assert!(with_model(326).is_some(), "Buds2 Pro");
        assert!(with_model(330).is_some(), "Buds FE");
        assert_eq!(with_model(334), None, "Buds3: swipes the request doesn't carry");
        assert_eq!(with_model(298), None, "Buds Pro: the old lock");
        assert_eq!(parse_status(&frames_of(&hex(STATUS_IN_CASE))[0]).unwrap().touch, None, "0x60 doesn't carry it");
    }

    #[test]
    fn touch_is_kept_by_the_other_updates() {
        let last = parse_status(&frames_of(&hex(REAL_EXTENDED))[0]);
        let status = frames_of(&hex(STATUS_IN_CASE)).remove(0);
        assert_eq!(next_snapshot(&status, last).unwrap().touch, Some(CAPTURE_TOUCH));
        assert_eq!(next_snapshot(&Frame { id: 0x77, payload: vec![2] }, last).unwrap().touch, Some(CAPTURE_TOUCH));
    }

    #[test]
    fn the_lock_request_keeps_the_gestures() {
        // Hold, single and hold in a call on; the others off.
        let t = Touch { on: true, gestures: 0b10_1001, hold: [2, 2], calls: true };
        assert_eq!(frames_of(&touch_frame(&t, false)), vec![Frame { id: 0x90, payload: vec![0, 1, 0, 0, 1, 0, 1] }]);
        assert_eq!(frames_of(&touch_frame(&t, true))[0].payload[0], 1);
        assert_eq!(frames_of(&touch_frame(&Touch { calls: false, ..t }, true))[0].payload, vec![1, 1, 0, 0, 1]);
    }

    #[test]
    fn touch_and_hold_on_both_buds() {
        assert_eq!(frames_of(&hold_frame(HoldAction::Volume)), vec![Frame { id: 0x92, payload: vec![3, 3] }]);
        assert_eq!(CAPTURE_TOUCH.hold_both(), Some(HoldAction::NoiseControl));
        assert_eq!(Touch { hold: [2, 3], ..CAPTURE_TOUCH }.hold_both(), None, "the buds differ");
        assert_eq!(Touch { hold: [4, 4], ..CAPTURE_TOUCH }.hold_both(), None, "Spotify isn't offered");
        assert_eq!(HoldAction::Voice.label(), "Sesli komut");
    }

    #[test]
    fn fit_test_frames_and_result() {
        assert_eq!(frames_of(&fit_frame(true)), vec![Frame { id: 0x9D, payload: vec![1] }]);
        assert_eq!(frames_of(&fit_frame(false)), vec![Frame { id: 0x9D, payload: vec![0] }]);
        assert_eq!(parse_fit(&Frame { id: 0x9E, payload: vec![1, 0] }), Some((Fit::Good, Fit::Bad)));
        assert_eq!(parse_fit(&Frame { id: 0x9E, payload: vec![2, 1] }), Some((Fit::Failed, Fit::Good)));
        assert_eq!(parse_fit(&Frame { id: 0x9E, payload: vec![7, 1] }), None);
        assert_eq!(parse_fit(&Frame { id: 0x9E, payload: vec![1] }), None);
        assert_eq!(parse_fit(&Frame { id: 0x61, payload: vec![1, 1] }), None);
    }

    #[test]
    fn where_each_bud_is() {
        let s = parse_status(&frames_of(&hex(EXTENDED_IN_CASE))[0]).unwrap();
        assert_eq!(s.place, [Place::Worn, Place::InCase]);
        let mut f = take_frames(&mut hex(REAL_EXTENDED)).remove(0);
        f.payload[6] = 0x20;
        assert_eq!(parse_status(&f).unwrap().place, [Place::Out, Place::Off]);
        f.payload[6] = 0x42;
        assert_eq!(parse_status(&f).unwrap().place, [Place::InCase, Place::Out], "the closed case too");
    }

    #[test]
    fn charging_from_revision_10() {
        let mut f = take_frames(&mut hex(REAL_EXTENDED)).remove(0);
        assert_eq!(parse_status(&f).unwrap().charging, Some(Charging::default()), "the capture: nothing charging");
        f.payload[36] = 0x55;
        assert_eq!(parse_status(&f).unwrap().charging, Some(Charging { left: true, right: true, case: true }));
        f.payload[0] = 9;
        assert_eq!(parse_status(&f).unwrap().charging, None);
        // 0x60 [7] once an extended status said the revision has it.
        let last = parse_status(&take_frames(&mut hex(REAL_EXTENDED)).remove(0));
        let status = take_frames(&mut hex(STATUS_IN_CASE)).remove(0);
        assert_eq!(next_snapshot(&status, last).unwrap().charging, Some(Charging { left: false, right: true, case: false }));
        assert_eq!(next_snapshot(&status, None).unwrap().charging, None);
    }

    #[test]
    fn the_model_and_settings_stay_through_other_updates() {
        let last = parse_status(&take_frames(&mut hex(REAL_EXTENDED)).remove(0));
        assert_eq!(last.unwrap().model, Some(316));
        assert!(last.unwrap().settings.is_some());
        let status = take_frames(&mut hex(STATUS_IN_CASE)).remove(0);
        let next = next_snapshot(&status, last).unwrap();
        assert_eq!((next.model, next.settings), (last.unwrap().model, last.unwrap().settings));
        assert_eq!(parse_status(&status).unwrap().model, None, "0x60 alone doesn't say");
    }

    #[test]
    fn touch_and_hold_on_the_pc() {
        assert_eq!(frames_of(&hold_frame(HoldAction::Pc)), vec![Frame { id: 0x92, payload: vec![5, 6] }], "5 left, 6 right");
        assert_eq!(Touch { hold: [5, 6], ..CAPTURE_TOUCH }.hold_both(), Some(HoldAction::Pc));
        assert_eq!(Touch { hold: [5, 5], ..CAPTURE_TOUCH }.hold_both(), None);
        assert_eq!(HoldAction::Pc.label(), "Bilgisayarda");
    }

    #[test]
    fn the_answers_to_touch_and_fit_requests_are_news() {
        assert!(is_news(&Frame { id: 0x42, payload: vec![0x90, 1, 1, 1, 1, 1, 1, 1] }), "the lock's ACK");
        assert!(is_news(&Frame { id: 0x42, payload: vec![0x92, 3, 3] }));
        assert!(is_news(&Frame { id: 0x42, payload: vec![0x9D, 1] }));
        assert!(is_news(&Frame { id: 0x91, payload: vec![1] }), "touch changed elsewhere");
        assert!(is_news(&Frame { id: 0x9E, payload: vec![1, 1] }));
        assert!(is_news(&Frame { id: 0x42, payload: vec![0x86, 1] }), "the equalizer's ACK, and the other settings'");
        assert!(is_news(&Frame { id: 0x42, payload: vec![0xAF, 0] }));
        assert!(is_news(&Frame { id: 0x42, payload: vec![0xA0] }), "find my earbuds");
        assert!(is_news(&Frame { id: 0x93, payload: vec![5] }), "a long press for the PC");
        assert!(is_news(&Frame { id: 0xA3, payload: vec![0, 1] }));
        assert!(!is_news(&Frame { id: 0x42, payload: vec![0x78, 1] }), "noise control switches are many");
        assert!(!is_news(&Frame { id: 0x42, payload: vec![] }));
        assert!(!is_news(&frames_of(&hex(REAL_EXTENDED))[0]));
    }

    #[test]
    fn a_fit_test_without_a_result_gives_up() {
        let running = FitTest::Running { since_ms: 1_000 };
        assert_eq!(running.expired(1_000 + FIT_TIMEOUT_MS), running);
        assert_eq!(running.expired(1_000 + FIT_TIMEOUT_MS + 1), FitTest::TimedOut);
        assert_eq!(FitTest::Done(Fit::Good, Fit::Good).expired(u64::MAX), FitTest::Done(Fit::Good, Fit::Good));
    }

    #[test]
    fn the_fit_row_says_where_the_test_is() {
        assert_eq!(fit_line(FitTest::Idle, BOTH_IN), "Bir ses çalar, oturuşu ölçer");
        assert_eq!(fit_line(FitTest::Idle, LEFT_IN), "İki kulaklığı da tak");
        assert_eq!(fit_line(FitTest::Running { since_ms: 0 }, BOTH_IN), "Ölçülüyor…");
        assert_eq!(fit_line(FitTest::Done(Fit::Good, Fit::Good), BOTH_IN), "İkisi de iyi oturuyor");
        assert_eq!(fit_line(FitTest::Done(Fit::Good, Fit::Bad), BOTH_IN), "Sol: iyi oturuyor · Sağ: oturmuyor");
        assert_eq!(fit_line(FitTest::Done(Fit::Failed, Fit::Good), LEFT_IN), "Sol: ölçülemedi · Sağ: iyi oturuyor", "a result stays when a bud comes out");
        assert_eq!(fit_line(FitTest::TimedOut, BOTH_IN), "Test tamamlanamadı");
        assert_eq!(fit_button(FitTest::Idle), "Başlat");
        assert_eq!(fit_button(FitTest::Running { since_ms: 0 }), "Durdur");
        assert_eq!(fit_button(FitTest::TimedOut), "Yeniden");
        assert!(fit_can_press(FitTest::Running { since_ms: 0 }, NONE_IN), "stopping always works");
        assert!(!fit_can_press(FitTest::Idle, LEFT_IN));
        assert!(fit_can_press(FitTest::TimedOut, BOTH_IN));
    }

    #[test]
    fn the_fit_row_in_english() {
        crate::i18n::in_language(crate::i18n::Language::English, || {
            assert_eq!(fit_line(FitTest::Done(Fit::Good, Fit::Bad), BOTH_IN), "Left: good fit · Right: loose");
            assert_eq!(HoldAction::NoiseControl.label(), "Noise control");
        });
    }

    /// The fields every status has.
    fn basics(s: BudsSnapshot) -> (u8, u8, Option<u8>, Option<NoiseControl>, Worn, Option<Touch>) {
        (s.left, s.right, s.case, s.noise, s.worn, s.touch)
    }

    #[test]
    fn parse_extended_status() {
        let f = &frames_of(&hex(EXTENDED_IN_CASE))[0];
        assert_eq!(basics(parse_status(f).unwrap()), (85, 80, Some(60), Some(NoiseControl::Anc), LEFT_IN, Some(CAPTURE_TOUCH)));
    }

    #[test]
    fn parse_short_status() {
        let f = &frames_of(&hex(STATUS_IN_CASE))[0];
        assert_eq!(basics(parse_status(f).unwrap()), (85, 80, Some(60), None, LEFT_IN, None));
    }

    #[test]
    fn case_hidden_when_no_bud_in_case() {
        let f = &frames_of(&hex(REAL_EXTENDED))[0];
        assert_eq!(basics(parse_status(f).unwrap()), (100, 100, None, Some(NoiseControl::Anc), BOTH_IN, Some(CAPTURE_TOUCH)));
    }

    #[test]
    fn other_messages_are_not_status() {
        let f = Frame { id: 0x88, payload: vec![1, 1, 0x22] };
        assert_eq!(parse_status(&f), None);
        assert_eq!(parse_status(&Frame { id: 0x61, payload: vec![10, 3, 50] }), None);
    }

    /// A bud reporting 0 has no reading (it is out of range), not an empty battery.
    #[test]
    fn zero_means_no_reading() {
        let s = BudsSnapshot { left: 0, right: 80, case: None, noise: None, worn: Worn::default(), ..Default::default() };
        assert_eq!(buds_labels(&s).0, "— · %80");
        assert!(!is_low(&s));
    }

    #[test]
    fn labels_show_case_only_when_known() {
        let s = BudsSnapshot { left: 85, right: 80, case: Some(60), noise: None, worn: Worn::default(), ..Default::default() };
        assert_eq!(buds_labels(&s), ("%85 · %80".to_string(), Some("Kutu %60".to_string())));
        assert_eq!(buds_labels(&BudsSnapshot { case: None, ..s }).1, None);
    }

    #[test]
    fn low_battery_below_15() {
        assert!(is_low(&BudsSnapshot { left: 14, right: 80, case: None, noise: None, worn: Worn::default(), ..Default::default() }));
        assert!(is_low(&BudsSnapshot { left: 80, right: 9, case: Some(100), noise: None, worn: Worn::default(), ..Default::default() }));
        assert!(!is_low(&BudsSnapshot { left: 15, right: 80, case: None, noise: None, worn: Worn::default(), ..Default::default() }));
    }

    fn paired(address: &str, name: &str, samsung: bool) -> Paired {
        Paired { address: address.into(), name: name.into(), samsung }
    }

    #[test]
    fn addresses_are_written_as_the_settings_keep_them() {
        assert_eq!(address_text(0x0200_0000_000A), "02:00:00:00:00:0A");
        assert_eq!(address_text(0xA1B2_C3D4_E5F6), "A1:B2:C3:D4:E5:F6");
    }

    #[test]
    fn the_chosen_pair_wins_while_it_is_paired() {
        let all = [
            paired("02:00:00:00:00:01", "Galaxy Buds2", true),
            paired("02:00:00:00:00:02", "Ekin's Buds Pro", false),
            paired("02:00:00:00:00:03", "Kulaklığım", false),
            paired("02:00:00:00:00:04", "HC-05", false),
            paired("02:00:00:00:00:0A", "Galaxy Buds3 Pro", true),
        ];
        assert_eq!(buds_among(&all, None), vec![0, 1, 4], "Samsung's service, and Buds Pro by name");
        assert_eq!(buds_among(&all, Some("02:00:00:00:00:03")), vec![2], "a renamed Buds Pro, chosen");
        assert_eq!(buds_among(&all, Some("02:00:00:00:00:0a")), vec![4], "one pair of two; case aside");
        assert_eq!(buds_among(&all, Some("02:00:00:00:00:FF")), vec![0, 1, 4], "no longer paired: automatic");
        assert!(buds_among(&[], None).is_empty());
    }
}
