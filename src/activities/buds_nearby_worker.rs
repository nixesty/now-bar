//! Listens to Bluetooth LE adverts while the Buds aren't connected to this PC (scanning while
//! they are would disturb their audio) and posts the lid opening `buds_nearby` sees in them.

use std::sync::{Arc, Mutex};

use windows::core::Result;
use windows::Devices::Bluetooth::Advertisement::{BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher, BluetoothLEScanningMode};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;

use super::buds_nearby::{lid_counter, Lid};
use crate::platform::log::log;

type Post = Arc<dyn Fn(u8, i16) + Send + Sync>;

pub struct NearbyWorker {
    watcher: Mutex<Option<(BluetoothLEAdvertisementWatcher, i64)>>,
    lid: Arc<Mutex<Lid>>,
    post: Post,
}

impl NearbyWorker {
    /// `last`: the lid counter remembered from before; `post` gets the new counter and the signal
    /// (dBm) on an advert thread when the lid opens.
    pub fn new(last: Option<u8>, post: impl Fn(u8, i16) + Send + Sync + 'static) -> Self {
        NearbyWorker { watcher: Mutex::new(None), lid: Arc::new(Mutex::new(Lid::new(last))), post: Arc::new(post) }
    }

    /// Starts or stops listening (nothing when it already is as asked).
    pub fn set_scanning(&self, on: bool) {
        let Ok(mut slot) = self.watcher.lock() else { return };
        match (on, slot.take()) {
            (true, None) => match self.start() {
                Ok(w) => {
                    log("buds: listening for the case");
                    *slot = Some(w);
                }
                Err(e) => log(&format!("buds: can't listen for the case: {e:?}")),
            },
            (false, Some((w, token))) => {
                let _ = w.Stop();
                let _ = w.RemoveReceived(token);
                log("buds: stopped listening for the case");
            }
            (_, kept) => *slot = kept,
        }
    }

    fn start(&self) -> Result<(BluetoothLEAdvertisementWatcher, i64)> {
        let watcher = BluetoothLEAdvertisementWatcher::new()?;
        // Active: the scan responses carry the name.
        watcher.SetScanningMode(BluetoothLEScanningMode::Active)?;
        let (lid, post) = (self.lid.clone(), self.post.clone());
        let token = watcher.Received(&TypedEventHandler::<BluetoothLEAdvertisementWatcher, BluetoothLEAdvertisementReceivedEventArgs>::new(move |_, args| {
            let args = args.ok()?;
            let adv = args.Advertisement()?;
            let name = adv.LocalName().map(|n| n.to_string()).unwrap_or_default();
            let mut counter = None;
            for md in adv.ManufacturerData()? {
                let data = md.Data()?;
                let mut bytes = vec![0u8; data.Length()? as usize];
                DataReader::FromBuffer(&data)?.ReadBytes(&mut bytes)?;
                counter = counter.or(lid_counter(md.CompanyId()?, &bytes));
            }
            let (rssi, address) = (args.RawSignalStrengthInDBm()?, args.BluetoothAddress()?);
            let opened = lid.lock().ok().and_then(|mut l| l.advert(address, &name, counter, rssi));
            if let Some(c) = opened {
                post(c, rssi);
            }
            Ok(())
        }))?;
        watcher.Start()?;
        Ok((watcher, token))
    }
}

impl Drop for NearbyWorker {
    fn drop(&mut self) {
        self.set_scanning(false);
    }
}
