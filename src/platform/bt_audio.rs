//! Connects or disconnects a paired Bluetooth headset's audio as Windows' own Bluetooth settings
//! do: IOCTL_KS_PROPERTY with KSPROPSETID_BtAudio (0 reconnect, 1 disconnect) on the
//! kernel-streaming interface of its A2DP device (ToothTray's way; Buds2Popup's). The interface is
//! found by the headset's Bluetooth address: no device ids written in.

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW, CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_SUCCESS};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::IO::DeviceIoControl;

const KSCATEGORY_AUDIO: GUID = GUID::from_u128(0x6994ad04_93ef_11d0_a3cc_00a0c9223196);
const KSPROPSETID_BT_AUDIO: GUID = GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);
const IOCTL_KS_PROPERTY: u32 = 0x002F_0003;
const KSPROPERTY_TYPE_GET: u32 = 1;
const ONESHOT_RECONNECT: u32 = 0;
const ONESHOT_DISCONNECT: u32 = 1;
/// The A2DP sink service: the headset's music audio.
const A2DP_SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";

#[repr(C)]
struct KsProperty {
    set: GUID,
    id: u32,
    flags: u32,
}

/// A kernel-streaming interface of `address`'s A2DP device ("A1:B2:C3:D4:E5:F6"): its path names
/// both (`\\?\BTHENUM#{0000110b-…}_vid&…#…&a1b2c3d4e5f6_c00000000#{6994ad04-…}\…`).
pub fn is_a2dp_of(path: &str, address: &str) -> bool {
    let (path, address) = (path.to_lowercase(), address.replace(':', "").to_lowercase());
    address.len() == 12 && path.contains(A2DP_SINK) && path.contains(&address)
}

pub fn reconnect(address: &str) -> Result<(), String> {
    send(address, ONESHOT_RECONNECT)
}

pub fn disconnect(address: &str) -> Result<(), String> {
    send(address, ONESHOT_DISCONNECT)
}

/// The audio interfaces present (a disconnected headset's stay).
fn audio_interfaces() -> Result<Vec<String>, String> {
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_Interface_List_SizeW(&mut len, &KSCATEGORY_AUDIO, PCWSTR::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS || len <= 1 {
            return Err("no audio interfaces".into());
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(&KSCATEGORY_AUDIO, PCWSTR::null(), &mut buf, CM_GET_DEVICE_INTERFACE_LIST_PRESENT) != CR_SUCCESS {
            return Err("audio interfaces not read".into());
        }
        Ok(buf.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect())
    }
}

fn send(address: &str, id: u32) -> Result<(), String> {
    let paths: Vec<String> = audio_interfaces()?.into_iter().filter(|p| is_a2dp_of(p, address)).collect();
    if paths.is_empty() {
        return Err("the headset's audio device isn't there".into());
    }
    let mut last = String::new();
    for path in paths {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        unsafe {
            let handle = match CreateFileW(PCWSTR(wide.as_ptr()), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) {
                Ok(h) => h,
                Err(e) => {
                    last = e.message();
                    continue;
                }
            };
            let prop = KsProperty { set: KSPROPSETID_BT_AUDIO, id, flags: KSPROPERTY_TYPE_GET };
            let mut returned = 0u32;
            let sent = DeviceIoControl(handle, IOCTL_KS_PROPERTY, Some(&prop as *const _ as *const _), std::mem::size_of::<KsProperty>() as u32, None, 0, Some(&mut returned), None);
            let _ = CloseHandle(handle);
            match sent {
                Ok(()) => return Ok(()),
                Err(e) => last = e.message(),
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headsets_music_interface_by_its_address() {
        // A made-up path the way Windows writes them.
        let a2dp = r"\\?\BTHENUM#{0000110b-0000-1000-8000-00805f9b34fb}_VID&00010075_PID&A013#8&2b5e1a3c&1&02000000000a_C00000000#{6994ad04-93ef-11d0-a3cc-00a0c9223196}\Wave";
        assert!(is_a2dp_of(a2dp, "02:00:00:00:00:0A"));
        assert!(!is_a2dp_of(a2dp, "02:00:00:00:00:0B"), "another headset");
        let hands_free = a2dp.replace("0000110b", "0000111e");
        assert!(!is_a2dp_of(&hands_free, "02:00:00:00:00:0A"), "the call audio");
        assert!(!is_a2dp_of(a2dp, ""));
    }
}
