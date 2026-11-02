//! The taskbar we live in. The pill is a child window of `Shell_TrayWnd` (TrafficMonitor-style):
//! a separate topmost popup gets covered whenever the taskbar itself has focus, a child never does.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use windows::core::{w, BSTR, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement, SetWinEventHook, TreeScope_Children,
    TreeScope_Descendants, HWINEVENTHOOK, UIA_AutomationIdPropertyId, UIA_BoundingRectanglePropertyId,
};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_GETTASKBARPOS, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::platform::log::log;
use crate::platform::windows::post_taskbar_items;
use crate::platform::os::{self, Generation};
use crate::render::layout::{RectPx, TaskbarAlign, TaskbarInfo};

pub fn find_taskbar() -> Option<HWND> {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), None).ok() }
}

pub fn window_rect(hwnd: HWND) -> Result<RectPx> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r)? };
    Ok(RectPx { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
}

fn explorer_dword(name: windows::core::PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(value)
}

/// Taskbar rect plus the settings that decide where free space is ([`alignment`]).
pub fn taskbar_info(bar: HWND) -> Result<TaskbarInfo> {
    let (align, widgets) = alignment(os::generation(), explorer_dword(w!("TaskbarAl")), explorer_dword(w!("TaskbarDa")));
    Ok(TaskbarInfo { rect: window_rect(bar)?, align, widgets, bottom: edge().is_none_or(|e| e == ABE_BOTTOM) })
}

/// Which screen edge the taskbar is on (`None`: the shell didn't say; taken as the bottom,
/// so a failed ask doesn't hide the pill until the next settings change).
fn edge() -> Option<u32> {
    let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
    (unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } != 0).then_some(data.uEdge)
}

/// The taskbar hides itself until the mouse comes (then a maximised window covers the monitor).
pub fn auto_hides() -> bool {
    let mut data = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 & ABS_AUTOHIDE != 0 }
}

const ABE_BOTTOM: u32 = 3;
const ABS_AUTOHIDE: u32 = 1;

/// Makes `child` a child of the taskbar and places it at `rect` (see [`place`]).
pub fn embed(child: HWND, bar: HWND, origin: RectPx, rect: RectPx) -> Result<()> {
    unsafe {
        // Windows keeps WS_CLIPSIBLINGS here; composition content only shows with it. Because a
        // clip-siblings window is cut out wherever a sibling lies above it, the composition window
        // must be the top-most of our children (see `PillWindow::set_rects`).
        let visible = GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_VISIBLE.0;
        SetWindowLongPtrW(child, GWL_STYLE, (WS_CHILD.0 | visible) as isize);
        // A top-level window has no previous parent, so SetParent may "fail" with a stale last
        // error even though it worked: check the outcome instead. Right after a previous instance
        // was killed (or Explorer restarted) the taskbar refuses for a moment, so retry briefly.
        let mut attached = false;
        for attempt in 0..30 {
            let _ = SetParent(child, Some(bar));
            if GetParent(child).ok() == Some(bar) {
                attached = true;
                if attempt > 0 {
                    crate::platform::log::log(&format!("embedded into the taskbar after {attempt} retries"));
                }
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if !attached {
            return Err(windows::core::Error::new(windows::Win32::Foundation::E_FAIL, "SetParent into the taskbar failed"));
        }
    }
    place(child, origin, rect)
}

/// Moves an embedded child to `rect` and keeps it above its siblings. `rect` is in screen
/// coordinates of the taskbar at `origin`, the rect it was worked out from, not of where the
/// taskbar is now: an auto-hiding one slides, and its children slide along with it.
pub fn place(child: HWND, origin: RectPx, rect: RectPx) -> Result<()> {
    unsafe {
        SetWindowPos(
            child,
            Some(HWND_TOP),
            rect.x - origin.x,
            rect.y - origin.y,
            rect.w,
            rect.h,
            SWP_NOACTIVATE,
        )
        .inspect_err(|e| crate::platform::log::log(&format!("place {rect:?}: {e:?}")))
    }
}

/// The x spans of the taskbar's own items (Start, Search, the app buttons, the tray...), in
/// screen pixels, for the main thread (told with `post_taskbar_items`).
pub type ItemsMailbox = Arc<Mutex<Vec<(i32, i32)>>>;

/// After a window shows or hides the buttons are read soon, and again once the taskbar's own
/// animation has settled.
const LOOK_SOON_MS: [u64; 2] = [150, 700];
/// And every so often regardless: a renamed or pinned button, a missed event.
const LOOK_EVERY_MS: u64 = 2000;

thread_local! {
    static NUDGED: Cell<bool> = const { Cell::new(false) };
}

extern "system" fn on_window(_: HWINEVENTHOOK, _: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    // Only top-level windows that may have a button: no children, no tool windows.
    unsafe {
        if GetAncestor(hwnd, GA_ROOT) != hwnd || GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return;
        }
    }
    NUDGED.with(|n| n.set(true));
}

/// Starts the thread that reads where the taskbar's items are and posts them whenever they
/// change. Windows 11's taskbar is XAML: only UI Automation knows its buttons' rects (the old
/// MSTaskListWClass window is still there, at a stale size). Windows 10's is windows, but its
/// app buttons too are known only to UI Automation.
pub fn watch_items(mailbox: ItemsMailbox) {
    let spawned = std::thread::Builder::new().name("taskbar-items".into()).spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut reader = match ItemReader::new() {
            Ok(r) => r,
            Err(e) => return log(&format!("taskbar items: no UI Automation ({e})")),
        };
        // An app opening or closing shows or hides (or, for packaged apps, uncloaks) its window.
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        let _show = SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_HIDE, None, Some(on_window), 0, 0, flags);
        let _cloak = SetWinEventHook(EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED, None, Some(on_window), 0, 0, flags);
        let mut last: Option<Vec<(i32, i32)>> = None;
        let mut soon: Vec<u64> = Vec::new();
        let mut next = GetTickCount64();
        loop {
            let now = GetTickCount64();
            if now >= next || soon.iter().any(|&t| t <= now) {
                soon.retain(|&t| t > now);
                next = now + LOOK_EVERY_MS;
                let items = reader.read();
                if last.as_ref() != Some(&items) {
                    if last.is_none() {
                        log(&format!("taskbar items ({:?}): {items:?}", reader.generation));
                    }
                    if let Ok(mut slot) = mailbox.lock() {
                        slot.clone_from(&items);
                    }
                    post_taskbar_items();
                    last = Some(items);
                }
            }
            let due = soon.iter().copied().chain([next]).min().unwrap_or(next);
            let wait = due.saturating_sub(GetTickCount64()).min(u32::MAX as u64) as u32;
            MsgWaitForMultipleObjects(None, false, wait, QS_ALLINPUT);
            // The hooks' callbacks run in here.
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&msg);
            }
            if NUDGED.with(|n| n.replace(false)) && soon.is_empty() {
                let now = GetTickCount64();
                soon = LOOK_SOON_MS.iter().map(|ms| now + ms).collect();
            }
        }
    });
    if let Err(e) = spawned {
        log(&format!("taskbar items: no thread ({e})"));
    }
}

/// `parent`'s visible child windows, but ours (the pill is the taskbar's child too).
fn children(parent: HWND) -> Vec<BarWindow> {
    let mut found = Vec::new();
    let mut prev = None;
    unsafe {
        while let Ok(hwnd) = FindWindowExW(Some(parent), prev, None, None) {
            prev = Some(hwnd);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if !IsWindowVisible(hwnd).as_bool() || pid == GetCurrentProcessId() {
                continue;
            }
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
            if let Ok(rect) = window_rect(hwnd) {
                found.push(BarWindow { class: String::from_utf16_lossy(&class[..n]), rect });
            }
        }
    }
    found
}

struct ItemReader {
    uia: IUIAutomation,
    cache: IUIAutomationCacheRequest,
    generation: Generation,
    /// The taskbar and its XAML frame (Start, Search, the buttons), found once per taskbar.
    frame: Option<(HWND, IUIAutomationElement)>,
}

impl ItemReader {
    fn new() -> Result<Self> {
        unsafe {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let cache = uia.CreateCacheRequest()?;
            cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
            Ok(ItemReader { uia, cache, generation: os::generation(), frame: None })
        }
    }

    /// The items' spans now: the frame's children (one cross-process call, about 6 ms) and the
    /// tray's window, or Windows 10's windows and buttons. Empty when there is no taskbar.
    fn read(&mut self) -> Vec<(i32, i32)> {
        let Some(bar) = find_taskbar() else {
            self.frame = None;
            return Vec::new();
        };
        let Ok(on) = window_rect(bar) else { return Vec::new() };
        if self.generation == Generation::Windows10 {
            return self.read_classic(bar, on);
        }
        let mut spans = Vec::new();
        if let Some(frame) = self.frame(bar) {
            match self.children(&frame, on) {
                Ok(v) => spans = v,
                // Explorer restarted under the same handle, or the frame was rebuilt.
                Err(_) => self.frame = None,
            }
        }
        if let Ok(tray) = unsafe { FindWindowExW(Some(bar), None, w!("TrayNotifyWnd"), None) } {
            if let Ok(r) = window_rect(tray) {
                spans.push((r.x, r.right()));
            }
        }
        spans
    }

    fn frame(&mut self, bar: HWND) -> Option<IUIAutomationElement> {
        if self.frame.as_ref().is_none_or(|(b, _)| *b != bar) {
            self.frame = unsafe {
                let id = VARIANT::from(BSTR::from("TaskbarFrame"));
                let found = self.uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &id).and_then(|c| {
                    self.uia.ElementFromHandle(bar)?.FindFirst(TreeScope_Descendants, &c)
                });
                found.ok().map(|f| (bar, f))
            };
        }
        self.frame.as_ref().map(|(_, f)| f.clone())
    }

    /// Windows 10: the taskbar's and its rebar's windows, and the app buttons in MSTaskListWClass
    /// (a window as wide as their whole band) from UI Automation.
    fn read_classic(&self, bar: HWND, on: RectPx) -> Vec<(i32, i32)> {
        let top = children(bar);
        let rebar = unsafe { FindWindowExW(Some(bar), None, w!("ReBarWindow32"), None) }.ok();
        let band = rebar.map(children).unwrap_or_default();
        let list = rebar.and_then(|r| unsafe {
            let sw = FindWindowExW(Some(r), None, w!("MSTaskSwWClass"), None).ok()?;
            FindWindowExW(Some(sw), None, w!("MSTaskListWClass"), None).ok()
        });
        let buttons = list.map(|l| self.buttons(l)).unwrap_or_default();
        classic_items(on, &top, &band, &buttons)
    }

    /// The app buttons' rects (empty when UI Automation can't say: the pill then keeps its home).
    fn buttons(&self, list: HWND) -> Vec<RectPx> {
        let read = || -> Result<Vec<RectPx>> {
            unsafe {
                let all = self.uia.ElementFromHandle(list)?.FindAllBuildCache(TreeScope_Children, &self.uia.CreateTrueCondition()?, &self.cache)?;
                let mut rects = Vec::new();
                for i in 0..all.Length()? {
                    let r = all.GetElement(i)?.CachedBoundingRectangle()?;
                    rects.push(RectPx { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top });
                }
                Ok(rects)
            }
        };
        read().unwrap_or_default()
    }

    /// The frame's children with a size, on the taskbar's row.
    fn children(&self, frame: &IUIAutomationElement, on: RectPx) -> Result<Vec<(i32, i32)>> {
        unsafe {
            let all = frame.FindAllBuildCache(TreeScope_Children, &self.uia.CreateTrueCondition()?, &self.cache)?;
            let mut spans = Vec::new();
            for i in 0..all.Length()? {
                let r = all.GetElement(i)?.CachedBoundingRectangle()?;
                if r.right > r.left && r.bottom > on.y && r.top < on.bottom() {
                    spans.push((r.left, r.right));
                }
            }
            Ok(spans)
        }
    }
}

/// Icons centred (`TaskbarAl`) and Widgets on (`TaskbarDa`), with Windows 11's defaults for a
/// missing value. Windows 10's buttons always start at the left and it has no Widgets button.
pub fn alignment(generation: Generation, al: Option<u32>, da: Option<u32>) -> (TaskbarAlign, bool) {
    match generation {
        Generation::Windows10 => (TaskbarAlign::Left, false),
        Generation::Windows11 => (if al == Some(0) { TaskbarAlign::Left } else { TaskbarAlign::Center }, da.unwrap_or(1) != 0),
    }
}

/// One of the classic taskbar's windows (visible, not ours).
#[derive(Clone, Debug)]
pub struct BarWindow {
    pub class: String,
    pub rect: RectPx,
}

/// What takes room on Windows 10's taskbar `on` (spans in screen pixels): the taskbar's own
/// children (`top`: Start, Search, Task View, the tray...) but the rebar, the rebar's (`band`:
/// toolbars) but the app buttons' band, and the app buttons themselves (`buttons`).
pub fn classic_items(on: RectPx, top: &[BarWindow], band: &[BarWindow], buttons: &[RectPx]) -> Vec<(i32, i32)> {
    // A window as wide as half the bar hosts others (the rebar is already left out by name).
    let item = |r: &RectPx| r.w > 0 && r.h > 0 && r.w <= on.w / 2 && r.bottom() > on.y && r.y < on.bottom();
    let windows = top.iter().filter(|w| w.class != "ReBarWindow32").chain(band.iter().filter(|w| w.class != "MSTaskSwWClass"));
    windows.map(|w| w.rect).chain(buttons.iter().copied()).filter(item).map(|r| (r.x, r.right())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_10_buttons_are_always_on_the_left() {
        assert_eq!(alignment(Generation::Windows10, None, None), (TaskbarAlign::Left, false));
        assert_eq!(alignment(Generation::Windows10, Some(1), Some(1)), (TaskbarAlign::Left, false), "a leftover value");
    }

    #[test]
    fn windows_11_reads_its_settings() {
        assert_eq!(alignment(Generation::Windows11, None, None), (TaskbarAlign::Center, true));
        assert_eq!(alignment(Generation::Windows11, Some(0), Some(0)), (TaskbarAlign::Left, false));
        assert_eq!(alignment(Generation::Windows11, Some(1), Some(1)), (TaskbarAlign::Center, true));
    }

    fn window(class: &str, x: i32, right: i32) -> BarWindow {
        BarWindow { class: class.into(), rect: RectPx { x, y: 1040, w: right - x, h: 40 } }
    }

    /// Windows 10 at 100 %: Start, the search box, Task View, the app buttons' band up to the
    /// tray, and the buttons UI Automation finds in it.
    fn win10_bar() -> (RectPx, Vec<BarWindow>, Vec<BarWindow>) {
        let on = RectPx { x: 0, y: 1040, w: 1920, h: 40 };
        let top = vec![
            window("Start", 0, 48),
            window("TrayDummySearchControl", 48, 392),
            window("TrayButton", 392, 440),
            window("ReBarWindow32", 440, 1660),
            window("TrayNotifyWnd", 1660, 1920),
        ];
        (on, top, vec![window("MSTaskSwWClass", 442, 1660)])
    }

    #[test]
    fn classic_taskbar_items_skip_the_band_and_take_the_buttons() {
        let (on, top, band) = win10_bar();
        let buttons: Vec<RectPx> = (0..3).map(|i| RectPx { x: 442 + 48 * i, y: 1040, w: 48, h: 40 }).collect();
        let items = classic_items(on, &top, &band, &buttons);
        assert_eq!(items, vec![(0, 48), (48, 392), (392, 440), (1660, 1920), (442, 490), (490, 538), (538, 586)]);
    }

    /// A toolbar beside the app buttons (Links, Desktop) takes its room.
    #[test]
    fn toolbars_in_the_rebar_count() {
        let (on, top, mut band) = win10_bar();
        band[0].rect.w = 1500 - 442;
        band.push(window("ToolbarWindow32", 1500, 1660));
        assert!(classic_items(on, &top, &band, &[]).contains(&(1500, 1660)));
    }

    /// Nothing that only hosts others (as wide as most of the bar), nothing empty, nothing off the
    /// taskbar's row (a button scrolled to another page).
    #[test]
    fn hosts_empty_and_off_row_windows_dont_count() {
        let (on, mut top, band) = win10_bar();
        top.push(window("Windows.UI.Composition.DesktopWindowContentBridge", 0, 1920));
        top.push(window("Empty", 900, 900));
        let off = RectPx { x: 700, y: 1080, w: 48, h: 40 };
        let items = classic_items(on, &top, &band, &[off]);
        assert_eq!(items, vec![(0, 48), (48, 392), (392, 440), (1660, 1920)]);
    }
}
