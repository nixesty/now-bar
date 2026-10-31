//! Our windows. Window procedures only queue [`UiEvent`]s; the app drains them after each
//! dispatched message, so no handler ever runs re-entrantly inside a Win32 call.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Once;

use windows::core::{h, w, Interface, Result, PCWSTR};
use windows::UI::Composition::ContainerVisual;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_HOSTBACKDROPBRUSH, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::taskbar;
use crate::render::gfx::Gfx;
use crate::render::layout::RectPx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillEvent {
    /// Left click at `x` pixels from the pill's left edge.
    Click { x: i32 },
    Wheel(i32),
    MiddleClick,
    RightClick,
    Hover(bool),
    /// The mouse went down on the pill and moved past the drag distance (no click follows).
    DragStart,
}

/// Pointer events on a card; coordinates are pixels from the card's top-left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardEvent {
    Down { x: i32, y: i32 },
    Move { x: i32, y: i32 },
    Up { x: i32, y: i32 },
    Wheel { x: i32, y: i32, delta: i32 },
    Hover(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiEvent {
    Pill(PillEvent),
    Card(CardEvent),
    Timer(usize),
    /// Another process's window came to the foreground.
    Foreground,
    /// The pill⇄card morph settled; `opened` says in which direction.
    MorphDone { opened: bool },
    /// A key pressed while the picker has focus (virtual-key code).
    Key(u16),
    /// A character typed while the question card has focus (a UTF-16 unit).
    Char(u16),
    /// The question card's shortcut.
    QuestionHotkey,
    /// Claude's answer moved on (its mailbox has it).
    Question,
    /// The tray icon was clicked.
    Tray { right: bool },
    /// Win+Alt+N.
    Hotkey,
    /// The PC woke from sleep.
    Resume,
    /// The session was unlocked.
    Unlocked,
    /// The session was locked.
    Locked,
    /// Plugged in or out, or the battery level changed.
    PowerChanged,
    /// The apps using the microphone or the camera changed.
    Privacy,
    /// The connected headsets or their levels changed.
    Headsets,
    /// A new weather answer.
    Weather,
    /// A command came from `nowbar.exe push` and the like (`ipc::take()` has it).
    Ipc,
    /// The updater found a release, installed one or failed (its mailbox says which).
    Update,
    /// A file drag came over the pill or the card, or left it (`dragdrop::hover()` says where).
    DropHover,
    /// Files were dropped on the pill or the card (`dragdrop::take_dropped()` has them).
    Dropped,
    /// The shelf worker made thumbnails.
    Shelf,
    /// A new screenshot was saved.
    Capture,
    /// The foreground window moved or resized (it may have gone fullscreen).
    ForegroundMoved,
    /// Explorer (re)created the taskbar: re-embed the pill, re-add the tray icon.
    TaskbarCreated,
    /// The taskbar's buttons or tray changed size (`taskbar::watch_items` has the new spans).
    TaskbarItems,
    /// Display, DPI or taskbar settings changed: re-read the layout.
    Environment,
    /// Windows transparency effects were switched on or off.
    EffectsChanged,
    /// The media worker has a new snapshot.
    Media,
    /// The default audio output changed.
    OutputChanged,
    /// The Buds worker has a new reading.
    Buds,
    /// The default output's volume or mute changed.
    VolumeChanged,
    /// The settings window saved new settings.
    SettingsChanged,
    /// The lyrics worker has an answer.
    Lyrics,
    /// The football worker has new match data.
    Matches,
    /// The wallpaper's accent was read again (`wallpaper::accent()` has it).
    Wallpaper,
    /// The games worker saw a download, a game running or one of them ending.
    Games,
    /// Dijital Denge's minute: the usage, and any limit crossed.
    Wellbeing,
    /// An app sent something through the API (its mailbox has it).
    Api,
    /// A shelf tool finished (its mailbox has the result).
    ShelfTool,
    /// Explorer's copy dialog came, moved on or went (its mailbox has it).
    Transfer,
}

/// Callback message of the tray icon.
pub const WM_TRAY: u32 = WM_APP + 1;
/// Posted from other threads (UISettings, media worker, audio notifications) to raise UI events.
pub const WM_EFFECTS: u32 = WM_APP + 2;
pub const WM_MEDIA: u32 = WM_APP + 3;
pub const WM_OUTPUT: u32 = WM_APP + 4;
pub const WM_BUDS: u32 = WM_APP + 5;
pub const WM_VOLUME: u32 = WM_APP + 6;
pub const WM_SETTINGS_FILE: u32 = WM_APP + 7;
pub const WM_MATCHES: u32 = WM_APP + 8;
pub const WM_LYRICS: u32 = WM_APP + 9;
pub const WM_PRIVACY: u32 = WM_APP + 10;
pub const WM_HEADSETS: u32 = WM_APP + 11;
pub const WM_WEATHER: u32 = WM_APP + 12;
pub const WM_SHELF: u32 = WM_APP + 13;
pub const WM_UPDATE: u32 = WM_APP + 14;
pub const WM_TASKBAR_ITEMS: u32 = WM_APP + 15;
pub const WM_CAPTURE: u32 = WM_APP + 16;
pub const WM_WALLPAPER: u32 = WM_APP + 17;
pub const WM_GAMES: u32 = WM_APP + 18;
pub const WM_WELLBEING: u32 = WM_APP + 19;
pub const WM_API: u32 = WM_APP + 20;
pub const WM_SHELF_TOOL: u32 = WM_APP + 21;
pub const WM_TRANSFER: u32 = WM_APP + 22;
pub const WM_QUESTION: u32 = WM_APP + 23;

static MAIN_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
static TASKBAR_CREATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static SETTINGS_CHANGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn post_main(message: u32) {
    let hwnd = HWND(MAIN_HWND.load(std::sync::atomic::Ordering::Relaxed) as *mut _);
    if !hwnd.is_invalid() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// Raise the matching `UiEvent` on the UI thread; safe to call from any thread.
pub fn post_effects_changed() {
    post_main(WM_EFFECTS);
}

pub fn post_media() {
    post_main(WM_MEDIA);
}

pub fn post_output_changed() {
    post_main(WM_OUTPUT);
}

pub fn post_buds() {
    post_main(WM_BUDS);
}

pub fn post_volume_changed() {
    post_main(WM_VOLUME);
}

pub fn post_settings_changed() {
    post_main(WM_SETTINGS_FILE);
}

pub fn post_matches() {
    post_main(WM_MATCHES);
}

pub fn post_lyrics() {
    post_main(WM_LYRICS);
}

pub fn post_privacy() {
    post_main(WM_PRIVACY);
}

pub fn post_headsets() {
    post_main(WM_HEADSETS);
}

pub fn post_weather() {
    post_main(WM_WEATHER);
}

pub fn post_update() {
    post_main(WM_UPDATE);
}

pub fn post_shelf() {
    post_main(WM_SHELF);
}

pub fn post_capture() {
    post_main(WM_CAPTURE);
}

pub fn post_taskbar_items() {
    post_main(WM_TASKBAR_ITEMS);
}

pub fn post_wallpaper() {
    post_main(WM_WALLPAPER);
}

pub fn post_games() {
    post_main(WM_GAMES);
}

pub fn post_wellbeing() {
    post_main(WM_WELLBEING);
}

pub fn post_api() {
    post_main(WM_API);
}

pub fn post_shelf_tool() {
    post_main(WM_SHELF_TOOL);
}

pub fn post_transfer() {
    post_main(WM_TRANSFER);
}

pub fn post_question() {
    post_main(WM_QUESTION);
}

thread_local! {
    static EVENTS: RefCell<VecDeque<UiEvent>> = const { RefCell::new(VecDeque::new()) };
    static HOVER_PILL: RefCell<bool> = const { RefCell::new(false) };
    static HOVER_CARD: RefCell<bool> = const { RefCell::new(false) };
    /// Where the left button went down on the pill, and whether that press became a drag.
    static PILL_PRESS: std::cell::Cell<Option<(i32, i32, bool)>> = const { std::cell::Cell::new(None) };
}

pub fn push_event(e: UiEvent) {
    let first = EVENTS.with(|q| {
        let mut q = q.borrow_mut();
        q.push_back(e);
        q.len() == 1
    });
    // Sent messages (WM_COPYDATA, broadcasts) and WinEvent callbacks run inside GetMessage, which
    // doesn't return for them: a posted no-op makes it return so the loop takes the event now.
    if first {
        post_main(WM_NULL);
    }
}

/// The queued events, a timer that fired several times (during a modal loop) only once.
pub fn take_events() -> Vec<UiEvent> {
    let all: Vec<UiEvent> = EVENTS.with(|q| q.borrow_mut().drain(..).collect());
    let mut out: Vec<UiEvent> = Vec::with_capacity(all.len());
    for e in all {
        if matches!(e, UiEvent::Timer(_)) && out.contains(&e) {
            continue;
        }
        out.push(e);
    }
    out
}

const MA_NOACTIVATE: isize = 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
const HTTRANSPARENT: isize = -1;

fn x_of(lp: LPARAM) -> i32 {
    (lp.0 & 0xFFFF) as i16 as i32
}

fn y_of(lp: LPARAM) -> i32 {
    ((lp.0 >> 16) & 0xFFFF) as i16 as i32
}

fn wheel_of(wp: WPARAM) -> i32 {
    ((wp.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Starts TME_LEAVE tracking the first time the mouse moves in; returns true on that first move.
fn track_enter(hwnd: HWND, flag: &'static std::thread::LocalKey<RefCell<bool>>) -> bool {
    if flag.with(|h| h.replace(true)) {
        return false;
    }
    let mut tme = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    unsafe {
        let _ = TrackMouseEvent(&mut tme);
    }
    true
}

/// Windows that only show composition content: never activate, never take the mouse.
extern "system" fn passive_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

extern "system" fn pill_input_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pill = |e| {
        push_event(UiEvent::Pill(e));
        LRESULT(0)
    };
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_LBUTTONDOWN => {
            PILL_PRESS.with(|p| p.set(Some((x_of(lp), y_of(lp), false))));
            unsafe { SetCapture(hwnd) };
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            // A press that became a shelf drag ends inside OLE's loop, which takes this release;
            // on any other pill a sloppy click is still a click.
            PILL_PRESS.with(|p| p.set(None));
            pill(PillEvent::Click { x: x_of(lp) })
        }
        WM_MBUTTONUP => pill(PillEvent::MiddleClick),
        WM_RBUTTONUP => pill(PillEvent::RightClick),
        // Vertical wheel, tilt wheel and touchpad two-finger swipes all move through the stack.
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => pill(PillEvent::Wheel(wheel_of(wp))),
        WM_MOUSEMOVE => {
            if track_enter(hwnd, &HOVER_PILL) {
                push_event(UiEvent::Pill(PillEvent::Hover(true)));
            }
            let held = wp.0 & windows::Win32::System::SystemServices::MK_LBUTTON.0 as usize != 0;
            if let (true, Some((x, y, false))) = (held, PILL_PRESS.with(|p| p.get())) {
                if super::dragdrop::drag_moved(x_of(lp) - x, y_of(lp) - y) {
                    PILL_PRESS.with(|p| p.set(Some((x, y, true))));
                    push_event(UiEvent::Pill(PillEvent::DragStart));
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            HOVER_PILL.with(|h| *h.borrow_mut() = false);
            pill(PillEvent::Hover(false))
        }
        WM_TIMER => {
            push_event(UiEvent::Timer(wp.0));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

extern "system" fn card_input_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let card = |e| {
        push_event(UiEvent::Card(e));
        LRESULT(0)
    };
    let (x, y) = (x_of(lp), y_of(lp));
    match msg {
        // A card that takes the keyboard (the picker, the question card) takes it back on a click
        // when another window had it; the others never activate.
        WM_MOUSEACTIVATE => {
            let focusable = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_NOACTIVATE.0 == 0;
            LRESULT(if focusable { MA_ACTIVATE as isize } else { MA_NOACTIVATE })
        }
        WM_LBUTTONDOWN => {
            unsafe { SetCapture(hwnd) };
            card(CardEvent::Down { x, y })
        }
        WM_LBUTTONUP => {
            unsafe {
                let _ = ReleaseCapture();
            }
            card(CardEvent::Up { x, y })
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            // Wheel coordinates are in screen space.
            let mut p = windows::Win32::Foundation::POINT { x, y };
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
            }
            card(CardEvent::Wheel { x: p.x, y: p.y, delta: wheel_of(wp) })
        }
        WM_MOUSEMOVE => {
            if track_enter(hwnd, &HOVER_CARD) {
                push_event(UiEvent::Card(CardEvent::Hover(true)));
            }
            card(CardEvent::Move { x, y })
        }
        WM_MOUSELEAVE => {
            HOVER_CARD.with(|h| *h.borrow_mut() = false);
            card(CardEvent::Hover(false))
        }
        WM_TIMER => {
            push_event(UiEvent::Timer(wp.0));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            push_event(UiEvent::Key(wp.0 as u16));
            LRESULT(0)
        }
        // Typing in the question card (Backspace, Enter, Esc and Ctrl+V come as characters too).
        WM_CHAR => {
            push_event(UiEvent::Char(wp.0 as u16));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const PBT_APMPOWERSTATUSCHANGE: usize = 0x0A;

/// Hidden top-level window: tray callbacks, the hotkey, power events and app-wide timers.
extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => push_event(UiEvent::Tray { right: false }),
                WM_RBUTTONUP => push_event(UiEvent::Tray { right: true }),
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            push_event(if wp.0 as i32 == super::hotkey::QUESTION_ID { UiEvent::QuestionHotkey } else { UiEvent::Hotkey });
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            if wp.0 == WTS_SESSION_UNLOCK as usize {
                push_event(UiEvent::Unlocked);
            } else if wp.0 == WTS_SESSION_LOCK as usize {
                push_event(UiEvent::Locked);
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            match wp.0 {
                PBT_APMRESUMEAUTOMATIC => push_event(UiEvent::Resume),
                PBT_APMPOWERSTATUSCHANGE => push_event(UiEvent::PowerChanged),
                _ => {}
            }
            LRESULT(1)
        }
        WM_TIMER => {
            push_event(UiEvent::Timer(wp.0));
            LRESULT(0)
        }
        // 1 tells the sender it was ours and taken.
        WM_COPYDATA => match unsafe { super::ipc::receive(lp) } {
            Some(c) => {
                let s = crate::settings::current();
                let answer = super::ipc::answer_for(&c, s.enabled && s.live_enabled && s.claude_ask, s.claude_ask_wait_s);
                super::ipc::queue(c);
                push_event(UiEvent::Ipc);
                LRESULT(answer)
            }
            None => LRESULT(0),
        },
        WM_EFFECTS | WM_MEDIA | WM_OUTPUT | WM_BUDS | WM_VOLUME | WM_SETTINGS_FILE | WM_MATCHES | WM_LYRICS | WM_PRIVACY | WM_HEADSETS | WM_WEATHER | WM_SHELF | WM_CAPTURE | WM_UPDATE
        | WM_TASKBAR_ITEMS | WM_WALLPAPER | WM_GAMES | WM_WELLBEING | WM_API | WM_SHELF_TOOL | WM_TRANSFER | WM_QUESTION => {
            push_event(match msg {
                WM_API => UiEvent::Api,
                WM_QUESTION => UiEvent::Question,
                WM_SHELF_TOOL => UiEvent::ShelfTool,
                WM_TRANSFER => UiEvent::Transfer,
                WM_WELLBEING => UiEvent::Wellbeing,
                WM_GAMES => UiEvent::Games,
                WM_WALLPAPER => UiEvent::Wallpaper,
                WM_TASKBAR_ITEMS => UiEvent::TaskbarItems,
                WM_UPDATE => UiEvent::Update,
                WM_MATCHES => UiEvent::Matches,
                WM_LYRICS => UiEvent::Lyrics,
                WM_PRIVACY => UiEvent::Privacy,
                WM_HEADSETS => UiEvent::Headsets,
                WM_WEATHER => UiEvent::Weather,
                WM_SHELF => UiEvent::Shelf,
                WM_CAPTURE => UiEvent::Capture,
                WM_EFFECTS => UiEvent::EffectsChanged,
                WM_SETTINGS_FILE => UiEvent::SettingsChanged,
                WM_MEDIA => UiEvent::Media,
                WM_BUDS => UiEvent::Buds,
                WM_VOLUME => UiEvent::VolumeChanged,
                _ => UiEvent::OutputChanged,
            });
            LRESULT(0)
        }
        WM_SETTINGCHANGE | WM_DISPLAYCHANGE | WM_DPICHANGED => {
            push_event(UiEvent::Environment);
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        m if m != 0 && m == TASKBAR_CREATED.load(std::sync::atomic::Ordering::Relaxed) => {
            push_event(UiEvent::TaskbarCreated);
            LRESULT(0)
        }
        m if m != 0 && m == SETTINGS_CHANGED.load(std::sync::atomic::Ordering::Relaxed) => {
            push_event(UiEvent::SettingsChanged);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

const PASSIVE_CLASS: PCWSTR = w!("NowBarVisual");
const MAIN_CLASS: PCWSTR = w!("NowBarMain");
const PILL_INPUT_CLASS: PCWSTR = w!("NowBarPillInput");
const CARD_INPUT_CLASS: PCWSTR = w!("NowBarCardInput");

fn register_classes() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default().into();
        let arrow = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
        let classes: [(PCWSTR, WNDPROC); 4] = [
            (PASSIVE_CLASS, Some(passive_proc)),
            (PILL_INPUT_CLASS, Some(pill_input_proc)),
            (CARD_INPUT_CLASS, Some(card_input_proc)),
            (MAIN_CLASS, Some(main_proc)),
        ];
        for (class, proc_) in classes {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: proc_,
                hInstance: instance,
                lpszClassName: class,
                hCursor: arrow,
                ..Default::default()
            });
        }
    });
}

fn create(class: PCWSTR, ex: WINDOW_EX_STYLE, r: RectPx) -> Result<HWND> {
    register_classes();
    unsafe {
        let instance = GetModuleHandleW(None)?.into();
        CreateWindowExW(ex, class, w!("Now Bar"), WS_POPUP, r.x, r.y, r.w, r.h, None, None, Some(instance), None)
    }
}

/// Rounded hit region so only the pill or card itself takes the mouse.
pub fn set_hit_region(hwnd: HWND, w: i32, h: i32, radius_px: i32) {
    unsafe {
        let d = radius_px * 2;
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, d, d);
        SetWindowRgn(hwnd, Some(rgn), true);
    }
}

/// Root visual sized to the window, so children can use relative sizes.
fn composition_target(g: &Gfx, hwnd: HWND, size: RectPx) -> Result<(DesktopWindowTarget, ContainerVisual)> {
    let target = unsafe { g.compositor.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
    let root = g.compositor.CreateContainerVisual()?;
    root.SetSize(windows_numerics::Vector2 { X: size.w as f32, Y: size.h as f32 })?;
    target.SetRoot(&root)?;
    Ok((target, root))
}

/// The pill: a click-through composition window across the taskbar, in which the pill's frame
/// (`stage`) moves, plus a pill-shaped input window, both children of the taskbar.
pub struct PillWindow {
    pub hwnd: HWND,
    pub input: HWND,
    pub target: DesktopWindowTarget,
    pub root: ContainerVisual,
    /// The pill's frame (pill plus slivers): the pill visual lives in it, centred.
    pub stage: ContainerVisual,
    origin: RectPx,
    lane: RectPx,
    /// Last placement and visibility: these calls go across processes into Explorer, so ticks
    /// that change nothing skip them.
    placed: std::cell::Cell<Option<(RectPx, i32)>>,
    stage_x: std::cell::Cell<Option<f32>>,
    visible: std::cell::Cell<Option<bool>>,
}

impl PillWindow {
    /// `origin` is the taskbar's rect the layout came from, `lane` the composition window's rect
    /// (the taskbar's width at the pill's height) and `frame_w` the pill frame's width, in screen
    /// pixels. The window never moves: the pill glides inside it, which a moving child window
    /// can't do smoothly.
    pub fn create(g: &Gfx, bar: HWND, origin: RectPx, lane: RectPx, frame_w: i32) -> Result<Self> {
        let ex = WS_EX_NOREDIRECTIONBITMAP | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        let hwnd = create(PASSIVE_CLASS, ex, lane)?;
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)? };
        taskbar::embed(hwnd, bar, origin, lane)?;
        let (target, root) = composition_target(g, hwnd, lane)?;
        let stage = g.compositor.CreateContainerVisual()?;
        stage.SetSize(windows_numerics::Vector2 { X: frame_w as f32, Y: lane.h as f32 })?;
        root.Children()?.InsertAtTop(&stage)?;

        let input = create(PILL_INPUT_CLASS, WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW, lane)?;
        taskbar::embed(input, bar, origin, lane)?;
        // Alpha 1: invisible but still hit-testable. Set after embedding; set before, a layered
        // child keeps an opaque (black) surface.
        unsafe { SetLayeredWindowAttributes(input, COLORREF(0), 1, LWA_ALPHA)? };
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = ShowWindow(input, SW_SHOWNOACTIVATE);
        }
        let (placed, stage_x, visible) = Default::default();
        Ok(PillWindow { hwnd, input, target, root, stage, origin, lane, placed, stage_x, visible })
    }

    /// Puts the input window on `pill` and centres the pill's frame (`frame_w` wide) on it; with
    /// `glide` the frame springs there (the width's spring: a pill growing against the buttons
    /// keeps its left edge). The composition window goes back on top: it is click-through, so
    /// the input window below still takes the clicks, and nothing above it cuts the pill out.
    pub fn place(&self, pill: RectPx, frame_w: i32, radius_px: i32, glide: bool) -> Result<()> {
        let x = (pill.x + pill.w / 2 - frame_w / 2 - self.lane.x) as f32;
        if self.stage_x.replace(Some(x)) != Some(x) {
            if glide {
                self.glide_to(x)?;
            } else {
                self.stage.StopAnimation(h!("Offset.X"))?;
                self.stage.SetOffset(windows_numerics::Vector3 { X: x, Y: 0.0, Z: 0.0 })?;
            }
        }
        if self.placed.get() == Some((pill, radius_px)) {
            return Ok(());
        }
        taskbar::place(self.input, self.origin, pill)?;
        set_hit_region(self.input, pill.w, pill.h, radius_px);
        taskbar::place(self.hwnd, self.origin, self.lane)?;
        self.placed.set(Some((pill, radius_px)));
        Ok(())
    }

    /// Springs the frame to `x` from wherever it is (an interrupted glide carries on).
    fn glide_to(&self, x: f32) -> Result<()> {
        use crate::render::motion::{scaled_ticks, spring_samples, SPRING_DURATION_S};
        let c = self.stage.Compositor()?;
        let anim = c.CreateScalarKeyFrameAnimation()?;
        let linear = c.CreateLinearEasingFunction()?;
        for (progress, k) in spring_samples(GLIDE_FRAMES) {
            let expr = format!("this.StartingValue+({x}-this.StartingValue)*{k}");
            anim.InsertExpressionKeyFrameWithEasingFunction(progress, &windows::core::HSTRING::from(expr), &linear)?;
        }
        anim.SetDuration(windows::Foundation::TimeSpan { Duration: scaled_ticks(SPRING_DURATION_S * 1000.0) })?;
        self.stage.StartAnimation(h!("Offset.X"), &anim)
    }

    /// On show now (not hidden for fullscreen, no room or nothing to show).
    pub fn shown(&self) -> bool {
        self.visible.get() == Some(true)
    }

    pub fn set_visible(&self, visible: bool) {
        if self.visible.replace(Some(visible)) == Some(visible) {
            return;
        }
        let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
        unsafe {
            let _ = ShowWindow(self.hwnd, cmd);
            let _ = ShowWindow(self.input, cmd);
        }
    }

    /// Owner for pill-side `SetTimer` calls (WM_TIMER becomes `UiEvent::Timer`).
    pub fn timer_owner(&self) -> HWND {
        self.input
    }
}

/// Key frames of the glide: the pill width spring's count, so the two move as one.
const GLIDE_FRAMES: usize = 32;

impl Drop for PillWindow {
    fn drop(&mut self) {
        // Gone under the mouse: no WM_MOUSELEAVE comes, and the next window must track again.
        HOVER_PILL.with(|h| *h.borrow_mut() = false);
        super::dragdrop::revoke(self.input);
        unsafe {
            let _ = DestroyWindow(self.input);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// The card: a fixed-size, click-through, topmost composition window with the DWM host backdrop
/// (the blur), plus a card-shaped input window. Never activates.
pub struct CardWindow {
    pub hwnd: HWND,
    pub input: HWND,
    pub target: DesktopWindowTarget,
    pub root: ContainerVisual,
    pub frame: RectPx,
}

impl CardWindow {
    pub fn create(g: &Gfx, frame: RectPx) -> Result<Self> {
        let ex = WS_EX_TOOLWINDOW
            | WS_EX_TOPMOST
            | WS_EX_NOACTIVATE
            | WS_EX_NOREDIRECTIONBITMAP
            | WS_EX_LAYERED
            | WS_EX_TRANSPARENT;
        let hwnd = create(PASSIVE_CLASS, ex, frame)?;
        unsafe {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
            let on: i32 = 1;
            let corners = DWMWCP_DONOTROUND;
            let no_border: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
            // Windows 11 only: on Windows 10 they fail, and the card keeps the flat material
            // (os::card_blur).
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const i32 as _, 4);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corners as *const _ as _, 4);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &no_border as *const u32 as _, 4);
        }
        let (target, root) = composition_target(g, hwnd, frame)?;
        let input = create(CARD_INPUT_CLASS, WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, frame)?;
        unsafe { SetLayeredWindowAttributes(input, COLORREF(0), 1, LWA_ALPHA)? };
        Ok(CardWindow { hwnd, input, target, root, frame })
    }

    pub fn show(&self) {
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        }
    }

    /// Moves the (hidden) window to `frame`, the same size: the pill it grows from has moved.
    pub fn move_to(&mut self, frame: RectPx) -> Result<()> {
        unsafe { SetWindowPos(self.hwnd, None, frame.x, frame.y, frame.w, frame.h, SWP_NOACTIVATE | SWP_NOZORDER)? };
        self.frame = frame;
        Ok(())
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.input, SW_HIDE);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Back to the top of the topmost band: a click on the taskbar raises it above the card,
    /// which would hide the closing morph behind it.
    pub fn raise(&self) {
        unsafe {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            if IsWindowVisible(self.input).as_bool() {
                let _ = SetWindowPos(self.input, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
            }
        }
    }

    /// Puts the input window over `r` (screen pixels) with a rounded hit region, above the card.
    pub fn set_hit(&self, r: RectPx, radius_px: i32) -> Result<()> {
        unsafe {
            SetWindowPos(self.input, Some(HWND_TOPMOST), r.x, r.y, r.w, r.h, SWP_NOACTIVATE | SWP_SHOWWINDOW)?;
        }
        set_hit_region(self.input, r.w, r.h, radius_px);
        Ok(())
    }

    pub fn hide_hit(&self) {
        unsafe {
            let _ = ShowWindow(self.input, SW_HIDE);
        }
    }

    /// Owner for card-side `SetTimer` calls.
    pub fn timer_owner(&self) -> HWND {
        self.input
    }

    /// The picker and the question card take keyboard focus; every other card never activates.
    pub fn set_focusable(&self, focusable: bool) {
        unsafe {
            let ex = GetWindowLongPtrW(self.input, GWL_EXSTYLE) as u32;
            let ex = if focusable { ex & !WS_EX_NOACTIVATE.0 } else { ex | WS_EX_NOACTIVATE.0 };
            SetWindowLongPtrW(self.input, GWL_EXSTYLE, ex as isize);
            if focusable {
                let _ = SetForegroundWindow(self.input);
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(self.input));
            }
        }
    }
}

impl Drop for CardWindow {
    fn drop(&mut self) {
        HOVER_CARD.with(|h| *h.borrow_mut() = false);
        super::dragdrop::revoke(self.input);
        unsafe {
            let _ = DestroyWindow(self.input);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// The in-game island: a topmost, click-through composition window that never activates, at the
/// top of a fullscreen app's monitor; the game keeps the mouse and the keyboard.
pub struct IslandWindow {
    pub hwnd: HWND,
    pub target: DesktopWindowTarget,
    pub root: ContainerVisual,
}

impl IslandWindow {
    pub fn create(g: &Gfx) -> Result<Self> {
        let ex = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | WS_EX_LAYERED | WS_EX_TRANSPARENT;
        let r = RectPx { x: -20_000, y: -20_000, w: 1, h: 1 };
        let hwnd = create(PASSIVE_CLASS, ex, r)?;
        unsafe {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)?;
            // A pill-shaped content: no corners or border of DWM's around the window.
            let corners = DWMWCP_DONOTROUND;
            let no_border: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corners as *const _ as _, 4);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &no_border as *const u32 as _, 4);
        }
        let (target, root) = composition_target(g, hwnd, r)?;
        Ok(IslandWindow { hwnd, target, root })
    }

    /// At `r` (screen pixels), on top of everything, without taking the focus.
    pub fn show_at(&self, r: RectPx) -> Result<()> {
        self.root.SetSize(windows_numerics::Vector2 { X: r.w as f32, Y: r.h as f32 })?;
        unsafe { SetWindowPos(self.hwnd, Some(HWND_TOPMOST), r.x, r.y, r.w, r.h, SWP_NOACTIVATE | SWP_SHOWWINDOW)? };
        Ok(())
    }
}

impl Drop for IslandWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Hidden window that owns the tray icon, the hotkey and app-wide timers. Top-level (not
/// message-only) so it also receives broadcasts such as TaskbarCreated.
pub struct MainWindow {
    pub hwnd: HWND,
}

impl MainWindow {
    pub fn create() -> Result<Self> {
        let message = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        TASKBAR_CREATED.store(message, std::sync::atomic::Ordering::Relaxed);
        let settings = unsafe { RegisterWindowMessageW(&windows::core::HSTRING::from(crate::settings::SETTINGS_CHANGED)) };
        SETTINGS_CHANGED.store(settings, std::sync::atomic::Ordering::Relaxed);
        let hwnd = create(MAIN_CLASS, WS_EX_TOOLWINDOW, RectPx { x: 0, y: 0, w: 1, h: 1 })?;
        MAIN_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
        super::session::watch_unlocks(hwnd);
        Ok(MainWindow { hwnd })
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    push_event(UiEvent::Foreground);
}

/// Queues `UiEvent::Foreground` whenever another process's window becomes the foreground window.
pub fn watch_foreground() -> HWINEVENTHOOK {
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }
}
