//! The hook side of a permission answered on the card: a hidden window titled with the ask's id,
//! which Pebbly sends its answer to, waited on for the seconds Pebbly named.
//!
//! The id is public (a window title); an answer must also carry the nonce, which only went to
//! Pebbly inside the ask itself, so another process can't approve in the card's name.

use std::cell::{Cell, RefCell};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostQuitMessage, RegisterClassW, SetTimer,
    TranslateMessage, MSG, MSGFLT_ALLOW, WM_COPYDATA, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use super::ipc::{self, Command};

/// The class Pebbly looks the waiting hook up by (the title is the ask's id).
pub const ASK_CLASS: windows::core::PCWSTR = w!("PebblyAsk");

thread_local! {
    /// (id, nonce) of the ask this process waits on.
    static WAITING_FOR: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
    static ANSWER: Cell<Option<bool>> = const { Cell::new(None) };
}

extern "system" fn ask_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COPYDATA => match unsafe { ipc::receive(lp) } {
            Some(Command::Answer { id, nonce, allow }) if WAITING_FOR.with(|w| { let w = w.borrow(); w.0 == id && w.1 == nonce }) => {
                ANSWER.with(|a| a.set(Some(allow)));
                unsafe { PostQuitMessage(0) };
                LRESULT(1)
            }
            _ => LRESULT(0),
        },
        WM_TIMER => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// The hidden window, made before the ask is sent so a quick click always finds it.
pub struct AskWindow(HWND);

impl AskWindow {
    pub fn create(id: &str, nonce: &str) -> Option<AskWindow> {
        WAITING_FOR.with(|w| *w.borrow_mut() = (id.to_string(), nonce.to_string()));
        unsafe {
            let instance = GetModuleHandleW(None).ok()?.into();
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(ask_proc), hInstance: instance, lpszClassName: ASK_CLASS, ..Default::default() });
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, ASK_CLASS, &HSTRING::from(id), WS_POPUP, 0, 0, 0, 0, None, None, Some(instance), None).ok()?;
            // An elevated Claude still takes the (medium) Pebbly's answer; the nonce guards it.
            let _ = ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None);
            Some(AskWindow(hwnd))
        }
    }

    /// Waits up to `secs` for the card's answer: allow, deny, or none (timed out).
    pub fn wait(&self, secs: u32) -> Option<bool> {
        unsafe {
            SetTimer(Some(self.0), 1, secs.saturating_mul(1000), None);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        ANSWER.with(|a| a.get())
    }
}

impl Drop for AskWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
