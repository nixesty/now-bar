//! The Buds card's side of the app: what the Buds worker reports, the card's pages and what their
//! buttons do, the fit test and find my earbuds over time, a long press for the PC.

use super::*;
use crate::activities::buds::{fit_can_press, Fit, FitTest};
use crate::activities::buds_controls::{drain_level, find_can_start, rated_remaining_min, remaining_text, Side, FIND_MAX_MS};
use crate::activities::buds::FIT_TIMEOUT_MS;
use crate::render::card::BudsCard;
use crate::settings::BudsPcAction;

/// While the Buds card is open, their temperatures and voltages are asked this often (GBC: 12 s).
const HEALTH_EVERY_MS: u64 = 12_000;

impl App {
    pub(super) fn buds_card(&self, b: BudsSnapshot, page: BudsPage, now: u64) -> BudsCard {
        BudsCard {
            b: shown_buds(b),
            fit: self.buds_fit.expired(now),
            page,
            health: self.buds_health,
            firmware: self.buds_firmware.clone(),
            capacity: self.buds_capacity,
            // Measured once there is a steady fall; Samsung's figures until then.
            remaining: self.buds_drain.remaining_min().or_else(|| rated_remaining_min(&b)).map(remaining_text),
            finding: self.buds_find.running(now),
            muted: self.buds_find.muted,
            pc_action: settings::current().buds_pc_action,
            offer: None,
        }
    }

    /// Takes what the Buds worker reported since.
    pub(super) fn take_buds(&mut self) -> Result<()> {
        let news = self.buds_news.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default();
        if news.is_empty() {
            return Ok(());
        }
        let now = now_ms();
        for n in news {
            match n {
                BudsNews::Reading(reading) => self.take_buds_reading(reading, now),
                BudsNews::Fit(left, right) => self.take_fit(left, right, now),
                BudsNews::Health(h) => self.buds_health = Some(h),
                BudsNews::Firmware(f) => self.buds_firmware = Some(f),
                BudsNews::Capacity(c) => self.buds_capacity = Some(c),
                BudsNews::FindStopped => {
                    if self.buds_find.since_ms.is_some() {
                        log("buds: they stopped ringing");
                    }
                    self.buds_find = FindState::default();
                }
                BudsNews::Muted(m) => self.buds_find.muted = m,
                BudsNews::HoldPressed(side) => self.on_pc_hold(side),
                n @ (BudsNews::Link { .. } | BudsNews::LidOpened { .. } | BudsNews::Sent(_)) => self.take_offer_news(n, now),
            }
        }
        self.refresh()
    }

    /// A reading; the first marks the moment they connected, none means they went.
    fn take_buds_reading(&mut self, reading: Option<BudsSnapshot>, now: u64) {
        let connected_ms = self.buds.map_or(now, |(_, at)| at);
        let before = self.buds.and_then(|(b, _)| b.noise);
        if let Some(mode) = reading.and_then(|b| b.noise).filter(|m| before.is_some_and(|b| b != *m)) {
            // Switched on the Buds or the phone (the card's own switch echoes back): a moment,
            // unless the card is open on the Buds.
            let echo = self.buds_mode_sent.is_some_and(|(sent, at)| sent == mode && now.saturating_sub(at) < 5000);
            if !echo && self.open != Some(OpenCard::Activity(ActivityKind::Buds)) {
                self.buds_mode_moment = Some((mode, now));
            }
        }
        if let (Some((old, _)), Some(new)) = (self.buds, reading) {
            if old.worn != new.worn {
                log(&format!("buds: worn L {} R {} (was L {} R {})", new.worn.left, new.worn.right, old.worn.left, old.worn.right));
                self.follow_ears(old.worn, new.worn, now);
            }
        }
        match reading {
            Some(b) => {
                // They're talking: an offer is answered, and the model is kept for the next.
                self.buds_offer = None;
                self.remember_buds_model(b.model);
                // Ringing is loud: a bud going into an ear stops it.
                if self.buds_find.since_ms.is_some() && b.worn.count() > 0 {
                    self.stop_finding("a bud went in");
                }
                self.buds_drain.note(now, drain_level(&b));
            }
            None => {
                self.buds_fit = FitTest::Idle;
                self.buds_find = FindState::default();
                self.buds_health = None;
                self.buds_firmware = None;
                self.buds_capacity = None;
                self.buds_drain = Drain::default();
                self.buds_page = BudsPage::Home;
                self.buds_page_pending = None;
            }
        }
        self.buds = reading.map(|b| (b, connected_ms));
    }

    fn take_fit(&mut self, left: Fit, right: Fit, now: u64) {
        log(&format!("buds: fit L {left:?} R {right:?}"));
        // The Buds stop their tone on a stop request (Galaxy Buds Manager sends one too).
        if matches!(self.buds_fit.expired(now), FitTest::Running { .. }) {
            self.buds_worker.fit_test(false);
            self.buds_fit = FitTest::Done(left, right);
        }
    }

    /// The card's buttons. Changes show at once; the Buds' next extended status says what's true.
    pub(super) fn buds_act(&mut self, action: CardAction) -> Result<()> {
        let now = now_ms();
        match action {
            CardAction::BudsPage(page) => return self.turn_buds_page(page),
            CardAction::BudsBack => return self.turn_buds_page(BudsPage::Home),
            CardAction::BudsTouch => {
                if let Some(t) = self.buds.as_mut().and_then(|(b, _)| b.touch.as_mut()) {
                    t.on = !t.on;
                    self.buds_worker.set_touch(t, t.on);
                }
            }
            CardAction::BudsHold(hold) => {
                if let Some(t) = self.buds.as_mut().and_then(|(b, _)| b.touch.as_mut()) {
                    t.hold = hold.bytes();
                    self.buds_worker.set_hold(hold);
                }
            }
            CardAction::BudsFit => {
                let fit = self.buds_fit.expired(now);
                if matches!(fit, FitTest::Running { .. }) {
                    self.buds_worker.fit_test(false);
                    self.buds_fit = FitTest::Idle;
                } else if self.buds.is_some_and(|(b, _)| fit_can_press(fit, b.worn)) {
                    self.buds_fit = FitTest::Running { since_ms: now };
                    self.buds_worker.fit_test(true);
                }
            }
            CardAction::BudsEq(e) => {
                if let Some(s) = self.buds.as_mut().and_then(|(b, _)| b.settings.as_mut()) {
                    s.eq = e;
                    self.buds_worker.set_eq(e);
                }
            }
            CardAction::BudsAmbient(level) => {
                if let Some(s) = self.buds.as_mut().and_then(|(b, _)| b.settings.as_mut()) {
                    s.ambient = level;
                    self.buds_worker.set_ambient(level);
                }
            }
            CardAction::BudsCustom(c) => {
                if let Some(s) = self.buds.as_mut().and_then(|(b, _)| b.settings.as_mut()) {
                    s.custom = Some(c);
                    self.buds_worker.set_custom(c);
                }
            }
            CardAction::BudsToggle(t, on) => {
                if let Some(s) = self.buds.as_mut().and_then(|(b, _)| b.settings.as_mut()) {
                    *s = s.with(t, on);
                    self.buds_worker.set_toggle(t, on);
                }
            }
            CardAction::BudsFind => {
                if self.buds_find.running(now) {
                    self.stop_finding("the card");
                } else if self.buds.is_some_and(|(b, _)| find_can_start(b.worn)) {
                    self.buds_find = FindState { since_ms: Some(now), muted: [false; 2] };
                    self.buds_worker.find(true);
                }
            }
            CardAction::BudsMute(side) => {
                let i = usize::from(side.min(1));
                self.buds_find.muted[i] = !self.buds_find.muted[i];
                self.buds_worker.mute(self.buds_find.muted);
            }
            CardAction::BudsPc(a) => {
                log(&format!("buds: a long press for the PC → {a:?}"));
                let s = settings::with_buds_pc_action(settings::try_load(), &settings::current(), a);
                if let Err(e) = settings::save(&s) {
                    log(&format!("settings save: {e:?}"));
                }
                settings::install(s);
            }
            _ => {}
        }
        self.refresh()
    }

    /// Slides the Buds card's content out and, `PAGE_OUT_MS` later, shows `page`; the card springs
    /// to its size at once, as switching cards does.
    fn turn_buds_page(&mut self, page: BudsPage) -> Result<()> {
        if self.buds_page == page || self.buds_page_pending.is_some() || self.open != Some(OpenCard::Activity(ActivityKind::Buds)) {
            return Ok(());
        }
        let now = now_ms();
        if let (Some((b, _)), CardState::Open) = (self.buds, self.card_state) {
            let next = CardContent::Buds(self.buds_card(b, page, now));
            self.fit_open_card(&next)?;
        }
        let slide = self.ui.card_scale.0 * PAGE_SLIDE_DIP * if page == BudsPage::Home { 1.0 } else { -1.0 };
        self.ui.morph.page_turn(slide)?;
        self.buds_page_pending = Some(page);
        unsafe { SetTimer(Some(self.ui.card_window.timer_owner()), TIMER_PAGE, PAGE_OUT_MS as u32, None) };
        Ok(())
    }

    /// Halfway through a page turn: the new page comes in.
    pub(super) fn apply_buds_page(&mut self, page: BudsPage) -> Result<()> {
        self.buds_page = page;
        self.refresh()
    }

    /// The card closed or shows something else: the fit test and the ringing stop, and it opens
    /// on its home page next time.
    pub(super) fn leave_buds_card(&mut self) {
        if matches!(self.buds_fit, FitTest::Running { .. }) {
            self.buds_worker.fit_test(false);
        }
        self.buds_fit = FitTest::Idle;
        if self.buds_find.since_ms.is_some() {
            self.stop_finding("the card closed");
        }
        self.buds_page = BudsPage::Home;
        self.buds_page_pending = None;
    }

    fn stop_finding(&mut self, why: &str) {
        log(&format!("buds: stop ringing ({why})"));
        self.buds_worker.find(false);
        self.buds_find = FindState::default();
    }

    /// What the Buds card's time does at each refresh: a fit test without a result gives up (the
    /// tone stops too), ringing stops after `FIND_MAX_MS`, an open card asks for their health.
    pub(super) fn tick_buds(&mut self, now: u64) {
        if matches!(self.buds_fit, FitTest::Running { .. }) && self.buds_fit.expired(now) == FitTest::TimedOut {
            log("buds: fit test, no result");
            self.buds_worker.fit_test(false);
            self.buds_fit = FitTest::TimedOut;
        }
        if self.buds_find.since_ms.is_some() && !self.buds_find.running(now) {
            self.stop_finding("3 min");
        }
        if self.buds_card_open() && now.saturating_sub(self.buds_health_asked) >= HEALTH_EVERY_MS {
            self.buds_health_asked = now;
            self.buds_worker.ask_health();
        }
    }

    /// When the Buds card's time next needs a refresh (ms from now).
    pub(super) fn buds_wake_in(&self, now: u64) -> Option<u64> {
        let fit = match self.buds_fit {
            FitTest::Running { since_ms } => Some((since_ms + FIT_TIMEOUT_MS).saturating_sub(now) + 5),
            _ => None,
        };
        let find = self.buds_find.since_ms.map(|s| (s + FIND_MAX_MS).saturating_sub(now) + 5);
        let health = self.buds_card_open().then(|| (self.buds_health_asked + HEALTH_EVERY_MS).saturating_sub(now) + 5);
        [fit, find, health].into_iter().flatten().min()
    }

    fn buds_card_open(&self) -> bool {
        self.open == Some(OpenCard::Activity(ActivityKind::Buds)) && matches!(self.card_state, CardState::Open | CardState::Opening)
    }

    /// A long press on a bud whose touch and hold is the PC's: what the settings say.
    fn on_pc_hold(&mut self, side: Side) {
        let action = settings::current().buds_pc_action;
        log(&format!("buds: long press ({side:?}) → {action:?}"));
        match action {
            BudsPcAction::MicMute => match crate::platform::audio::toggle_mic_mute() {
                Ok(muted) => log(&format!("buds: microphone {}", if muted { "muted" } else { "on" })),
                Err(e) => log(&format!("buds: microphone not switched: {e:?}")),
            },
            BudsPcAction::PlayPause => {
                let _ = self.media_command(MediaCommand::PlayPause);
            }
            BudsPcAction::Next => {
                let _ = self.media_command(MediaCommand::Next);
            }
            BudsPcAction::Screenshot => open_url("ms-screenclip:"),
        }
    }
}
