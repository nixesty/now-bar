//! Buds2Popup in Pebbly: while the Buds aren't connected here, opening their case offers to
//! connect them (the pill, and the card opening by itself); the audio is asked to reconnect, and
//! once the Buds talk the card is the manager. The lid counter outlives a restart in
//! `buds-lid.txt` beside `place.json`.

use super::*;
use crate::activities::buds::model_name;
use crate::activities::buds_nearby::{should_offer, Offer, SETTLE_MS};
use crate::activities::buds_nearby_worker::NearbyWorker;
use crate::render::card::{bud_art, BudsCard};

fn lid_path() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(crate::DIR).join("buds-lid.txt")
}

/// The lid counter last seen, from before a restart.
pub(super) fn read_lid() -> Option<u8> {
    std::fs::read_to_string(lid_path()).ok()?.trim().parse().ok()
}

fn write_lid(counter: u8) {
    let _ = std::fs::write(lid_path(), counter.to_string());
}

/// The listener for the case, posting into the Buds' news.
pub(super) fn nearby_worker(mailbox: std::sync::Arc<std::sync::Mutex<Vec<BudsNews>>>) -> NearbyWorker {
    NearbyWorker::new(read_lid(), move |counter, rssi| {
        if let Ok(mut slot) = mailbox.lock() {
            slot.push(BudsNews::LidOpened { counter, rssi });
        }
        post_buds();
    })
}

/// The name the offer says: the Buds last connected, else just "Galaxy Buds".
fn offer_title(model: Option<u16>) -> &'static str {
    model.map(model_name).filter(|n| n.starts_with("Galaxy")).unwrap_or("Galaxy Buds")
}

impl App {
    /// The offer's news: where the Buds are, the case opening, the audio request's answer.
    pub(super) fn take_offer_news(&mut self, news: BudsNews, now: u64) {
        match news {
            BudsNews::Link { address, connected } => {
                if connected && !self.buds_linked && matches!(self.buds_offer, Some(Offer::Asking { .. } | Offer::Connecting { .. })) {
                    self.buds_offer = Some(Offer::Connected { at: now });
                }
                log(&format!("buds: {} this PC", if connected { "connected to" } else { "not connected to" }));
                self.buds_address = address;
                self.buds_linked = connected;
            }
            BudsNews::LidOpened { counter, rssi } => {
                write_lid(counter);
                let s = settings::current();
                log(&format!("buds: the case opened ({rssi} dBm)"));
                // They may connect by themselves: the offer waits a moment.
                if s.buds_enabled && s.buds_offer && !self.buds_linked {
                    unsafe { SetTimer(Some(self.main.hwnd), TIMER_BUDS_OFFER, SETTLE_MS as u32, None) };
                }
            }
            BudsNews::Sent(result) => match result {
                Ok(()) => log("buds: audio request sent"),
                Err(e) => {
                    log(&format!("buds: audio request failed: {e}"));
                    if matches!(self.buds_offer, Some(Offer::Connecting { .. })) {
                        self.buds_offer = Some(Offer::Failed { at: now });
                    }
                }
            },
            _ => {}
        }
    }

    /// A moment after the case opened: the offer, unless the Buds came by themselves. The card
    /// opens by itself as Buds2Popup's window did (not over another card or a full-screen game)
    /// and goes after 20 s untouched, as the morning brief does.
    pub(super) fn offer_buds(&mut self) -> Result<()> {
        unsafe {
            let _ = KillTimer(Some(self.main.hwnd), TIMER_BUDS_OFFER);
        }
        let now = now_ms();
        if !should_offer(self.buds_linked || self.buds.is_some(), self.buds_offer.and_then(|o| o.at(now)), self.buds_offer_last, now) {
            return Ok(());
        }
        log("buds: offering to connect");
        self.buds_offer = Some(Offer::Asking { since: now });
        self.buds_offer_last = Some(now);
        self.refresh()?;
        if self.card_state == CardState::Closed && !self.fullscreen {
            self.open_card(OpenCard::Activity(ActivityKind::Buds), false)?;
            if self.open == Some(OpenCard::Activity(ActivityKind::Buds)) {
                self.brief_auto = true;
                unsafe { SetTimer(Some(self.main.hwnd), TIMER_BRIEF_IDLE, BRIEF_IDLE_MS, None) };
            }
        }
        Ok(())
    }

    /// The offer card's and the Sistem page's buttons.
    pub(super) fn offer_act(&mut self, action: CardAction) -> Result<()> {
        let now = now_ms();
        match action {
            CardAction::BudsConnect => match self.buds_address.clone() {
                Some(address) => {
                    log("buds: connecting the audio");
                    self.buds_offer = Some(Offer::Connecting { since: now });
                    self.send_audio(address, true);
                }
                None => self.buds_offer = Some(Offer::Failed { at: now }),
            },
            CardAction::BudsDismiss => {
                self.buds_offer = None;
                return self.close_card();
            }
            CardAction::BudsDisconnect => {
                if let Some(address) = self.buds_address.clone() {
                    log("buds: disconnecting the audio");
                    self.send_audio(address, false);
                }
                return self.close_card();
            }
            _ => {}
        }
        self.refresh()
    }

    /// Asks the audio to reconnect or disconnect on a short thread (the driver takes its time).
    fn send_audio(&self, address: String, connect: bool) {
        let mailbox = self.buds_news.clone();
        std::thread::spawn(move || {
            let result = if connect { crate::platform::bt_audio::reconnect(&address) } else { crate::platform::bt_audio::disconnect(&address) };
            if let Ok(mut slot) = mailbox.lock() {
                slot.push(BudsNews::Sent(result));
            }
            post_buds();
        });
    }

    /// At each refresh: an offer whose time is up goes or fails, and the case is listened for
    /// only while the paired Buds are away (and not while connecting).
    pub(super) fn tick_offer(&mut self, now: u64) {
        self.buds_offer = self.buds_offer.and_then(|o| o.at(now));
        let s = settings::current();
        let connecting = matches!(self.buds_offer, Some(Offer::Connecting { .. }));
        self.nearby.set_scanning(s.buds_enabled && s.buds_offer && self.buds_address.is_some() && !self.buds_linked && self.buds.is_none() && !connecting);
    }

    /// When the offer next changes by itself (ms from now).
    pub(super) fn offer_wake_in(&self, now: u64) -> Option<u64> {
        use crate::activities::buds_nearby::{CONNECTED_MS, CONNECT_MS, FAILED_MS, OFFER_MS};
        let until = match self.buds_offer? {
            Offer::Asking { since } => since + OFFER_MS,
            Offer::Connecting { since } => since + CONNECT_MS,
            Offer::Connected { at } => at + CONNECTED_MS,
            Offer::Failed { at } => at + FAILED_MS,
        };
        Some(until.saturating_sub(now) + 5)
    }

    /// The pill while the offer stands and the Buds aren't talking yet.
    pub(super) fn offer_pill(&self, now: u64) -> Option<PillContent> {
        let offer = self.buds_offer?.at(now)?;
        let model = settings::current().buds_model;
        let px = (19.0 * self.ui.pill_scale.0).round() as u32;
        Some(PillContent::BudsOffer { title: offer_title(model).into(), offer, art: bud_art(model, 0, px) })
    }

    /// The offer card, the Buds drawn as they were last seen.
    pub(super) fn offer_card(&self, now: u64) -> Option<CardContent> {
        let offer = self.buds_offer?.at(now)?;
        let b = BudsSnapshot { model: settings::current().buds_model, ..Default::default() };
        Some(CardContent::Buds(BudsCard { b, page: BudsPage::Offer, offer: Some(offer), ..Default::default() }))
    }

    /// The Buds that just connected, remembered for the next offer's pictures.
    pub(super) fn remember_buds_model(&self, model: Option<u16>) {
        let Some(model) = model else { return };
        if settings::current().buds_model == Some(model) {
            return;
        }
        let s = settings::with_buds_model(settings::try_load(), &settings::current(), model);
        if let Err(e) = settings::save(&s) {
            log(&format!("settings save: {e:?}"));
        }
        settings::install(s);
    }
}
