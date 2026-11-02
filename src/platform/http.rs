//! A plain HTTPS GET through WinHTTP. Windows.Web.Http (WinINet underneath) can't reach some
//! servers that only take TLS 1.3 (Open-Meteo fails with 0x80072F8F, a secure failure, while
//! curl gets through); WinHTTP asks for TLS 1.2 and 1.3 explicitly.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;

/// GETs `url` and returns the body as text, or why not.
pub fn get_text(url: &str) -> Result<String, String> {
    match get_bytes(url, &[])? {
        (200, body) => String::from_utf8(body).map_err(|e| format!("not UTF-8: {e}")),
        (status, _) => Err(format!("HTTP {status}")),
    }
}

/// GETs `url` with extra request `headers` (redirects followed): the status and the body.
pub fn get_bytes(url: &str, headers: &[(&str, &str)]) -> Result<(u32, Vec<u8>), String> {
    let wide: Vec<u16> = url.encode_utf16().collect();
    // Null pointers with non-zero lengths: the parts point into `wide` itself, where the path and
    // the query follow each other.
    let mut parts = URL_COMPONENTS {
        dwStructSize: std::mem::size_of::<URL_COMPONENTS>() as u32,
        dwSchemeLength: 1,
        dwHostNameLength: 1,
        dwUrlPathLength: 1,
        dwExtraInfoLength: 1,
        ..Default::default()
    };
    unsafe {
        WinHttpCrackUrl(&wide, 0, &mut parts).map_err(|e| format!("bad URL: {e:?}"))?;
        let host = String::from_utf16_lossy(std::slice::from_raw_parts(parts.lpszHostName.0, parts.dwHostNameLength as usize));
        let rest = String::from_utf16_lossy(std::slice::from_raw_parts(parts.lpszUrlPath.0, (parts.dwUrlPathLength + parts.dwExtraInfoLength) as usize));
        let session = Handle(WinHttpOpen(&HSTRING::from("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Pebbly"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return Err(format!("WinHttpOpen: {:?}", windows::core::Error::from_thread()));
        }
        let protocols: u32 = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
        if WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&protocols.to_ne_bytes())).is_err() {
            // Windows 10 has no TLS 1.3 and may refuse the pair: 1.2 alone, which every source takes.
            let _ = WinHttpSetOption(Some(session.0), WINHTTP_OPTION_SECURE_PROTOCOLS, Some(&WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2.to_ne_bytes()));
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 15_000);
        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(host), parts.nPort, 0));
        if connect.0.is_null() {
            return Err(format!("WinHttpConnect: {:?}", windows::core::Error::from_thread()));
        }
        let secure = if parts.nScheme == WINHTTP_INTERNET_SCHEME_HTTPS { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let request = Handle(WinHttpOpenRequest(connect.0, &HSTRING::from("GET"), &HSTRING::from(rest), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), secure));
        if request.0.is_null() {
            return Err(format!("WinHttpOpenRequest: {:?}", windows::core::Error::from_thread()));
        }
        let extra: Vec<u16> = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect::<String>().encode_utf16().collect();
        WinHttpSendRequest(request.0, (!extra.is_empty()).then_some(&extra[..]), None, 0, 0, 0).map_err(|e| format!("send: {e:?}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("receive: {e:?}"))?;
        let mut status = 0u32;
        let mut size = 4u32;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        );
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).map_err(|e| format!("read: {e:?}"))?;
            if available == 0 {
                break;
            }
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut _, available, &mut read).map_err(|e| format!("read: {e:?}"))?;
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok((status, body))
    }
}

/// Closes a WinHTTP handle when it goes.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// Waits for a WinRT async operation for at most `secs`, then cancels it: Windows.Web.Http has no
/// timeout of its own, and a stalled connection would hold a worker (a live match's polling) for
/// good.
#[macro_export]
macro_rules! join_within {
    ($op:expr, $secs:expr) => {{
        let op = $op;
        let start = std::time::Instant::now();
        loop {
            match op.Status() {
                Ok(windows_future::AsyncStatus::Started) if start.elapsed() < std::time::Duration::from_secs($secs) => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Ok(windows_future::AsyncStatus::Started) => {
                    let _ = op.Cancel();
                    break Err(windows::core::Error::new(windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult(), "timed out"));
                }
                _ => break op.GetResults(),
            }
        }
    }};
}
