//! Audio outputs: the default one (its name for the media card's chip), the list for our own
//! output picker, switching Windows' default output, and the output volume.

use std::sync::Mutex;

use windows::core::{implement, Result, GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::{PKEY_DeviceInterface_FriendlyName, PKEY_Device_DeviceDesc};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl, IAudioMeterInformation,
};
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, eRender, AUDIO_VOLUME_NOTIFICATION_DATA, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor, DEVICE_STATE, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantToStringAlloc, PropVariantToUInt32};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};

use super::windows::{post_output_changed, post_volume_changed};
use crate::render::gfx::wide;

/// PKEY_Device_EnumeratorName: "BTHENUM" / "BTHHFENUM" for Bluetooth audio.
const PKEY_DEVICE_ENUMERATOR_NAME: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 24 };

/// What kind of output a device is, for its icon in the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Speakers,
    Headphones,
    Bluetooth,
    Headset,
    Display,
    Other,
}

/// How a device is attached, from its enumerator (Sonar's virtual devices are "ROOT").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutputBus {
    Bluetooth,
    Usb,
    Internal,
    Virtual,
}

pub fn output_bus(enumerator: &str) -> OutputBus {
    let e = enumerator.to_ascii_uppercase();
    if e.starts_with("BTH") {
        OutputBus::Bluetooth
    } else if e.starts_with("USB") {
        OutputBus::Usb
    } else if e == "HDAUDIO" || e == "INTELAUDIO" || e == "ACPI" || e == "PCI" {
        OutputBus::Internal
    } else {
        OutputBus::Virtual
    }
}

/// One active playback device.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub kind: OutputKind,
    pub bus: OutputBus,
    pub default: bool,
}

/// The picker's order: the current device first, then Bluetooth, USB, built-in, virtual, then name.
pub fn picker_order(devices: &[OutputDevice]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..devices.len()).collect();
    order.sort_by(|&a, &b| {
        let key = |d: &OutputDevice| (!d.default, d.bus, d.name.to_lowercase());
        key(&devices[a]).cmp(&key(&devices[b]))
    });
    order
}

/// Kind from the endpoint's form factor (EndpointFormFactor) and its device's enumerator;
/// Bluetooth wins, since Buds report Headphones or Headset.
pub fn output_kind(form_factor: u32, enumerator: &str) -> OutputKind {
    let e = enumerator.to_ascii_uppercase();
    if e.starts_with("BTH") {
        return OutputKind::Bluetooth;
    }
    match form_factor {
        1 => OutputKind::Speakers,
        3 => OutputKind::Headphones,
        5 => OutputKind::Headset,
        8 | 9 => OutputKind::Display,
        _ => OutputKind::Other,
    }
}

/// Vendor/driver names that say nothing useful about where the sound goes.
const GENERIC: [&str; 6] = ["realtek", "high definition", "audio device", "intel", "nvidia", "amd"];

/// Chip label from a device's description ("Kulaklıklar") and its interface name
/// ("Galaxy Buds2", or a driver such as "Realtek(R) Audio"): the interface name when it names
/// the device, otherwise the description.
pub fn output_label(desc: &str, iface: &str) -> String {
    let lower = iface.to_lowercase();
    if iface.trim().is_empty() || GENERIC.iter().any(|g| lower.contains(g)) {
        desc.trim().to_string()
    } else {
        iface.trim().to_string()
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn string_property(store: &windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore, key: &PROPERTYKEY) -> String {
    unsafe {
        let Ok(value) = store.GetValue(key) else { return String::new() };
        match PropVariantToStringAlloc(&value) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const _));
                s
            }
            Err(_) => String::new(),
        }
    }
}

fn u32_property(store: &windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore, key: &PROPERTYKEY) -> u32 {
    unsafe { store.GetValue(key).ok().and_then(|v| PropVariantToUInt32(&v).ok()).unwrap_or(0) }
}

fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn device(id: &str) -> Result<IMMDevice> {
    let w = wide(id);
    unsafe { enumerator()?.GetDevice(PCWSTR(w.as_ptr())) }
}

/// Every active playback device, in Windows' order, the default one marked.
pub fn list_outputs() -> Vec<OutputDevice> {
    let Ok(en) = enumerator() else { return Vec::new() };
    unsafe {
        let default_id = en.GetDefaultAudioEndpoint(eRender, eMultimedia).and_then(|d| d.GetId()).map(take_pwstr).ok();
        let Ok(all) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        (0..all.GetCount().unwrap_or(0))
            .filter_map(|i| {
                let d = all.Item(i).ok()?;
                let id = take_pwstr(d.GetId().ok()?);
                let store = d.OpenPropertyStore(STGM_READ).ok()?;
                let name = output_label(&string_property(&store, &PKEY_Device_DeviceDesc), &string_property(&store, &PKEY_DeviceInterface_FriendlyName));
                let enumerator = string_property(&store, &PKEY_DEVICE_ENUMERATOR_NAME);
                let kind = output_kind(u32_property(&store, &PKEY_AudioEndpoint_FormFactor), &enumerator);
                let default = default_id.as_deref() == Some(id.as_str());
                Some(OutputDevice { id, name, kind, bus: output_bus(&enumerator), default })
            })
            .collect()
    }
}

/// Windows' undocumented (but stable since Vista) policy interface; only `SetDefaultEndpoint` is
/// called, the other slots are declared to keep the vtable in order.
#[allow(non_snake_case)]
mod policy {
    use std::ffi::c_void;

    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::ERole;

    #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: windows_core::IUnknown {
        unsafe fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        unsafe fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
        unsafe fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        unsafe fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        unsafe fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, default_period: *mut i64, min_period: *mut i64) -> HRESULT;
        unsafe fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        unsafe fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        unsafe fn GetPropertyValue(&self, device: PCWSTR, key: *const PROPERTYKEY, value: *mut c_void) -> HRESULT;
        unsafe fn SetPropertyValue(&self, device: PCWSTR, key: *const PROPERTYKEY, value: *mut c_void) -> HRESULT;
        unsafe fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        unsafe fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    pub fn set_default(policy: &IPolicyConfig, device: PCWSTR, role: ERole) -> windows::core::Result<()> {
        unsafe { policy.SetDefaultEndpoint(device, role).ok() }
    }
}
use policy::IPolicyConfig;

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// Makes `id` Windows' default output (console and multimedia roles: calls stay on the chat device,
/// e.g. SteelSeries Sonar's Chat).
pub fn set_default_output(id: &str) -> Result<()> {
    let w = wide(id);
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL)?;
        for role in [eConsole, eMultimedia] {
            policy::set_default(&policy, PCWSTR(w.as_ptr()), role)?;
        }
    }
    Ok(())
}

fn endpoint_volume(id: &str) -> Result<IAudioEndpointVolume> {
    unsafe { device(id)?.Activate(CLSCTX_ALL, None) }
}

/// Master volume of `id`, 0–1.
pub fn output_volume(id: &str) -> Option<f32> {
    unsafe { endpoint_volume(id).ok()?.GetMasterVolumeLevelScalar().ok() }
}

pub fn set_output_volume(id: &str, v: f32) -> Result<()> {
    unsafe { endpoint_volume(id)?.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null()) }
}

pub fn output_muted(id: &str) -> Option<bool> {
    unsafe { endpoint_volume(id).ok()?.GetMute().ok().map(|b| b.as_bool()) }
}

pub fn set_output_mute(id: &str, mute: bool) -> Result<()> {
    unsafe { endpoint_volume(id)?.SetMute(mute, std::ptr::null()) }
}

/// Mutes or unmutes the microphone calls use (the default communications one); its new state.
pub fn toggle_mic_mute() -> Result<bool> {
    unsafe {
        let volume: IAudioEndpointVolume = enumerator()?.GetDefaultAudioEndpoint(eCapture, eCommunications)?.Activate(CLSCTX_ALL, None)?;
        let muted = !volume.GetMute()?.as_bool();
        volume.SetMute(muted, std::ptr::null())?;
        Ok(muted)
    }
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeNotify;

impl IAudioEndpointVolumeCallback_Impl for VolumeNotify_Impl {
    fn OnNotify(&self, _: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        post_volume_changed();
        Ok(())
    }
}

/// The default output's level (0..1) and whether it is muted.
pub fn default_output_level() -> Option<(f32, bool)> {
    unsafe {
        let device = enumerator().ok()?.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        let v = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()?;
        Some((v.GetMasterVolumeLevelScalar().ok()?, v.GetMute().ok()?.as_bool()))
    }
}

/// The default output's peak meter: how loud what is playing is right now (0..1).
pub struct PeakMeter(IAudioMeterInformation);

impl PeakMeter {
    pub fn default_output() -> Option<PeakMeter> {
        unsafe {
            let device = enumerator().ok()?.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok().map(PeakMeter)
        }
    }

    /// Left and right peaks (a mono output gives the same for both).
    pub fn peaks(&self) -> (f32, f32) {
        unsafe {
            let n = self.0.GetMeteringChannelCount().unwrap_or(0);
            if n >= 2 {
                let mut v = vec![0f32; n as usize];
                if self.0.GetChannelsPeakValues(&mut v).is_ok() {
                    return (v[0], v[1]);
                }
            }
            let p = self.0.GetPeakValue().unwrap_or(0.0);
            (p, p)
        }
    }
}

/// Name of the current default playback device, e.g. "Galaxy Buds2" or "Hoparlör".
pub fn default_output_name() -> Option<String> {
    unsafe {
        let device = enumerator().ok()?.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let label = output_label(&string_property(&store, &PKEY_Device_DeviceDesc), &string_property(&store, &PKEY_DeviceInterface_FriendlyName));
        (!label.is_empty()).then_some(label)
    }
}

#[implement(IMMNotificationClient)]
struct OutputNotify;

impl IMMNotificationClient_Impl for OutputNotify_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        post_output_changed();
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        post_output_changed();
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        post_output_changed();
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eMultimedia {
            post_output_changed();
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

/// Raises `UiEvent::OutputChanged` when the default playback device or the device list changes,
/// and `UiEvent::VolumeChanged` when the default device's volume does.
pub struct OutputWatch {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    volume: Mutex<Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>>,
}

impl OutputWatch {
    pub fn start() -> Result<Self> {
        let enumerator = enumerator()?;
        let client: IMMNotificationClient = OutputNotify.into();
        unsafe { enumerator.RegisterEndpointNotificationCallback(&client)? };
        let watch = OutputWatch { enumerator, client, volume: Mutex::new(None) };
        watch.follow_default();
        Ok(watch)
    }

    /// Watches the volume of the current default device; call again after it changed (not from
    /// inside a notification — Windows forbids re-registering there).
    pub fn follow_default(&self) {
        let Ok(mut slot) = self.volume.lock() else { return };
        if let Some((v, cb)) = slot.take() {
            unsafe {
                let _ = v.UnregisterControlChangeNotify(&cb);
            }
        }
        unsafe {
            let Ok(d) = self.enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return };
            let Ok(v) = d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return };
            let cb: IAudioEndpointVolumeCallback = VolumeNotify.into();
            if v.RegisterControlChangeNotify(&cb).is_ok() {
                *slot = Some((v, cb));
            }
        }
    }
}

impl Drop for OutputWatch {
    fn drop(&mut self) {
        unsafe {
            let _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.client);
            if let Ok(mut slot) = self.volume.lock() {
                if let Some((v, cb)) = slot.take() {
                    let _ = v.UnregisterControlChangeNotify(&cb);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_labels() {
        assert_eq!(output_label("Kulaklıklar", "Galaxy Buds2"), "Galaxy Buds2");
        assert_eq!(output_label("Hoparlör", "Realtek(R) Audio"), "Hoparlör");
        assert_eq!(output_label("Hoparlörler", "AMD High Definition Audio Device"), "Hoparlörler");
        assert_eq!(output_label("Hoparlör", ""), "Hoparlör");
    }

    #[test]
    fn output_kind_maps_form_factors() {
        assert_eq!(output_kind(1, "HDAUDIO"), OutputKind::Speakers);
        assert_eq!(output_kind(3, "HDAUDIO"), OutputKind::Headphones);
        assert_eq!(output_kind(5, "USB"), OutputKind::Headset);
        assert_eq!(output_kind(9, "HDAUDIO"), OutputKind::Display);
        assert_eq!(output_kind(8, "HDAUDIO"), OutputKind::Display);
        assert_eq!(output_kind(0, "SWD"), OutputKind::Other);
        // Bluetooth wins over the form factor (Buds report Headphones or Headset).
        assert_eq!(output_kind(3, "BTHENUM"), OutputKind::Bluetooth);
        assert_eq!(output_kind(5, "BTHHFENUM"), OutputKind::Bluetooth);
    }

    #[test]
    fn output_bus_from_enumerator() {
        assert_eq!(output_bus("BTHENUM"), OutputBus::Bluetooth);
        assert_eq!(output_bus("USB"), OutputBus::Usb);
        assert_eq!(output_bus("HDAUDIO"), OutputBus::Internal);
        assert_eq!(output_bus("ROOT"), OutputBus::Virtual);
        assert_eq!(output_bus("SWD"), OutputBus::Virtual);
    }

    fn dev(name: &str, kind: OutputKind, bus: OutputBus, default: bool) -> OutputDevice {
        OutputDevice { id: name.into(), name: name.into(), kind, bus, default }
    }

    /// A PC's devices: the current one first, then Bluetooth, USB, built-in, virtual, then by name.
    #[test]
    fn picker_order_default_then_bus_then_name() {
        use OutputBus::*;
        let d = vec![
            dev("SteelSeries Sonar - Gaming", OutputKind::Headphones, Virtual, true),
            dev("SteelSeries Sonar - Chat", OutputKind::Headset, Virtual, false),
            dev("Speaker", OutputKind::Speakers, Internal, false),
            dev("Ekin adlı kişiye ait Buds2", OutputKind::Bluetooth, Bluetooth, false),
            dev("SteelSeries Sonar - Aux", OutputKind::Headphones, Virtual, false),
        ];
        let names: Vec<&str> = picker_order(&d).iter().map(|&i| d[i].name.as_str()).collect();
        assert_eq!(names, ["SteelSeries Sonar - Gaming", "Ekin adlı kişiye ait Buds2", "Speaker", "SteelSeries Sonar - Aux", "SteelSeries Sonar - Chat"]);
    }
}
