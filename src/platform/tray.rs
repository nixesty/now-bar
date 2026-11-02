//! Tray icon (a small white pill, drawn at run time) and its menu.

use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS};
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::windows::WM_TRAY;
use crate::i18n::{tr, trf};

pub const MENU_TIMER: u32 = 1;
pub const MENU_STOPWATCH: u32 = 2;
pub const MENU_AUTOSTART: u32 = 3;
pub const MENU_SETTINGS: u32 = 5;
pub const MENU_MATCH_OPEN: u32 = 6;
pub const MENU_MATCH_HIDE: u32 = 7;
pub const MENU_QUIT: u32 = 4;
pub const MENU_UPDATE: u32 = 8;
pub const MENU_BRIEF: u32 = 9;
pub const MENU_WELLBEING: u32 = 10;
pub const MENU_MUSIC: u32 = 11;
pub const MENU_QUESTION: u32 = 12;

const TRAY_ID: u32 = 1;

/// Straight-alpha BGRA pixels of a white capsule centred in a `size`×`size` square.
pub fn pill_icon_bgra(size: usize) -> Vec<u8> {
    let s = size as f32;
    let (w, h) = (s * 0.84, s * 0.44);
    let (x0, y0) = ((s - w) / 2.0, (s - h) / 2.0);
    let r = h / 2.0;
    let mut px = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Signed distance to the capsule: distance to the segment between the end centres, minus r.
            let sx = cx.clamp(x0 + r, x0 + w - r);
            let d = ((cx - sx).powi(2) + (cy - (y0 + r)).powi(2)).sqrt() - r;
            let coverage = (0.5 - d).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let i = (y * size + x) * 4;
                px[i..i + 3].copy_from_slice(&[255, 255, 255]);
                px[i + 3] = (coverage * 255.0).round() as u8;
            }
        }
    }
    px
}

fn pill_icon() -> Result<HICON> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let pixels = pill_icon_bgra(size as usize);
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
        let mask = CreateBitmap(size, size, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: color, ..Default::default() });
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn notify_data(owner: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: owner, uID: TRAY_ID, ..Default::default() }
}

pub fn create_tray(owner: HWND) -> Result<()> {
    let mut nid = notify_data(owner);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = pill_icon()?;
    for (dst, src) in nid.szTip.iter_mut().zip("Pebbly".encode_utf16()) {
        *dst = src;
    }
    unsafe { Shell_NotifyIconW(NIM_ADD, &nid).ok() }
}

pub fn remove_tray(owner: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &notify_data(owner));
    }
}

/// Shows the menu at the cursor and returns the chosen `MENU_*` id (0 if dismissed). A newer
/// release (`update`, its version) comes first, then a match in front of the pill's two items.
pub fn show_menu(owner: HWND, autostart: bool, match_items: bool, update: Option<&str>) -> u32 {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        if let Some(version) = update {
            let _ = AppendMenuW(menu, MF_STRING, MENU_UPDATE as usize, &HSTRING::from(trf("Güncelle: {}", &[&version])));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        if match_items {
            let _ = AppendMenuW(menu, MF_STRING, MENU_MATCH_OPEN as usize, &HSTRING::from(tr("Maç sayfasını aç")));
            let _ = AppendMenuW(menu, MF_STRING, MENU_MATCH_HIDE as usize, &HSTRING::from(tr("Maçı gizle")));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        }
        let _ = AppendMenuW(menu, MF_STRING, MENU_BRIEF as usize, w!("Now Brief"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_WELLBEING as usize, &HSTRING::from(tr("Dijital Denge")));
        let _ = AppendMenuW(menu, MF_STRING, MENU_MUSIC as usize, &HSTRING::from(tr("Haftanın müziği")));
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUESTION as usize, &HSTRING::from(tr("Claude'a sor")));
        let _ = AppendMenuW(menu, MF_STRING, MENU_TIMER as usize, &HSTRING::from(tr("Zamanlayıcı başlat")));
        let _ = AppendMenuW(menu, MF_STRING, MENU_STOPWATCH as usize, &HSTRING::from(tr("Kronometre başlat")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let check = if autostart { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | check, MENU_AUTOSTART as usize, &HSTRING::from(tr("Windows ile başlat")));
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS as usize, &HSTRING::from(tr("Ayarlar…")));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT as usize, &HSTRING::from(tr("Çıkış")));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_CENTERALIGN;
        let chosen = TrackPopupMenu(menu, flags, p.x, p.y, Some(0), owner, None).0 as u32;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &[u8], size: usize, x: usize, y: usize) -> u8 {
        px[(y * size + x) * 4 + 3]
    }

    #[test]
    fn pill_icon_is_a_white_capsule() {
        let size = 32;
        let px = pill_icon_bgra(size);
        assert_eq!(px.len(), size * size * 4);
        assert_eq!(alpha(&px, size, 16, 16), 255);
        assert_eq!(&px[(16 * size + 16) * 4..][..3], &[255, 255, 255]);
        assert_eq!(alpha(&px, size, 0, 0), 0);
        assert_eq!(alpha(&px, size, 16, 4), 0, "capsule is shorter than the icon");
        let edge = alpha(&px, size, 2, 16);
        assert!(edge > 0 && edge < 255, "left end is anti-aliased: {edge}");
    }
}
