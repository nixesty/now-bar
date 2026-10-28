//! The Bluetooth side of the Buds: finds the paired Buds by Samsung's SPP service (their name is
//! whatever the user called them, e.g. "Ekin adlı kişiye ait Buds2"), or the pair chosen in the
//! settings, follows their connection, reads the RFCOMM stream and posts battery snapshots.
//! Blocking WinRT calls stay on this thread.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::core::{AgileReference, Result, HSTRING};
use windows_future::AsyncStatus;
use windows::Devices::Bluetooth::Rfcomm::{RfcommDeviceService, RfcommServiceId};
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher};
use windows::Foundation::TypedEventHandler;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, IOutputStream, InputStreamOptions};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::buds::{
    address_text, buds_among, fit_frame, hold_frame, is_news, model_id, model_name, next_snapshot, parse_fit, set_noise_frame, take_frames, touch_frame, BudsSnapshot, Fit, HoldAction, NoiseControl, Paired, Touch, HELLO,
    SPP_STANDARD_UUID, SPP_UUID,
};
use crate::platform::log::log;

/// After a failed connection, retry in 5 s, 15 s, then every 60 s while the Buds stay connected.
const BACKOFF_S: [u64; 3] = [5, 15, 60];
const READ_CHUNK: u32 = 512;
/// The first frames are logged in hex, to check the parser against the real device.
const LOG_FRAMES: usize = 5;
/// A device that takes the connection but says nothing this long isn't Buds (or is stuck).
const FIRST_FRAME_S: u64 = 10;
/// WSAEADDRINUSE: another app (GalaxyBudsClient, Samsung's Galaxy Buds) holds the channel.
const IN_USE: u32 = 0x8007_2740;

enum Msg {
    /// A device offering the service was paired, unpaired or changed.
    Rescan,
    /// The Buds connected or disconnected.
    Changed,
}

/// The open RFCOMM connection's output, while there is one: requests go out through it (from
/// another thread, hence the agile reference).
type Out = Arc<Mutex<Option<AgileReference<IOutputStream>>>>;
/// The open connection itself, to drop it when another pair is chosen.
type Socket = Arc<Mutex<Option<AgileReference<StreamSocket>>>>;

pub struct BudsWorker {
    out: Out,
    socket: Socket,
    tx: Sender<Msg>,
    /// One request at a time: two quick clicks would write on the stream at once.
    writing: Arc<Mutex<()>>,
}

impl BudsWorker {
    /// `post` runs on the worker thread with each new reading (`None`: Buds not available),
    /// `fit` with each fit test result (left, right).
    pub fn start(post: impl Fn(Option<BudsSnapshot>) + Send + 'static, fit: impl Fn(Fit, Fit) + Send + 'static) -> Self {
        let (tx, rx) = channel::<Msg>();
        let (out, socket) = (Out::default(), Socket::default());
        let (shared, held, worker_tx) = (out.clone(), socket.clone(), tx.clone());
        std::thread::Builder::new()
            .name("buds".into())
            .spawn(move || {
                if let Err(e) = run(worker_tx, rx, post, fit, shared, held) {
                    log(&format!("buds worker stopped: {e:?}"));
                }
            })
            .expect("spawn buds thread");
        BudsWorker { out, socket, tx, writing: Arc::default() }
    }

    /// The pair chosen in the settings changed: looks again, dropping the connection open now
    /// (closing the socket ends its read; the queued look then picks the new pair).
    pub fn reselect(&self) {
        log("buds: the chosen pair changed");
        let _ = self.tx.send(Msg::Rescan);
        let Some(socket) = self.socket.lock().ok().and_then(|s| s.clone()) else { return };
        std::thread::spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            if let Ok(s) = socket.resolve() {
                let _ = s.Close();
            }
        });
    }

    /// Switches the Buds' noise control.
    pub fn set_noise(&self, mode: NoiseControl) {
        self.send(set_noise_frame(mode), format!("noise control → {mode:?}"));
    }

    /// Switches touches on or off, the gestures as `t` has them.
    pub fn set_touch(&self, t: &Touch, on: bool) {
        self.send(touch_frame(t, on), format!("touch controls → {}", if on { "on" } else { "off" }));
    }

    /// Sets touch and hold on both buds.
    pub fn set_hold(&self, action: HoldAction) {
        self.send(hold_frame(action), format!("touch and hold → {action:?}"));
    }

    /// Starts the fit test (the Buds play a tone, then say the result) or stops it.
    pub fn fit_test(&self, on: bool) {
        self.send(fit_frame(on), format!("fit test → {}", if on { "start" } else { "stop" }));
    }

    /// Writes a request on a short thread of its own (the write waits for the Bluetooth stack).
    /// Nothing happens while the Buds aren't connected.
    fn send(&self, frame: Vec<u8>, what: String) {
        let Some(stream) = self.out.lock().ok().and_then(|o| o.clone()) else { return };
        let writing = self.writing.clone();
        std::thread::spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let _turn = writing.lock();
            let sent = stream.resolve().and_then(|stream| DataWriter::CreateDataWriter(&stream)).and_then(|w| {
                w.WriteBytes(&frame)?;
                w.StoreAsync()?.join()?;
                w.DetachStream().map(|_| ())
            });
            match sent {
                Ok(()) => log(&format!("buds: {what}")),
                Err(e) => log(&format!("buds: not sent ({what}): {e:?}")),
            }
        });
    }
}

/// The paired Buds: their SPP service and device, subscribed to connection changes.
struct Found {
    service: RfcommDeviceService,
    device: BluetoothDevice,
    token: i64,
}

impl Drop for Found {
    fn drop(&mut self) {
        let _ = self.device.RemoveConnectionStatusChanged(self.token);
    }
}

fn connected(device: &BluetoothDevice) -> bool {
    device.ConnectionStatus().is_ok_and(|s| s == BluetoothConnectionStatus::Connected)
}

/// Samsung's own service (Buds2 and later), and the standard serial one (Buds Pro).
fn selectors() -> Result<[HSTRING; 2]> {
    Ok([
        RfcommDeviceService::GetDeviceSelector(&RfcommServiceId::FromUuid(SPP_UUID)?)?,
        RfcommDeviceService::GetDeviceSelector(&RfcommServiceId::FromUuid(SPP_STANDARD_UUID)?)?,
    ])
}

fn run(tx: Sender<Msg>, rx: Receiver<Msg>, post: impl Fn(Option<BudsSnapshot>), fit: impl Fn(Fit, Fit), out: Out, socket: Socket) -> Result<()> {
    unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
    let selectors = selectors()?;
    let mut watchers = Vec::new();
    for selector in &selectors {
        let watcher = DeviceInformation::CreateWatcherAqsFilter(selector)?;
        let rescan = |tx: &Sender<Msg>| {
            let tx = tx.clone();
            move || {
                let _ = tx.send(Msg::Rescan);
            }
        };
        let added = rescan(&tx);
        watcher.Added(&TypedEventHandler::<DeviceWatcher, DeviceInformation>::new(move |_, _| {
            added();
            Ok(())
        }))?;
        let removed = rescan(&tx);
        watcher.Removed(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(move |_, _| {
            removed();
            Ok(())
        }))?;
        // Updates arrive only with an Updated handler registered; they don't change what we use.
        watcher.Updated(&TypedEventHandler::<DeviceWatcher, DeviceInformationUpdate>::new(|_, _| Ok(())))?;
        watcher.Start()?;
        watchers.push(watcher);
    }

    let mut found: Vec<Found> = Vec::new();
    let mut failures = 0usize;
    let mut retry_at: Option<Instant> = None;
    let mut in_use_said = false;
    let mut pending = Some(Msg::Rescan);
    loop {
        let msg = match pending.take() {
            Some(m) => Ok(m),
            None => {
                let wait = retry_at.map_or(Duration::from_secs(3600), |t| t.saturating_duration_since(Instant::now()));
                rx.recv_timeout(wait)
            }
        };
        match msg {
            Ok(Msg::Rescan) => {
                // Each watcher adds every paired device one by one at start: one scan for the lot.
                loop {
                    match rx.recv_timeout(Duration::from_millis(300)) {
                        Ok(Msg::Rescan) => continue,
                        Ok(other) => pending = Some(other),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return Ok(()),
                    }
                    break;
                }
                found = find(&selectors, &tx);
                failures = 0;
                retry_at = None;
            }
            // A new connection gets a fresh try at once.
            Ok(Msg::Changed) => {
                failures = 0;
                retry_at = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
        // Whichever pair is connected now (two pairs can be paired; either may connect).
        let Some(f) = found.iter().find(|f| connected(&f.device)) else {
            // None connected: wait for a connection, not a retry time (no spinning on a stale one).
            retry_at = None;
            continue;
        };
        if retry_at.is_some_and(|t| Instant::now() < t) {
            continue;
        }
        let result = read_until_closed(&f.service, &post, &fit, &out, &socket);
        if let Ok(mut o) = out.lock() {
            *o = None;
        }
        if let Ok(mut s) = socket.lock() {
            *s = None;
        }
        post(None);
        if !connected(&f.device) {
            // The Buds went away; their next connection brings us back.
            retry_at = None;
            continue;
        }
        match result {
            Ok(()) => in_use_said = false,
            Err(e) if e.code().0 as u32 == IN_USE => {
                if !in_use_said {
                    log("buds: another app (GalaxyBudsClient, Samsung's Galaxy Buds) is using the Buds; trying again now and then");
                    in_use_said = true;
                }
                failures += 1;
            }
            Err(e) => {
                log(&format!("buds: RFCOMM refused or silent: {e:?}"));
                failures += 1;
            }
        }
        retry_at = Some(Instant::now() + Duration::from_secs(BACKOFF_S[failures.clamp(1, BACKOFF_S.len()) - 1]));
    }
}

/// A paired device offering one of the services, with what it takes to use it.
struct Candidate {
    paired: Paired,
    service: RfcommDeviceService,
    device: BluetoothDevice,
}

/// Every paired device offering Samsung's service or the standard serial one, once each
/// (Samsung's is asked first, so it wins for a device offering both).
fn candidates(selectors: &[HSTRING; 2]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for (i, selector) in selectors.iter().enumerate() {
        let Ok(infos) = DeviceInformation::FindAllAsyncAqsFilter(selector).and_then(|op| op.join()) else { continue };
        for info in &infos {
            let Ok(id) = info.Id() else { continue };
            // Null (an error here) when Windows denies access to the service.
            let Ok(service) = RfcommDeviceService::FromIdAsync(&id).and_then(|op| op.join()) else { continue };
            let Ok(device) = service.Device() else { continue };
            let address = device.BluetoothAddress().map(address_text).unwrap_or_default();
            if out.iter().any(|c| c.paired.address == address) {
                continue;
            }
            let name = device.Name().map(|n| n.to_string()).unwrap_or_default();
            out.push(Candidate { paired: Paired { address, name, samsung: i == 0 }, service, device });
        }
    }
    out
}

/// The paired devices that may be the Buds, for the settings window's list. Blocks: call it off
/// the UI thread.
pub fn paired() -> Vec<Paired> {
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    selectors().map(|s| candidates(&s).into_iter().map(|c| c.paired).collect()).unwrap_or_default()
}

/// The Buds among the paired devices (the pair chosen in the settings, else every one that looks
/// like Buds), each subscribed to its connection changes.
fn find(selectors: &[HSTRING; 2], tx: &Sender<Msg>) -> Vec<Found> {
    let all = candidates(selectors);
    let paired: Vec<Paired> = all.iter().map(|c| c.paired.clone()).collect();
    let chosen = crate::settings::current().buds_device.as_ref().map(|d| d.address.clone());
    if chosen.as_ref().is_some_and(|c| !paired.iter().any(|p| p.address.eq_ignore_ascii_case(c))) {
        log("buds: the chosen pair isn't paired now; any Buds will do");
    }
    let picked = buds_among(&paired, chosen.as_deref());
    let mut found: Vec<Found> = Vec::new();
    for (i, c) in all.into_iter().enumerate().filter(|(i, _)| picked.contains(i)) {
        let tx = tx.clone();
        let Ok(token) = c.device.ConnectionStatusChanged(&TypedEventHandler::new(move |_, _| {
            let _ = tx.send(Msg::Changed);
            Ok(())
        })) else {
            continue;
        };
        log(&format!("buds: found {:?}, connected {}{}", c.paired.name, connected(&c.device), if chosen.is_some() && picked.len() == 1 && picked[0] == i { " (chosen)" } else { "" }));
        found.push(Found { service: c.service, device: c.device, token });
    }
    if found.is_empty() {
        log("buds: no paired Galaxy Buds (Samsung's SPP service, or Buds Pro)");
    }
    found
}

/// Connects, then posts every reading and fit test result until the stream ends (`Ok`) or fails
/// (`Err`).
fn read_until_closed(service: &RfcommDeviceService, post: &impl Fn(Option<BudsSnapshot>), fit: &impl Fn(Fit, Fit), out: &Out, held: &Socket) -> Result<()> {
    let socket = StreamSocket::new()?;
    socket
        .ConnectWithProtectionLevelAsync(&service.ConnectionHostName()?, &service.ConnectionServiceName()?, service.ProtectionLevel()?)?
        .join()?;
    log("buds: RFCOMM connected");
    if let Ok(mut o) = out.lock() {
        *o = socket.OutputStream().and_then(|s| AgileReference::new(&s)).ok();
    }
    if let Ok(mut s) = held.lock() {
        *s = AgileReference::new(&socket).ok();
    }
    if !HELLO.is_empty() {
        let writer = DataWriter::CreateDataWriter(&socket.OutputStream()?)?;
        writer.WriteBytes(HELLO)?;
        writer.StoreAsync()?.join()?;
        let _ = writer.DetachStream();
    }
    let reader = DataReader::CreateDataReader(&socket.InputStream()?)?;
    reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
    let mut buf = Vec::new();
    let mut logged = 0;
    let mut last: Option<BudsSnapshot> = None;
    let opened = Instant::now();
    loop {
        let op = reader.LoadAsync(READ_CHUNK)?;
        // Until the first reading: a device that takes the connection but says nothing would
        // hold this thread forever.
        let n = if last.is_some() {
            op.join()?
        } else {
            while op.Status()? == AsyncStatus::Started {
                if opened.elapsed() > Duration::from_secs(FIRST_FRAME_S) {
                    let _ = op.Cancel();
                    return Err(windows::core::Error::new(windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult(), "no reading in 10 s"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            op.GetResults()?
        };
        if n == 0 {
            return Ok(());
        }
        let mut chunk = vec![0u8; n as usize];
        reader.ReadBytes(&mut chunk)?;
        buf.extend_from_slice(&chunk);
        for frame in take_frames(&mut buf) {
            // The answers to the card's touch and fit requests are logged too: they're rare, and
            // the only check of those requests against the real Buds.
            let news = is_news(&frame);
            if logged < LOG_FRAMES || news {
                let hex: Vec<String> = frame.payload.iter().map(|b| format!("{b:02X}")).collect();
                log(&format!("buds frame 0x{:02X}: {}", frame.id, hex.join(" ")));
                logged += usize::from(!news);
            }
            if let Some((left, right)) = parse_fit(&frame) {
                fit(left, right);
            }
            if let (None, Some(id)) = (last, model_id(&frame)) {
                log(&format!("buds: model {id} ({})", model_name(id)));
            }
            let next = next_snapshot(&frame, last);
            if let Some(s) = next.filter(|s| last != Some(*s)) {
                post(Some(s));
                last = Some(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Lists this PC's paired devices that may be Buds: `cargo t --lib live_buds_paired -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_buds_paired() {
        let paired = super::paired();
        let chosen = crate::settings::current().buds_device.clone();
        println!("paired: {paired:?}");
        println!("buds: {:?}", super::buds_among(&paired, chosen.as_ref().map(|d| d.address.as_str())));
    }
}
