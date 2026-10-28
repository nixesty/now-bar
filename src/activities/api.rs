//! The app notifications API (DOCS.md): what other programs send over loopback HTTP, judged
//! before anything reaches the pill. The pure part; platform/api_server.rs owns the sockets.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};

use crate::activities::live::{cut, AppIcon, LiveState};
use crate::settings::ApiApp;

/// What the UI thread answers a request with: `Shown` first when the request waits for a click
/// (its prompt was answered, its item is up), then `Done`.
#[derive(Debug)]
pub enum Reply {
    Shown,
    Done { status: u16, body: String, retry_after: Option<u32> },
}

impl Reply {
    pub fn reject(r: Reject) -> Reply {
        Reply::Done { status: r.status(), body: reject_json(r, None), retry_after: r.retry_after() }
    }

    pub fn ok(body: String) -> Reply {
        Reply::Done { status: 200, body, retry_after: None }
    }
}

#[derive(Debug)]
pub enum ApiWhat {
    /// A notification or an activity; a notification without an id has been given one.
    /// `exe_icon`: the caller exe's own, for a prompt (which never shows the app's choice).
    Show { req: Box<AppRequest>, icon: Option<AppIcon>, exe_icon: Option<AppIcon> },
    Events { wait_s: u32 },
}

/// One request for the UI thread, and where its answer goes.
#[derive(Debug)]
pub struct ApiCall {
    pub caller: Caller,
    pub what: ApiWhat,
    pub reply: Sender<Reply>,
}

pub type ApiMailbox = Arc<Mutex<Vec<ApiCall>>>;

/// Where the Now Bar listens when it can: 0x4E42, "NB", the WM_COPYDATA tag's number.
pub const DEFAULT_PORT: u16 = 20034;
pub const API_VERSION: u32 = 1;
pub const MAX_HEAD: usize = 8 * 1024;
pub const MAX_BODY: usize = 96 * 1024;
pub const MAX_ICON_BYTES: usize = 64 * 1024;
pub const MAX_ICON_PX: u32 = 512;
/// How long a request waits for the user's answer to its app's prompt.
pub const ASK_WAIT_MS: u64 = 20_000;
/// How long a prompt stays in the pill.
pub const PROMPT_MS: u64 = 120_000;
/// After a prompt is denied or ignored its exe asks no sooner than this, under any name.
pub const COOLDOWN_MS: u64 = 600_000;
/// A button counts once its item has been on screen this long: a click aimed at what was there
/// before lands on nothing.
pub const SETTLE_MS: u64 = 1_000;
pub const PER_APP_MAX: usize = 3;
pub const EVENTS_MAX: usize = 32;
pub const EVENT_TTL_MS: u64 = 600_000;
const NOTIFY_MS: u64 = 5_000;
/// A notification with buttons waits longer for them.
const NOTIFY_BUTTONS_MS: u64 = 60_000;
const DURATION_S: (u64, u64) = (2, 600);
const TITLE_MAX: usize = 40;
const TEXT_MAX: usize = 120;
const LABEL_MAX: usize = 16;
const WAIT_MAX_S: u64 = 120;
const EVENTS_WAIT_MAX_S: u64 = 60;
const RATE_PER_S: f64 = 10.0;
const BURST: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// What was wrong ("title", "icon", …), said back as `detail`.
    BadRequest(&'static str),
    /// A browser, another Windows user, a caller that can't be told.
    Forbidden,
    /// The user said no to this app.
    Denied,
    NotFound,
    Method,
    LengthRequired,
    TooLarge,
    MediaType,
    RateLimited,
    /// Another app's prompt is on screen.
    PermissionBusy,
    /// This exe's last prompt was denied or ignored a moment ago.
    PermissionCooldown,
    /// The Now Bar is switched off in its settings.
    Disabled,
}

impl Reject {
    pub fn status(self) -> u16 {
        match self {
            Reject::BadRequest(_) => 400,
            Reject::Forbidden | Reject::Denied => 403,
            Reject::NotFound => 404,
            Reject::Method => 405,
            Reject::LengthRequired => 411,
            Reject::TooLarge => 413,
            Reject::MediaType => 415,
            Reject::RateLimited | Reject::PermissionBusy | Reject::PermissionCooldown => 429,
            Reject::Disabled => 503,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Reject::BadRequest(_) => "bad_request",
            Reject::Forbidden => "forbidden",
            Reject::Denied => "denied",
            Reject::NotFound => "not_found",
            Reject::Method => "method_not_allowed",
            Reject::LengthRequired => "length_required",
            Reject::TooLarge => "too_large",
            Reject::MediaType => "unsupported_media_type",
            Reject::RateLimited => "rate_limited",
            Reject::PermissionBusy => "permission_busy",
            Reject::PermissionCooldown => "permission_cooldown",
            Reject::Disabled => "disabled",
        }
    }

    pub fn retry_after(self) -> Option<u32> {
        match self {
            Reject::RateLimited => Some(1),
            Reject::PermissionBusy => Some(10),
            Reject::PermissionCooldown => Some((COOLDOWN_MS / 1000) as u32),
            _ => None,
        }
    }
}

/// A request's line and headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// How the body comes: by its length, in chunks (.NET's JsonContent), or not at all.
    pub fn body(&self) -> Result<Body, Reject> {
        if let Some(te) = self.header("Transfer-Encoding") {
            return if te.eq_ignore_ascii_case("chunked") { Ok(Body::Chunked) } else { Err(Reject::LengthRequired) };
        }
        match self.header("Content-Length") {
            None => Ok(Body::None),
            Some(v) => match v.parse::<usize>() {
                Ok(n) if n > MAX_BODY => Err(Reject::TooLarge),
                Ok(n) => Ok(Body::Length(n)),
                Err(_) => Err(Reject::BadRequest("content-length")),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    None,
    Length(usize),
    Chunked,
}

/// A chunked body put together; `None` while more is to come.
pub fn dechunk(raw: &[u8]) -> Result<Option<Vec<u8>>, Reject> {
    let bad = Reject::BadRequest("chunked");
    let line_end = |from: usize| raw[from..].windows(2).position(|w| w == b"\r\n").map(|i| from + i);
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let Some(end) = line_end(i) else { return Ok(None) };
        let size = std::str::from_utf8(&raw[i..end]).ok().and_then(|l| l.split(';').next()).map(str::trim).ok_or(bad)?;
        let size = usize::from_str_radix(size, 16).map_err(|_| bad)?;
        i = end + 2;
        if size == 0 {
            // Then trailers, if any, and a blank line.
            let done = raw[i..].starts_with(b"\r\n") || raw[i..].windows(4).any(|w| w == b"\r\n\r\n");
            return Ok(done.then_some(out));
        }
        if out.len() + size > MAX_BODY {
            return Err(Reject::TooLarge);
        }
        if raw.len() < i + size + 2 {
            return Ok(None);
        }
        if &raw[i + size..i + size + 2] != b"\r\n" {
            return Err(bad);
        }
        out.extend_from_slice(&raw[i..i + size]);
        i += size + 2;
    }
}

/// Where the head ends (just past its blank line), once it has.
pub fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

pub fn parse_head(bytes: &[u8]) -> Result<Head, Reject> {
    if bytes.len() > MAX_HEAD {
        return Err(Reject::TooLarge);
    }
    let bad = Reject::BadRequest("head");
    let text = std::str::from_utf8(bytes).map_err(|_| bad)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(path), Some(version), None) = (first.next(), first.next(), first.next(), first.next()) else { return Err(bad) };
    if method.is_empty() || !path.starts_with('/') || !version.starts_with("HTTP/1.") {
        return Err(bad);
    }
    let mut headers = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (k, v) = line.split_once(':').ok_or(bad)?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(Head { method: method.to_string(), path: path.to_string(), headers })
}

/// Turns away what a web page could send: browsers say where a request comes from (`Origin`,
/// `Sec-Fetch-Site` and `-Dest`, even for an `<img>`), can't send JSON without a preflight that
/// never gets CORS headers here, and a page on a rebinding name has its own `Host`. Node's own
/// fetch sends `Sec-Fetch-Mode` alone: an app, let through.
pub fn screen(head: &Head, port: u16) -> Result<(), Reject> {
    if ["Origin", "Sec-Fetch-Site", "Sec-Fetch-Dest"].iter().any(|h| head.header(h).is_some()) {
        return Err(Reject::Forbidden);
    }
    let host = head.header("Host").ok_or(Reject::Forbidden)?.to_ascii_lowercase();
    if ![format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")].contains(&host) {
        return Err(Reject::Forbidden);
    }
    if matches!(head.method.as_str(), "POST" | "PUT") {
        let ty = head.header("Content-Type").unwrap_or("").split(';').next().unwrap_or("").trim();
        if !ty.eq_ignore_ascii_case("application/json") {
            return Err(Reject::MediaType);
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Info,
    Notify,
    Activity(String),
    Events,
}

pub fn route(method: &str, path: &str) -> Result<Route, Reject> {
    let path = path.split('?').next().unwrap_or("");
    if let Some(id) = path.strip_prefix("/v1/activities/") {
        if method != "PUT" {
            return Err(Reject::Method);
        }
        return if id_ok(id) { Ok(Route::Activity(id.to_string())) } else { Err(Reject::BadRequest("id")) };
    }
    let (wants, r) = match path.trim_end_matches('/') {
        "/v1" => ("GET", Route::Info),
        "/v1/notify" => ("POST", Route::Notify),
        "/v1/events" => ("POST", Route::Events),
        _ => return Err(Reject::NotFound),
    };
    if method == wants { Ok(r) } else { Err(Reject::Method) }
}

/// An app's own id for an item: 1–40 of letters, digits, `.`, `_`, `-`.
fn id_ok(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn button_id_ok(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSpec {
    /// The caller exe's own icon.
    Exe,
    Glyph(char),
    /// A PNG or JPEG, not yet decoded (its size is checked).
    Image(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub uri: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Notify { life_ms: u64 },
    Activity { state: LiveState, progress: Option<u8> },
}

/// A notification or an activity as an app sent it, checked and cut to size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppRequest {
    pub app: String,
    pub id: Option<String>,
    pub title: String,
    pub text: String,
    pub icon: IconSpec,
    pub accent: Option<(u8, u8, u8)>,
    pub buttons: Vec<Button>,
    /// Seconds the response waits for a click (0: it doesn't).
    pub wait_s: u32,
    pub kind: Kind,
}

pub fn parse_notify(body: &[u8]) -> Result<AppRequest, Reject> {
    let m = object(body)?;
    let id = opt_str(&m, "id")?.map(|id| if id_ok(id) { Ok(id.to_string()) } else { Err(Reject::BadRequest("id")) }).transpose()?;
    let mut r = request(&m, id, true)?;
    let fallback = if r.buttons.is_empty() { NOTIFY_MS } else { NOTIFY_BUTTONS_MS };
    let life_ms = opt_u64(&m, "duration")?.map_or(fallback, |s| s.clamp(DURATION_S.0, DURATION_S.1) * 1000);
    r.kind = Kind::Notify { life_ms };
    Ok(r)
}

pub fn parse_activity(id: &str, body: &[u8]) -> Result<AppRequest, Reject> {
    let m = object(body)?;
    let state = match opt_str(&m, "state")? {
        None => LiveState::Run,
        Some(s) => serde_json::from_value(Value::String(s.to_string())).map_err(|_| Reject::BadRequest("state"))?,
    };
    let progress = opt_u64(&m, "progress")?.map(|p| p.min(100) as u8);
    let mut r = request(&m, Some(id.to_string()), state != LiveState::Clear)?;
    r.kind = Kind::Activity { state, progress };
    Ok(r)
}

/// `/v1/events`: the app, and how long to wait for something to happen.
pub fn parse_events(body: &[u8]) -> Result<(String, u32), Reject> {
    let m = object(body)?;
    let app = app_of(&m)?;
    let wait = opt_u64(&m, "wait")?.unwrap_or(0).min(EVENTS_WAIT_MAX_S) as u32;
    Ok((app, wait))
}

fn object(body: &[u8]) -> Result<Map<String, Value>, Reject> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => Ok(m),
        _ => Err(Reject::BadRequest("json")),
    }
}

fn opt_str<'a>(m: &'a Map<String, Value>, key: &'static str) -> Result<Option<&'a str>, Reject> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(Reject::BadRequest(key)),
    }
}

fn opt_u64(m: &Map<String, Value>, key: &'static str) -> Result<Option<u64>, Reject> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or(Reject::BadRequest(key)),
    }
}

fn app_of(m: &Map<String, Value>) -> Result<String, Reject> {
    opt_str(m, "app")?.map(str::trim).filter(|a| app_name_ok(a)).map(str::to_string).ok_or(Reject::BadRequest("app"))
}

/// What both kinds share; `kind` is the caller's to set.
fn request(m: &Map<String, Value>, id: Option<String>, title_needed: bool) -> Result<AppRequest, Reject> {
    let app = app_of(m)?;
    let title = opt_str(m, "title")?.unwrap_or("").trim();
    if title.is_empty() && title_needed {
        return Err(Reject::BadRequest("title"));
    }
    let text = opt_str(m, "text")?.unwrap_or("").trim();
    let icon = parse_icon(opt_str(m, "icon")?)?;
    let accent = opt_str(m, "accent")?.map(|a| crate::settings::parse_hex(a.trim()).ok_or(Reject::BadRequest("accent"))).transpose()?;
    let buttons = parse_buttons(m.get("buttons"))?;
    let wait_s = opt_u64(m, "wait")?.unwrap_or(0).min(WAIT_MAX_S) as u32;
    Ok(AppRequest { app, id, title: cut(title, TITLE_MAX), text: cut(text, TEXT_MAX), icon, accent, buttons, wait_s, kind: Kind::Notify { life_ms: NOTIFY_MS } })
}

fn parse_buttons(v: Option<&Value>) -> Result<Vec<Button>, Reject> {
    let bad = Reject::BadRequest("buttons");
    let list = match v {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) if list.len() <= 2 => list,
        Some(_) => return Err(bad),
    };
    let mut out: Vec<Button> = Vec::new();
    for b in list {
        let Value::Object(b) = b else { return Err(bad) };
        let id = b.get("id").and_then(Value::as_str).filter(|id| button_id_ok(id)).ok_or(bad)?;
        let label = b.get("label").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()).ok_or(bad)?;
        let uri = match b.get("uri") {
            None | Some(Value::Null) => None,
            Some(Value::String(u)) if uri_allowed(u.trim()) => Some(u.trim().to_string()),
            Some(_) => return Err(Reject::BadRequest("uri")),
        };
        if out.iter().any(|o| o.id == id) {
            return Err(bad);
        }
        out.push(Button { id: id.to_string(), label: cut(label, LABEL_MAX), uri });
    }
    Ok(out)
}

fn parse_icon(v: Option<&str>) -> Result<IconSpec, Reject> {
    let bad = Reject::BadRequest("icon");
    let Some(v) = v.map(str::trim) else { return Ok(IconSpec::Exe) };
    if let Some(hex) = v.strip_prefix("glyph:") {
        let code = u32::from_str_radix(hex, 16).map_err(|_| bad)?;
        // The private use area, where Segoe Fluent Icons and MDL2 keep their symbols.
        return match char::from_u32(code) {
            Some(c) if (0xE000..=0xF8FF).contains(&code) => Ok(IconSpec::Glyph(c)),
            _ => Err(bad),
        };
    }
    let data = v.strip_prefix("data:image/png;base64,").or_else(|| v.strip_prefix("data:image/jpeg;base64,")).ok_or(bad)?;
    if data.len() > MAX_ICON_BYTES / 3 * 4 + 4 {
        return Err(Reject::TooLarge);
    }
    let bytes = base64(data).ok_or(bad)?;
    if bytes.len() > MAX_ICON_BYTES {
        return Err(Reject::TooLarge);
    }
    match image_size(&bytes) {
        Some((w, h)) if w == 0 || h == 0 => Err(bad),
        Some((w, h)) if w > MAX_ICON_PX || h > MAX_ICON_PX => Err(Reject::TooLarge),
        Some(_) => Ok(IconSpec::Image(bytes)),
        None => Err(bad),
    }
}

/// Standard or URL-safe base64, padding optional, line breaks skipped.
pub(crate) fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A PNG's or a JPEG's size from its header, before anything decodes it.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        if b.len() < 24 || &b[12..16] != b"IHDR" {
            return None;
        }
        let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        return Some((be(16), be(20)));
    }
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // The start of frame markers (C0–CF but DHT, JPG and DAC) hold the size.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if i + 9 > b.len() {
                return None;
            }
            let (h, w) = (u16::from_be_bytes([b[i + 5], b[i + 6]]), u16::from_be_bytes([b[i + 7], b[i + 8]]));
            return Some((w as u32, h as u32));
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

/// An app's name as people see it; names that would pass for Windows, the Now Bar or
/// Claude (letters and digits only, any case) aren't anyone's to take.
pub fn app_name_ok(name: &str) -> bool {
    let count = name.chars().count();
    if !(1..=32).contains(&count) || name.trim().is_empty() || name.chars().any(misleads) {
        return false;
    }
    let plain: String = name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    !["nowbar", "windows", "microsoft", "defender", "claude", "anthropic", "samsung"].iter().any(|w| plain.contains(w))
}

/// A character that could make the card's "name · exe" say something else: control and
/// invisible ones (zero-width, direction marks, soft hyphens), spaces other than a plain one,
/// and the card's own separators.
fn misleads(c: char) -> bool {
    c.is_control()
        || (c.is_whitespace() && c != ' ')
        || matches!(c, '·' | '•' | '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}')
}

/// Whether a button may open `uri`: the web, mail, or an app's own protocol; never one of
/// Windows' own handlers that open files, run things or search.
pub fn uri_allowed(uri: &str) -> bool {
    let Some((scheme, _)) = uri.split_once(':') else { return false };
    let scheme = scheme.to_ascii_lowercase();
    // One letter is a drive ("C:\").
    let well_formed = scheme.len() >= 2 && scheme.starts_with(|c: char| c.is_ascii_lowercase()) && scheme.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'));
    const NEVER: [&str; 17] = ["file", "shell", "javascript", "vbscript", "data", "about", "res", "hcp", "mk", "its", "ms-its", "mhtml", "jar", "search", "search-ms", "ldap", "telnet"];
    well_formed && !NEVER.contains(&scheme.as_str()) && !scheme.starts_with("ms-")
}

/// Who sent a request: the name it gave and the exe it ran from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Caller {
    pub app: String,
    /// The full path (from the process behind the connection, not from the request).
    pub exe: String,
}

impl Caller {
    pub fn key(&self) -> String {
        format!("{}|{}", self.exe.to_lowercase(), self.app)
    }

    pub fn exe_file(&self) -> &str {
        self.exe.rsplit(['\\', '/']).next().unwrap_or(&self.exe)
    }

    /// 64-bit FNV-1a of the key: too wide to make up a name that lands on another app's.
    pub fn owner(&self) -> String {
        let hash = self.key().bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01B3));
        format!("{hash:016x}")
    }

    fn same_exe(&self, exe: &str) -> bool {
        self.exe.to_lowercase() == exe.to_lowercase()
    }
}

/// The live set's id for an app's item: its own, inside the app's.
pub fn item_id(c: &Caller, id: &str) -> String {
    format!("app:{}:{id}", c.owner())
}

/// The live set's id for an app's prompt.
pub fn prompt_id(c: &Caller) -> String {
    format!("ask:{}", c.owner())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Allowed,
    Denied,
    /// Ask the user (or this app's prompt is already asking).
    Ask,
    /// Another app's prompt is on screen.
    Busy,
    Cooldown,
}

/// What to do with `c`'s request: the user's answer if there is one, then the prompt on screen,
/// then the exe's cooldown (`cooldown_until`, GetTickCount64 ms).
pub fn access(c: &Caller, apps: &[ApiApp], prompt: Option<&Caller>, cooldown_until: Option<u64>, now_ms: u64) -> Access {
    if let Some(a) = apps.iter().find(|a| a.name == c.app && c.same_exe(&a.exe)) {
        return if a.allowed { Access::Allowed } else { Access::Denied };
    }
    match prompt {
        Some(p) if p.key() == c.key() => Access::Ask,
        Some(_) => Access::Busy,
        None if cooldown_until.is_some_and(|until| now_ms < until) => Access::Cooldown,
        None => Access::Ask,
    }
}

/// Ten requests a second per app, in bursts of up to twenty.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: HashMap<String, (f64, u64)>,
}

impl RateLimiter {
    pub fn take(&mut self, key: &str, now_ms: u64) -> bool {
        if self.buckets.len() > 256 {
            self.buckets.retain(|_, (_, at)| now_ms.saturating_sub(*at) < 60_000);
        }
        let (tokens, at) = self.buckets.entry(key.to_string()).or_insert((BURST, now_ms));
        *tokens = (*tokens + now_ms.saturating_sub(*at) as f64 * RATE_PER_S / 1000.0).min(BURST);
        *at = now_ms;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    Button(String),
    Dismissed,
}

/// Something the user did to an app's item: `id` is the app's own id for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub id: String,
    pub kind: EventKind,
    pub at_ms: u64,
}

/// What no `wait` took, per app, until `/v1/events` asks or ten minutes pass.
#[derive(Debug, Default)]
pub struct Events {
    queues: HashMap<String, VecDeque<Event>>,
}

impl Events {
    pub fn push(&mut self, key: &str, e: Event) {
        if self.queues.len() > 64 {
            let now = e.at_ms;
            self.queues.retain(|_, q| q.back().is_some_and(|last| fresh(last, now)));
        }
        let q = self.queues.entry(key.to_string()).or_default();
        if q.len() >= EVENTS_MAX {
            q.pop_front();
        }
        q.push_back(e);
    }

    pub fn take(&mut self, key: &str, now_ms: u64) -> Vec<Event> {
        self.queues.remove(key).map(|q| q.into_iter().filter(|e| fresh(e, now_ms)).collect()).unwrap_or_default()
    }

    pub fn has(&self, key: &str, now_ms: u64) -> bool {
        self.queues.get(key).is_some_and(|q| q.iter().any(|e| fresh(e, now_ms)))
    }
}

fn fresh(e: &Event, now_ms: u64) -> bool {
    now_ms.saturating_sub(e.at_ms) <= EVENT_TTL_MS
}

fn event_value(e: &Event) -> Value {
    match &e.kind {
        EventKind::Button(b) => json!({"type": "button", "id": e.id, "button": b}),
        EventKind::Dismissed => json!({"type": "dismissed", "id": e.id}),
    }
}

/// `event`: none without a `wait`; `Some(None)` when the wait ran out.
pub fn ok_json(id: &str, event: Option<Option<&Event>>) -> String {
    let mut v = json!({"ok": true, "id": id});
    if let Some(e) = event {
        v["event"] = e.map_or(Value::Null, event_value);
    }
    v.to_string()
}

pub fn events_json(events: &[Event]) -> String {
    json!({"ok": true, "events": events.iter().map(event_value).collect::<Vec<_>>()}).to_string()
}

pub fn reject_json(r: Reject, detail: Option<&str>) -> String {
    let mut v = json!({"ok": false, "error": r.code()});
    let detail = detail.or(match r {
        Reject::BadRequest(d) => Some(d),
        _ => None,
    });
    if let Some(d) = detail {
        v["detail"] = d.into();
    }
    v.to_string()
}

/// The user hasn't answered the app's prompt yet.
pub fn pending_json() -> String {
    json!({"ok": false, "pending": true}).to_string()
}

pub fn info_json(version: &str) -> String {
    json!({"name": "Now Bar", "api": API_VERSION, "version": version}).to_string()
}

/// `%LOCALAPPDATA%\NowBar\api.json`, while the Now Bar listens.
pub fn discovery_json(port: u16, pid: u32, version: &str) -> String {
    json!({"name": "Now Bar", "api": API_VERSION, "version": version, "url": format!("http://127.0.0.1:{port}"), "port": port, "pid": pid}).to_string()
}

pub fn response(status: u16, body: &str, retry_after: Option<u32>) -> Vec<u8> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let retry = retry_after.map(|s| format!("Retry-After: {s}\r\n")).unwrap_or_default();
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{retry}Connection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activities::live::LiveState;
    use crate::settings::ApiApp;

    fn head(text: &str) -> Head {
        parse_head(text.as_bytes()).unwrap()
    }

    fn notify(json: &str) -> Result<AppRequest, Reject> {
        parse_notify(json.as_bytes())
    }

    /// A PNG's signature and IHDR, enough for `image_size`.
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend_from_slice(&w.to_be_bytes());
        png.extend_from_slice(&h.to_be_bytes());
        png
    }

    fn b64(bytes: &[u8]) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
            for i in 0..4 {
                out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        out
    }

    #[test]
    fn head_parses_and_finds_headers_case_insensitively() {
        let raw = "PUT /v1/activities/yedek HTTP/1.1\r\nHost: 127.0.0.1:20034\r\ncontent-type: application/json\r\nContent-Length: 12\r\n\r\n";
        assert_eq!(head_end(raw.as_bytes()), Some(raw.len()));
        assert_eq!(head_end(format!("{raw}{{\"a\":1}}").as_bytes()), Some(raw.len()), "the body follows");
        let h = head(raw);
        assert_eq!((h.method.as_str(), h.path.as_str()), ("PUT", "/v1/activities/yedek"));
        assert_eq!(h.header("Content-Type"), Some("application/json"));
        assert_eq!(h.body(), Ok(Body::Length(12)));
        assert_eq!(head("GET /v1 HTTP/1.1\r\nHost: localhost:20034\r\n\r\n").body(), Ok(Body::None));
        // .NET's JsonContent (PostAsJsonAsync) doesn't say its length.
        assert_eq!(head("POST /v1/notify HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").body(), Ok(Body::Chunked));
    }

    #[test]
    fn chunked_bodies_are_put_together() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), Ok(Some(b"Wikipedia".to_vec())));
        assert_eq!(dechunk(b"4;ext=1\r\nWiki\r\n0\r\nTrailer: x\r\n\r\n"), Ok(Some(b"Wiki".to_vec())), "extensions and trailers");
        assert_eq!(dechunk(b"4\r\nWi"), Ok(None), "more to come");
        assert_eq!(dechunk(b"4\r\nWiki\r\n0\r\n"), Ok(None), "the last line yet to come");
        assert_eq!(dechunk(b"zz\r\nWiki\r\n"), Err(Reject::BadRequest("chunked")));
        assert_eq!(dechunk(b"4\r\nWikiXX0\r\n\r\n"), Err(Reject::BadRequest("chunked")));
        assert_eq!(dechunk(format!("{:x}\r\n", MAX_BODY + 1).as_bytes()), Err(Reject::TooLarge));
    }

    #[test]
    fn head_rejects_garbage_and_oversize() {
        assert_eq!(parse_head(b"hello\r\n\r\n"), Err(Reject::BadRequest("head")));
        assert_eq!(parse_head(b"GET /v1 HTTP/1.1\r\nNoColon\r\n\r\n"), Err(Reject::BadRequest("head")));
        assert_eq!(parse_head(b"GET /v1 SPDY/3\r\n\r\n"), Err(Reject::BadRequest("head")));
        assert_eq!(head("POST /v1/notify HTTP/1.1\r\nContent-Length: lots\r\n\r\n").body(), Err(Reject::BadRequest("content-length")));
        assert_eq!(head("POST /v1/notify HTTP/1.1\r\nContent-Length: 999999\r\n\r\n").body(), Err(Reject::TooLarge));
        assert_eq!(head("POST /v1/notify HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n").body(), Err(Reject::LengthRequired));
        let big = format!("GET /v1 HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert_eq!(parse_head(big.as_bytes()), Err(Reject::TooLarge));
        assert_eq!(head_end(b"GET /v1 HTTP/1.1\r\nHost: x"), None);
    }

    #[test]
    fn screen_refuses_browsers() {
        let with = |extra: &str| head(&format!("POST /v1/notify HTTP/1.1\r\nHost: 127.0.0.1:20034\r\nContent-Type: application/json\r\n{extra}\r\n"));
        assert_eq!(screen(&with(""), 20034), Ok(()));
        assert_eq!(screen(&with("Origin: https://ornek.test\r\n"), 20034), Err(Reject::Forbidden));
        assert_eq!(screen(&with("Origin: null\r\n"), 20034), Err(Reject::Forbidden));
        assert_eq!(screen(&with("Sec-Fetch-Site: cross-site\r\n"), 20034), Err(Reject::Forbidden));
        assert_eq!(screen(&with("sec-fetch-dest: image\r\n"), 20034), Err(Reject::Forbidden));
        // Node's own fetch (v25, as it sent them): a mode, but no site, dest or origin.
        let node = "connection: keep-alive\r\naccept: */*\r\naccept-language: *\r\nsec-fetch-mode: cors\r\nuser-agent: node\r\naccept-encoding: gzip, deflate\r\ncontent-length: 2\r\n";
        assert_eq!(screen(&with(node), 20034), Ok(()), "an app on Node isn't a browser");
        let host = |h: &str| screen(&head(&format!("GET /v1 HTTP/1.1\r\nHost: {h}\r\n\r\n")), 20034);
        for ok in ["127.0.0.1:20034", "localhost:20034", "LOCALHOST:20034", "[::1]:20034"] {
            assert_eq!(host(ok), Ok(()), "{ok}");
        }
        for bad in ["evil.test:20034", "127.0.0.1:80", "127.0.0.1", "0.0.0.0:20034", "localhost.evil.test:20034"] {
            assert_eq!(host(bad), Err(Reject::Forbidden), "{bad}");
        }
        assert_eq!(screen(&head("GET /v1 HTTP/1.1\r\n\r\n"), 20034), Err(Reject::Forbidden), "no Host");
        let typed = |t: &str| screen(&head(&format!("POST /v1/notify HTTP/1.1\r\nHost: 127.0.0.1:20034\r\nContent-Type: {t}\r\n\r\n")), 20034);
        assert_eq!(typed("text/plain"), Err(Reject::MediaType));
        assert_eq!(typed("application/x-www-form-urlencoded"), Err(Reject::MediaType));
        assert_eq!(typed("multipart/form-data; boundary=x"), Err(Reject::MediaType));
        assert_eq!(typed("application/json; charset=utf-8"), Ok(()));
        assert_eq!(typed("Application/JSON"), Ok(()));
        assert_eq!(screen(&head("POST /v1/notify HTTP/1.1\r\nHost: 127.0.0.1:20034\r\n\r\n"), 20034), Err(Reject::MediaType), "no type");
    }

    #[test]
    fn routes_and_methods() {
        assert_eq!(route("GET", "/v1"), Ok(Route::Info));
        assert_eq!(route("GET", "/v1/"), Ok(Route::Info));
        assert_eq!(route("POST", "/v1/notify"), Ok(Route::Notify));
        assert_eq!(route("POST", "/v1/notify?x=1"), Ok(Route::Notify), "a query is ignored");
        assert_eq!(route("PUT", "/v1/activities/yedek"), Ok(Route::Activity("yedek".into())));
        assert_eq!(route("POST", "/v1/events"), Ok(Route::Events));
        assert_eq!(route("OPTIONS", "/v1/notify"), Err(Reject::Method));
        assert_eq!(route("GET", "/v1/notify"), Err(Reject::Method));
        assert_eq!(route("POST", "/v1"), Err(Reject::Method));
        assert_eq!(route("DELETE", "/v1/activities/yedek"), Err(Reject::Method));
        assert_eq!(route("GET", "/v2"), Err(Reject::NotFound));
        assert_eq!(route("GET", "/"), Err(Reject::NotFound));
        assert_eq!(route("PUT", "/v1/activities/"), Err(Reject::BadRequest("id")));
        assert_eq!(route("PUT", "/v1/activities/a%20b"), Err(Reject::BadRequest("id")));
        assert_eq!(route("PUT", &format!("/v1/activities/{}", "x".repeat(41))), Err(Reject::BadRequest("id")));
    }

    #[test]
    fn notify_parses_with_defaults() {
        let r = notify(r#"{"app":"Ekin","title":"Yedek bitti"}"#).unwrap();
        assert_eq!((r.app.as_str(), r.title.as_str(), r.text.as_str(), r.id.as_deref(), r.wait_s), ("Ekin", "Yedek bitti", "", None, 0));
        assert_eq!(r.kind, Kind::Notify { life_ms: 5_000 });
        assert_eq!((r.icon, r.accent, r.buttons.len()), (IconSpec::Exe, None, 0));
        let life = |extra: &str| match notify(&format!(r#"{{"app":"Ekin","title":"t"{extra}}}"#)).unwrap().kind {
            Kind::Notify { life_ms } => life_ms,
            k => panic!("{k:?}"),
        };
        assert_eq!(life(r#","buttons":[{"id":"ac","label":"Aç"}]"#), 60_000, "with buttons");
        assert_eq!(life(r#","duration":1"#), 2_000);
        assert_eq!(life(r#","duration":9999"#), 600_000);
        assert_eq!(life(r#","duration":12"#), 12_000);
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","id":"sohbet-1"}"#).unwrap().id.as_deref(), Some("sohbet-1"));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","whatever":[1,2]}"#).map(|r| r.title), Ok("t".into()), "unknown fields are ignored");
    }

    #[test]
    fn activity_reads_state_and_progress() {
        let a = parse_activity("yedek", br#"{"app":"Ekin","title":"Yedekleme","progress":140,"state":"wait"}"#).unwrap();
        assert_eq!(a.id.as_deref(), Some("yedek"));
        assert_eq!(a.kind, Kind::Activity { state: LiveState::Wait, progress: Some(100) });
        let plain = parse_activity("yedek", br#"{"app":"Ekin","title":"t"}"#).unwrap();
        assert_eq!(plain.kind, Kind::Activity { state: LiveState::Run, progress: None });
        assert_eq!(parse_activity("yedek", br#"{"app":"Ekin","title":"t","state":"nap"}"#), Err(Reject::BadRequest("state")));
        assert_eq!(parse_activity("yedek", br#"{"app":"Ekin","title":"t","progress":-3}"#), Err(Reject::BadRequest("progress")));
        let clear = parse_activity("yedek", br#"{"app":"Ekin","state":"clear"}"#).unwrap();
        assert_eq!(clear.kind, Kind::Activity { state: LiveState::Clear, progress: None }, "clear needs no title");
    }

    #[test]
    fn fields_are_cut_and_checked() {
        let long = notify(&format!(r#"{{"app":"Ekin","title":"{}","text":"{}"}}"#, "ş".repeat(50), "a".repeat(200))).unwrap();
        assert_eq!(long.title.chars().count(), 40);
        assert!(long.title.ends_with('…'));
        assert_eq!(long.text.chars().count(), 120);
        assert_eq!(notify(r#"{"app":"Ekin"}"#), Err(Reject::BadRequest("title")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"  "}"#), Err(Reject::BadRequest("title")));
        assert_eq!(notify(r#"{"app":"Ekin","title":7}"#), Err(Reject::BadRequest("title")));
        assert_eq!(notify(r#"{"title":"t"}"#), Err(Reject::BadRequest("app")));
        assert_eq!(notify("not json"), Err(Reject::BadRequest("json")));
        assert_eq!(notify(r#"["a"]"#), Err(Reject::BadRequest("json")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","accent":"mavi"}"#), Err(Reject::BadRequest("accent")));
        assert_eq!(notify(r##"{"app":"Ekin","title":"t","accent":"#1DB954"}"##).unwrap().accent, Some((0x1D, 0xB9, 0x54)));
        let three = r#"{"app":"Ekin","title":"t","buttons":[{"id":"a","label":"A"},{"id":"b","label":"B"},{"id":"c","label":"C"}]}"#;
        assert_eq!(notify(three), Err(Reject::BadRequest("buttons")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","buttons":[{"id":"a b","label":"A"}]}"#), Err(Reject::BadRequest("buttons")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","buttons":[{"id":"a","label":" "}]}"#), Err(Reject::BadRequest("buttons")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","buttons":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}"#), Err(Reject::BadRequest("buttons")), "the same id twice");
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","buttons":[{"id":"a","label":"A","uri":"file:///C:/x.exe"}]}"#), Err(Reject::BadRequest("uri")));
        let label = notify(r#"{"app":"Ekin","title":"t","buttons":[{"id":"a","label":"Çok uzun bir düğme adı","uri":"https://ornek.test"}]}"#).unwrap();
        assert_eq!(label.buttons[0].label.chars().count(), 16);
        assert_eq!(label.buttons[0].uri.as_deref(), Some("https://ornek.test"));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","wait":500}"#).unwrap().wait_s, 120);
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","wait":"x"}"#), Err(Reject::BadRequest("wait")));
        assert_eq!(notify(r#"{"app":"Ekin","title":"t","id":"a/b"}"#), Err(Reject::BadRequest("id")));
    }

    #[test]
    fn icons() {
        let icon = |v: &str| notify(&format!(r#"{{"app":"Ekin","title":"t","icon":{v}}}"#)).map(|r| r.icon);
        assert_eq!(icon(r#""glyph:E8B7""#), Ok(IconSpec::Glyph('\u{E8B7}')));
        assert_eq!(icon(r#""glyph:e8b7""#), Ok(IconSpec::Glyph('\u{E8B7}')));
        assert_eq!(icon(r#""glyph:0041""#), Err(Reject::BadRequest("icon")), "not a symbol font's code point");
        assert_eq!(icon(r#""glyph:ZZZZ""#), Err(Reject::BadRequest("icon")));
        let png = png_header(64, 64);
        assert_eq!(icon(&format!(r#""data:image/png;base64,{}""#, b64(&png))), Ok(IconSpec::Image(png)));
        assert_eq!(icon(&format!(r#""data:image/png;base64,{}""#, b64(&png_header(600, 64)))), Err(Reject::TooLarge), "over 512 px");
        assert_eq!(icon(r#""data:image/gif;base64,R0lG""#), Err(Reject::BadRequest("icon")));
        assert_eq!(icon(r#""data:image/png;base64,***""#), Err(Reject::BadRequest("icon")));
        assert_eq!(icon(&format!(r#""data:image/png;base64,{}""#, b64(b"not an image"))), Err(Reject::BadRequest("icon")));
        assert_eq!(icon(r#""https://ornek.test/a.png""#), Err(Reject::BadRequest("icon")));
        let big = format!(r#""data:image/png;base64,{}""#, "A".repeat(MAX_ICON_BYTES / 3 * 4 + 400));
        assert_eq!(icon(&big), Err(Reject::TooLarge));
        assert_eq!(icon("null"), Ok(IconSpec::Exe));
    }

    #[test]
    fn events_body_reads_app_and_wait() {
        assert_eq!(parse_events(br#"{"app":"Ekin","wait":30}"#), Ok(("Ekin".into(), 30)));
        assert_eq!(parse_events(br#"{"app":"Ekin"}"#), Ok(("Ekin".into(), 0)));
        assert_eq!(parse_events(br#"{"app":"Ekin","wait":600}"#), Ok(("Ekin".into(), 60)));
        assert_eq!(parse_events(br#"{"wait":3}"#), Err(Reject::BadRequest("app")));
    }

    #[test]
    fn app_names() {
        for ok in ["Ekin", "Yedekleyici", "Ekin's Tool 2", "Çalışma Saati", &"a".repeat(32)] {
            assert!(app_name_ok(ok), "{ok}");
        }
        for bad in ["", "   ", &"a".repeat(33), "Windows Update", "now-bar", "Now Bar", "CLAUDE", "Microsoft Defender", "Sam sung", "Anthropic", "a\u{7}b"] {
            assert!(!app_name_ok(bad), "{bad}");
        }
        // What could make the card's "name · exe" lie: invisible marks, turned-around text, its own dot.
        for bad in ["Ekin\u{200B}", "Ek\u{202E}exe.nik", "Ekin · ekin.exe", "Ekin • x", "Ekin\u{00A0}Uygulama", "\u{FEFF}Ekin", "Ekin\u{2066}x", "Ek\u{00AD}in"] {
            assert!(!app_name_ok(bad), "{bad:?}");
        }
        assert_eq!(notify(r#"{"app":"Windows Güvenlik","title":"t"}"#), Err(Reject::BadRequest("app")));
        assert_eq!(notify(r#"{"app":"  Ekin  ","title":"t"}"#).map(|r| r.app), Ok("Ekin".into()), "trimmed");
    }

    #[test]
    fn uris() {
        for ok in ["https://ornek.test/a", "http://127.0.0.1:8080/", "mailto:ekin@ornek.test", "ekin-app://sohbet/3", "ekin.app:open"] {
            assert!(uri_allowed(ok), "{ok}");
        }
        for bad in [
            "file:///C:/x.exe", "FILE:///C:/x.exe", "shell:startup", "ms-msdt:/id", "ms-settings:privacy", "search-ms:query=x", "search:query=x",
            "javascript:alert(1)", "vbscript:x", "data:text/html,x", r"C:\x.exe", r"\\sunucu\pay\x.exe", "x", "", "about:blank", "res://x", "hcp://x",
            "mk:@MSITStore:x", "its:x", "jar:x", "ldap://x", "telnet://x", "1abc:x", "a b:x",
        ] {
            assert!(!uri_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn image_size_reads_headers() {
        let png = png_header(600, 400);
        assert_eq!(image_size(&png), Some((600, 400)));
        // SOI, an APP0 segment, then SOF0: height 16, width 32.
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x10, 0x00, 0x20, 0x03];
        assert_eq!(image_size(&jpeg), Some((32, 16)));
        assert_eq!(image_size(b"not an image"), None);
        assert_eq!(image_size(&png[..18]), None);
        assert_eq!(image_size(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]), None, "cut short");
    }

    #[test]
    fn item_ids_are_scoped_per_caller() {
        let a = Caller { app: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into() };
        let b = Caller { app: "Ekin".into(), exe: r"C:\Apps\baska.exe".into() };
        assert_ne!(item_id(&a, "yedek"), item_id(&b, "yedek"));
        assert!(item_id(&a, "yedek").starts_with("app:") && item_id(&a, "yedek").ends_with(":yedek"));
        assert_eq!(item_id(&a, "yedek"), item_id(&Caller { app: "Ekin".into(), exe: r"c:\apps\EKIN.exe".into() }, "yedek"), "Windows paths ignore case");
        assert!(prompt_id(&a).starts_with("ask:"));
        assert_eq!(a.owner().len(), 16, "64 bits: no name can be made to look like another app");
        assert_eq!(a.exe_file(), "ekin.exe");
        assert!(item_id(&a, &"x".repeat(40)).chars().count() <= 64, "fits the live set's id");
    }

    #[test]
    fn access_decisions() {
        let ekin = Caller { app: "Ekin".into(), exe: r"C:\Apps\ekin.exe".into() };
        let other = Caller { app: "Deniz".into(), exe: r"C:\Apps\deniz.exe".into() };
        let saved = |allowed| vec![ApiApp { name: "Ekin".into(), exe: r"c:\apps\ekin.exe".into(), allowed }];
        assert_eq!(access(&ekin, &saved(true), None, None, 0), Access::Allowed);
        assert_eq!(access(&ekin, &saved(false), None, None, 0), Access::Denied);
        assert_eq!(access(&ekin, &saved(true), Some(&other), Some(u64::MAX), 0), Access::Allowed, "an answer beats the prompt and the cooldown");
        assert_eq!(access(&ekin, &[], None, None, 0), Access::Ask);
        assert_eq!(access(&ekin, &[], Some(&ekin), None, 0), Access::Ask, "its own prompt is open");
        assert_eq!(access(&ekin, &[], Some(&other), None, 0), Access::Busy);
        assert_eq!(access(&ekin, &[], None, Some(5_000), 4_999), Access::Cooldown);
        assert_eq!(access(&ekin, &[], None, Some(5_000), 5_000), Access::Ask, "over");
        let renamed = Caller { app: "Ekin 2".into(), ..ekin.clone() };
        assert_eq!(access(&renamed, &saved(true), None, None, 0), Access::Ask, "another name from the same exe is another app");
    }

    #[test]
    fn rate_limiter_allows_bursts_then_ten_a_second() {
        let mut r = RateLimiter::default();
        assert_eq!((0..25).filter(|_| r.take("a", 1_000)).count(), 20, "a burst of 20");
        assert!(r.take("b", 1_000), "another app has its own");
        assert!(r.take("a", 1_100), "one back after 100 ms");
        assert!(!r.take("a", 1_100));
        assert_eq!((0..30).filter(|_| r.take("a", 3_100)).count(), 20, "never more than the burst");
    }

    #[test]
    fn events_keep_order_and_expire() {
        let mut e = Events::default();
        let ev = |id: &str, at| Event { id: id.into(), kind: EventKind::Button("ac".into()), at_ms: at };
        e.push("k", ev("1", 0));
        e.push("k", Event { id: "2".into(), kind: EventKind::Dismissed, at_ms: 10 });
        assert!(e.has("k", 20));
        assert_eq!(e.take("k", 20).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1", "2"]);
        assert!(e.take("k", 20).is_empty(), "taken once");
        for n in 0..(EVENTS_MAX as u64 + 1) {
            e.push("k", ev(&n.to_string(), n));
        }
        let all = e.take("k", 100);
        assert_eq!((all.len(), all[0].id.as_str()), (EVENTS_MAX, "1"), "the oldest went");
        e.push("k", ev("eski", 0));
        assert!(!e.has("k", EVENT_TTL_MS + 1));
        assert!(e.take("k", EVENT_TTL_MS + 1).is_empty(), "ten minutes on, gone");
        assert!(e.take("baska", 0).is_empty());
    }

    #[test]
    fn responses_are_http() {
        let r = String::from_utf8(response(429, &reject_json(Reject::RateLimited, None), Reject::RateLimited.retry_after())).unwrap();
        assert!(r.starts_with("HTTP/1.1 429 Too Many Requests\r\n"));
        assert!(r.contains("\r\nRetry-After: 1\r\n") && r.contains("\r\nConnection: close\r\n") && r.contains("application/json"));
        let body = r.split("\r\n\r\n").nth(1).unwrap();
        assert!(r.contains(&format!("\r\nContent-Length: {}\r\n", body.len())));
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!((v["ok"].as_bool(), v["error"].as_str()), (Some(false), Some("rate_limited")));
        assert!(!String::from_utf8(response(200, "{}", None)).unwrap().contains("Retry-After"));
        let d: serde_json::Value = serde_json::from_str(&reject_json(Reject::BadRequest("title"), None)).unwrap();
        assert_eq!((d["error"].as_str(), d["detail"].as_str()), (Some("bad_request"), Some("title")));
        let ok: serde_json::Value = serde_json::from_str(&ok_json("yedek", None)).unwrap();
        assert_eq!((ok["ok"].as_bool(), ok["id"].as_str(), ok.get("event")), (Some(true), Some("yedek"), None));
        let timed_out: serde_json::Value = serde_json::from_str(&ok_json("yedek", Some(None))).unwrap();
        assert!(timed_out.get("event").is_some_and(|e| e.is_null()), "waited, nothing came");
        let click = Event { id: "yedek".into(), kind: EventKind::Button("ac".into()), at_ms: 5 };
        let clicked: serde_json::Value = serde_json::from_str(&ok_json("yedek", Some(Some(&click)))).unwrap();
        assert_eq!(clicked["event"], serde_json::json!({"type": "button", "id": "yedek", "button": "ac"}));
        let list: serde_json::Value = serde_json::from_str(&events_json(&[Event { id: "n1".into(), kind: EventKind::Dismissed, at_ms: 0 }])).unwrap();
        assert_eq!(list, serde_json::json!({"ok": true, "events": [{"type": "dismissed", "id": "n1"}]}));
        let pending: serde_json::Value = serde_json::from_str(&pending_json()).unwrap();
        assert_eq!(pending, serde_json::json!({"ok": false, "pending": true}));
        let info: serde_json::Value = serde_json::from_str(&info_json("0.3.0")).unwrap();
        assert_eq!(info, serde_json::json!({"name": "Now Bar", "api": 1, "version": "0.3.0"}));
        let found: serde_json::Value = serde_json::from_str(&discovery_json(20034, 4242, "0.3.0")).unwrap();
        assert_eq!(found["url"], "http://127.0.0.1:20034");
        assert_eq!((found["port"].as_u64(), found["pid"].as_u64(), found["api"].as_u64()), (Some(20034), Some(4242), Some(1)));
        for (r, status) in [(Reject::Denied, 403), (Reject::NotFound, 404), (Reject::Method, 405), (Reject::LengthRequired, 411), (Reject::TooLarge, 413), (Reject::MediaType, 415), (Reject::PermissionCooldown, 429), (Reject::Disabled, 503)] {
            assert_eq!(r.status(), status, "{r:?}");
        }
        assert_eq!((Reject::PermissionBusy.retry_after(), Reject::PermissionCooldown.retry_after(), Reject::Denied.retry_after()), (Some(10), Some(600), None));
    }
}
