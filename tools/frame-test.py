"""Opens a settings window of its own (PEBBLY_SETTINGS_SOLO), photographs it from the screen
(frame, rounded corners and shadow included), minimises and restores it without the mouse,
photographs it during and after the restore, and ends it (without saving).

Usage: python tools/frame-test.py <pebbly-settings.exe> <out-dir>
"""

import ctypes
import os
import struct
import subprocess
import sys
import time
import zlib
from ctypes import wintypes

user32 = ctypes.WinDLL("user32", use_last_error=True)
gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # per-monitor v2: physical pixels

WNDENUMPROC = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
SW_MINIMIZE, SW_RESTORE = 6, 9


def window_of(pid):
    found = []

    def each(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        title = ctypes.create_unicode_buffer(128)
        user32.GetWindowTextW(hwnd, title, 128)
        if owner.value == pid and user32.IsWindowVisible(hwnd) and title.value == "Pebbly Ayarları":
            found.append(hwnd)
        return True

    user32.EnumWindows(WNDENUMPROC(each), 0)
    return found[0] if found else None


def png(path, w, h, bgra):
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        row = bgra[y * w * 4:(y + 1) * w * 4]
        for x in range(w):
            b, g, r = row[x * 4], row[x * 4 + 1], row[x * 4 + 2]
            raw += bytes((r, g, b))
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(raw), 6)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)


def shoot(hwnd, path, margin=28):
    r = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(r))
    x, y, w, h = r.left - margin, r.top - margin, r.right - r.left + 2 * margin, r.bottom - r.top + 2 * margin
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, h)
    old = gdi32.SelectObject(mem, bmp)
    if margin:
        gdi32.BitBlt(mem, 0, 0, w, h, screen, x, y, 0x00CC0020 | 0x40000000)
    else:
        # The window itself, wherever it is in the z-order (PW_RENDERFULLCONTENT).
        user32.PrintWindow(hwnd, mem, 2)
    header = struct.pack("<IiiHHIIiiII", 40, w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    info = ctypes.create_string_buffer(header + b"\0" * 16)
    px = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mem, bmp, 0, h, px, info, 0)
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)
    png(path, w, h, px.raw)
    print(path, (r.left, r.top, r.right, r.bottom))


def main():
    exe, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    env = dict(os.environ, PEBBLY_SETTINGS_SOLO="1")
    proc = subprocess.Popen([exe], env=env)
    hwnd = None
    for _ in range(100):
        time.sleep(0.1)
        hwnd = window_of(proc.pid)
        if hwnd:
            break
    if not hwnd:
        proc.kill()
        sys.exit("no window")
    time.sleep(1.2)
    shoot(hwnd, os.path.join(out, "1-open.png"))
    user32.ShowWindow(hwnd, SW_MINIMIZE)
    time.sleep(1.0)
    user32.ShowWindow(hwnd, SW_RESTORE)
    time.sleep(0.12)
    shoot(hwnd, os.path.join(out, "2-restoring.png"))
    time.sleep(1.2)
    shoot(hwnd, os.path.join(out, "3-restored.png"))
    print("minimised still:", bool(user32.IsIconic(hwnd)))
    user32.ShowWindow(hwnd, 3)  # SW_MAXIMIZE
    time.sleep(1.0)
    shoot(hwnd, os.path.join(out, "4-maximised.png"), margin=0)
    # Killed, not closed: closing saves, and this copy's settings may be older than the real window's.
    proc.kill()


main()
