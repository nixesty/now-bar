//! What is live right now, and which activity is at the front of Pebbly's stack.

pub mod api;
pub mod api_desk;
pub mod question;
pub mod question_worker;
pub mod away;
pub mod basketball;
pub mod basketball_worker;
pub mod brief;
pub mod buds;
pub mod buds_controls;
pub mod buds_nearby;
pub mod buds_nearby_worker;
pub mod buds_worker;
pub mod capture;
pub mod claude;
pub mod media;
pub mod media_worker;
pub mod music;
pub mod football;
pub mod football_worker;
pub mod games;
pub mod games_worker;
pub mod island;
pub mod live;
pub mod lyrics;
pub mod lyrics_worker;
pub mod power;
pub mod shelf;
pub mod shelf_tools;
pub mod shelf_tools_worker;
pub mod shelf_worker;
pub mod timer;
pub mod transfer;
pub mod transfer_worker;
pub mod weather;
pub mod weather_alert;
pub mod weather_worker;
pub mod wellbeing;
pub mod wellbeing_worker;
pub mod zip;

use std::cmp::Reverse;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActivityKind {
    /// One media session (the media worker's stable id for it): each is its own stack item.
    Media(u32),
    /// One followed football match (its ESPN event id).
    Match(u32),
    Timer,
    Stopwatch,
    Buds,
    /// The volume changed: the level, for a moment.
    Volume,
    /// Plugged in, or the battery ran low: for a moment.
    Power,
    /// Apps are using the microphone or the camera.
    Privacy,
    /// A Bluetooth headset other than the Buds (a hash of its address).
    Headset(u32),
    /// Files dropped on the pill, until removed.
    Shelf,
    /// Nothing else on: the clock and the weather.
    Clock,
    /// The picked city's weather, at the back of the stack while other things are on.
    Weather,
    /// Rain or snow is about to start: for a moment.
    Rain,
    /// Thunder, a gale, heat, frost or bad air ahead: for a moment (one after another).
    WeatherAlert,
    /// A screenshot was just taken: for a moment.
    Capture,
    /// Sent by another program (`pebbly.exe push`, Claude Code's hooks): the key of its id.
    Live(u32),
    /// A Steam or Epic game downloading or running, or its moment ("Güncellendi", how long it
    /// was played): its key.
    Game(u32),
    /// An app went past its daily limit (Dijital Denge): for a moment.
    Usage,
    /// Back from a lock: "Sen yokken · 4 şey", for a moment.
    Away,
    /// Explorer copying, moving or deleting (its dialog, minimised or not), and its end for a moment.
    Transfer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceState {
    Playing,
    Paused,
    Running,
    Done,
    Connected { battery_low: bool },
    /// A goal or red card moment is playing (a few seconds).
    MatchMoment,
    /// Kick-off to the final whistle (half time included).
    MatchLive,
    /// Before kick-off, or the result after the final whistle.
    MatchUpcoming,
    /// A short-lived notice (the volume level).
    Moment,
    /// A live activity waits for the user (Claude asks for a permission).
    Waiting,
}

/// One live activity. `changed_ms` is the `GetTickCount64` time of its last state change.
#[derive(Clone, Copy, Debug)]
pub struct Presence {
    pub kind: ActivityKind,
    pub state: PresenceState,
    pub changed_ms: u64,
}

/// The settings window's page for an activity (a right-click on its pill opens it):
/// 2 Medya, 3 Zamanlayıcı, 4 Galaxy Buds, 5 Canlı maç, 8 Sistem, 9 Oyunlar, 10 Dijital Denge.
pub fn settings_page_for(kind: ActivityKind) -> u8 {
    match kind {
        ActivityKind::Media(_) => 2,
        ActivityKind::Timer | ActivityKind::Stopwatch => 3,
        ActivityKind::Buds => 4,
        ActivityKind::Match(_) => 5,
        ActivityKind::Volume
        | ActivityKind::Power
        | ActivityKind::Privacy
        | ActivityKind::Headset(_)
        | ActivityKind::Shelf
        | ActivityKind::Clock
        | ActivityKind::Weather
        | ActivityKind::Rain
        | ActivityKind::WeatherAlert
        | ActivityKind::Capture
        | ActivityKind::Away
        | ActivityKind::Transfer
        | ActivityKind::Live(_) => 8,
        ActivityKind::Game(_) => 9,
        ActivityKind::Usage => 10,
    }
}

/// Whether the activity opens a card (the moments and the system activities only show in the
/// pill): with a card open, the wheel steps through these only.
pub fn has_card(kind: ActivityKind) -> bool {
    !matches!(
        kind,
        ActivityKind::Volume
            | ActivityKind::Power
            | ActivityKind::Privacy
            | ActivityKind::Headset(_)
            | ActivityKind::Rain
            | ActivityKind::WeatherAlert
            | ActivityKind::Capture
            | ActivityKind::Usage
            | ActivityKind::Away
            | ActivityKind::Transfer
    )
}

/// A noise control switch made on the Buds (or the phone) shows this long.
pub const BUDS_MODE_MOMENT_MS: u64 = 2000;

/// The volume level stays in front this long after a change.
pub const VOLUME_MOMENT_MS: u64 = 1500;

/// Rain about to start stays in front this long.
pub const RAIN_MOMENT_MS: u64 = 6000;

/// Each weather alert stays in front this long.
pub const ALERT_MOMENT_MS: u64 = 6000;

/// A game updated or played stays in front this long.
pub const GAME_MOMENT_MS: u64 = 6000;

/// A screen time limit crossed stays in front this long.
pub const USAGE_MOMENT_MS: u64 = 6000;

/// Paused media stays in the stack this long, then disappears (settings; 0 = until it closes).
pub fn media_paused_keep_ms() -> u64 {
    match crate::settings::current().media_paused_keep_min {
        0 => u64::MAX / 4,
        m => m as u64 * 60_000,
    }
}

/// After connecting, the Buds come to the front for this long (settings).
pub fn buds_front_ms() -> u64 {
    crate::settings::current().buds_front_on_connect_s as u64 * 1000
}

/// Lower is closer to the front; `None` means the presence is no longer shown.
fn rank(p: &Presence, now_ms: u64, keep_ms: u64, buds_front: u64) -> Option<u8> {
    let age = now_ms.saturating_sub(p.changed_ms);
    match (p.kind, p.state) {
        (ActivityKind::Timer, PresenceState::Done) => Some(0),
        (ActivityKind::Volume, _) if age < VOLUME_MOMENT_MS => Some(1),
        (ActivityKind::Volume, _) => None,
        (ActivityKind::Power, _) if age < power::POWER_MOMENT_MS => Some(1),
        (ActivityKind::Power, _) => None,
        (ActivityKind::Match(_), PresenceState::MatchMoment) => Some(1),
        (ActivityKind::Buds, PresenceState::Moment) if age < BUDS_MODE_MOMENT_MS => Some(1),
        // The offer to connect (the case opened), while it waits.
        (ActivityKind::Buds, PresenceState::Waiting) => Some(1),
        (ActivityKind::Timer | ActivityKind::Stopwatch, _) => Some(2),
        (ActivityKind::Match(_), PresenceState::MatchLive) => Some(3),
        (ActivityKind::Media(_), PresenceState::Playing) => Some(4),
        (ActivityKind::Buds, PresenceState::Connected { battery_low }) if battery_low || age < buds_front => Some(5),
        (ActivityKind::Match(_), _) => Some(6),
        (ActivityKind::Privacy, _) => Some(7),
        (ActivityKind::Headset(_), PresenceState::Connected { battery_low }) if battery_low || age < buds_front => Some(5),
        (ActivityKind::Headset(_), _) => Some(9),
        // Only present while it has items, so never `None`.
        (ActivityKind::Shelf, _) => Some(8),
        (ActivityKind::Clock, _) => Some(10),
        // Always on with a city: the last thing behind everything else.
        (ActivityKind::Weather, _) => Some(10),
        (ActivityKind::Rain, _) if age < RAIN_MOMENT_MS => Some(1),
        (ActivityKind::Rain, _) => None,
        (ActivityKind::WeatherAlert, _) if age < ALERT_MOMENT_MS => Some(1),
        (ActivityKind::WeatherAlert, _) => None,
        (ActivityKind::Capture, _) if age < capture::CAPTURE_MOMENT_MS => Some(1),
        (ActivityKind::Capture, _) => None,
        (ActivityKind::Live(_), PresenceState::Waiting) if age < live::LIVE_WAIT_URGENT_MS => Some(1),
        (ActivityKind::Live(_), PresenceState::Waiting) => Some(3),
        (ActivityKind::Live(_), PresenceState::Done) if age < live::LIVE_DONE_MS => Some(1),
        (ActivityKind::Live(_), PresenceState::Done) => None,
        (ActivityKind::Live(_), _) => Some(6),
        (ActivityKind::Media(_), _) if age <= keep_ms => Some(8),
        (ActivityKind::Media(_), _) => None,
        (ActivityKind::Buds, _) => Some(9),
        (ActivityKind::Game(_), PresenceState::Moment) if age < GAME_MOMENT_MS => Some(1),
        (ActivityKind::Game(_), PresenceState::Moment) => None,
        // Downloading or running: with the running live items, behind the music.
        (ActivityKind::Game(_), _) => Some(6),
        (ActivityKind::Usage, _) if age < USAGE_MOMENT_MS => Some(1),
        (ActivityKind::Usage, _) => None,
        (ActivityKind::Away, _) if age < away::AWAY_MOMENT_MS => Some(1),
        (ActivityKind::Away, _) => None,
        (ActivityKind::Transfer, PresenceState::Moment) if age < transfer::TRANSFER_DONE_MS => Some(1),
        (ActivityKind::Transfer, PresenceState::Moment) => None,
        // Something the user started and waits for: ahead of the music.
        (ActivityKind::Transfer, _) => Some(3),
    }
}

/// A moment leading the stack now, or a timer done: what goes ahead of the user's pick.
pub fn urgent(p: &Presence, now_ms: u64) -> bool {
    rank(p, now_ms, media_paused_keep_ms(), buds_front_ms()).is_some_and(|r| r <= 1)
}

/// Adds what shows without an activity of its own: the clock (carrying the weather) when nothing
/// else is on and `idle_clock` allows it, and with other things on, the weather at the back
/// (`weather_behind`: there is a weather and it may wait in the stack).
pub fn add_idle(p: &mut Vec<Presence>, now_ms: u64, idle_clock: bool, weather_behind: bool) {
    // On means shown: a finished moment or a long-paused player stays in the list unranked.
    let (keep, buds_front) = (media_paused_keep_ms(), buds_front_ms());
    let on = p.iter().any(|x| rank(x, now_ms, keep, buds_front).is_some());
    let running = |kind| Presence { kind, state: PresenceState::Running, changed_ms: 0 };
    if !on && idle_clock {
        p.push(running(ActivityKind::Clock));
    } else if on && weather_behind {
        p.push(running(ActivityKind::Weather));
    }
}

/// Front first. Paused media older than 10 min is dropped. `user_front` = (kind, picked_ms)
/// holds until any presence changes after picked_ms.
pub fn stack_order(p: &[Presence], now_ms: u64, user_front: Option<(ActivityKind, u64)>) -> Vec<ActivityKind> {
    let (keep, buds_front) = (media_paused_keep_ms(), buds_front_ms());
    let mut ranked: Vec<(u8, Reverse<u64>, ActivityKind)> =
        p.iter().filter_map(|x| rank(x, now_ms, keep, buds_front).map(|r| (r, Reverse(x.changed_ms), x.kind))).collect();
    ranked.sort_by_key(|(r, changed, _)| (*r, *changed));
    let rank_of: Vec<(ActivityKind, u8)> = ranked.iter().map(|(r, _, k)| (*k, *r)).collect();
    let mut order: Vec<ActivityKind> = ranked.into_iter().map(|(_, _, k)| k).collect();
    if let Some((kind, picked_ms)) = user_front {
        let superseded = p.iter().any(|x| x.changed_ms > picked_ms);
        if let (false, Some(i)) = (superseded, order.iter().position(|k| *k == kind)) {
            let picked = order.remove(i);
            order.insert(0, picked);
        }
    }
    // A finished timer (rank 0) and a moment while it plays (rank 1) go ahead of the user's
    // pick (stable: the rest keeps its order, so the pick is back in front once the moment ends).
    let urgent = |k: &ActivityKind| rank_of.iter().find(|(kind, _)| kind == k).map_or(2, |(_, r)| (*r).min(2));
    order.sort_by_key(urgent);
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActivityKind::*;
    use PresenceState::*;

    fn p(kind: ActivityKind, state: PresenceState, changed_ms: u64) -> Presence {
        Presence { kind, state, changed_ms }
    }

    #[test]
    fn volume_moment_is_urgent_and_short() {
        let live = [p(Media(1), Playing, 100), p(Volume, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 1_500, picked).first(), Some(&Volume), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + VOLUME_MOMENT_MS + 1, picked);
        assert!(!after.contains(&Volume), "gone after {VOLUME_MOMENT_MS} ms: {after:?}");
        assert_eq!(settings_page_for(Volume), 8);
    }

    #[test]
    fn live_wait_is_urgent_then_ahead_of_media_and_done_is_a_moment() {
        use super::live::LIVE_DONE_MS;
        let picked = Some((Media(1), 2_000));
        let waiting = [p(Media(1), Playing, 100), p(Live(7), Waiting, 1_000)];
        assert_eq!(stack_order(&waiting, 4_900, picked).first(), Some(&Live(7)), "urgent: ahead of the pick");
        assert_eq!(stack_order(&waiting, 5_100, picked).first(), Some(&Media(1)), "the pick again");
        assert_eq!(stack_order(&waiting, 5_100, None), vec![Live(7), Media(1)], "ahead of the music");
        let running = [p(Media(1), Playing, 100), p(Media(2), Paused, 99_000), p(Live(7), Running, 1_000)];
        assert_eq!(stack_order(&running, 100_000, None), vec![Media(1), Live(7), Media(2)]);
        let done = [p(Media(1), Playing, 100), p(Live(7), Done, 1_000)];
        assert_eq!(stack_order(&done, 5_900, picked).first(), Some(&Live(7)));
        assert!(!stack_order(&done, 1_000 + LIVE_DONE_MS + 1, picked).contains(&Live(7)));
        assert!(has_card(Live(7)));
        assert_eq!(settings_page_for(Live(7)), 8);
    }

    #[test]
    fn a_screenshot_leads_for_six_seconds_without_a_card() {
        use super::capture::CAPTURE_MOMENT_MS;
        let live = [p(Media(1), Playing, 100), p(Capture, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 6_900, picked).first(), Some(&Capture), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + CAPTURE_MOMENT_MS + 1, picked);
        assert!(!after.contains(&Capture), "gone after {CAPTURE_MOMENT_MS} ms: {after:?}");
        assert!(!has_card(Capture));
        assert_eq!(settings_page_for(Capture), 8);
    }

    #[test]
    fn rain_moment_leads_for_six_seconds_without_a_card() {
        let live = [p(Media(1), Playing, 100), p(Rain, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 6_900, picked).first(), Some(&Rain), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + RAIN_MOMENT_MS + 1, picked);
        assert!(!after.contains(&Rain), "gone after {RAIN_MOMENT_MS} ms: {after:?}");
        assert!(!has_card(Rain));
        assert_eq!(settings_page_for(Rain), 8);
    }

    #[test]
    fn a_weather_alert_leads_for_six_seconds_without_a_card() {
        let live = [p(Media(1), Playing, 100), p(WeatherAlert, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 6_900, picked).first(), Some(&WeatherAlert), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + ALERT_MOMENT_MS + 1, picked);
        assert!(!after.contains(&WeatherAlert), "gone after {ALERT_MOMENT_MS} ms: {after:?}");
        assert!(!has_card(WeatherAlert));
        assert_eq!(settings_page_for(WeatherAlert), 8);
    }

    #[test]
    fn games_wait_with_the_live_items_and_their_moments_lead() {
        let live = [p(Media(1), Playing, 100), p(Game(5), Running, 0), p(Media(2), Paused, 99_000)];
        assert_eq!(stack_order(&live, 100_000, None), vec![Media(1), Game(5), Media(2)], "behind playing music, ahead of paused");
        let moment = [p(Media(1), Playing, 100), p(Game(5), Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&moment, 6_900, picked).first(), Some(&Game(5)), "ahead of the pick while it lasts");
        assert!(!stack_order(&moment, 1_000 + GAME_MOMENT_MS + 1, picked).contains(&Game(5)));
        assert!(has_card(Game(5)));
        assert_eq!(settings_page_for(Game(5)), 9);
    }

    #[test]
    fn a_screen_time_limit_leads_for_six_seconds_without_a_card() {
        let live = [p(Media(1), Playing, 100), p(Usage, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 6_900, picked).first(), Some(&Usage), "ahead of the pick while it lasts");
        assert!(!stack_order(&live, 1_000 + USAGE_MOMENT_MS + 1, picked).contains(&Usage));
        assert!(!has_card(Usage), "its pill opens the Dijital Denge card, not a stack card");
        assert_eq!(settings_page_for(Usage), 10);
    }

    #[test]
    fn the_away_moment_leads_for_eight_seconds_without_a_card() {
        use super::away::AWAY_MOMENT_MS;
        let live = [p(Media(1), Playing, 100), p(Away, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 8_900, picked).first(), Some(&Away), "ahead of the pick while it lasts");
        assert!(!stack_order(&live, 1_000 + AWAY_MOMENT_MS + 1, picked).contains(&Away));
        assert!(!has_card(Away), "its pill opens the away card, not a stack card");
        assert_eq!(settings_page_for(Away), 8);
    }

    #[test]
    fn a_copy_leads_the_music_and_its_end_is_a_moment() {
        use super::transfer::TRANSFER_DONE_MS;
        let live = [p(Media(1), Playing, 100), p(Transfer, Running, 1_000)];
        assert_eq!(stack_order(&live, 2_000, None), vec![Transfer, Media(1)], "ahead of the music");
        let done = [p(Media(1), Playing, 100), p(Transfer, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&done, 3_000, picked).first(), Some(&Transfer), "ahead of the pick while it lasts");
        assert!(!stack_order(&done, 1_000 + TRANSFER_DONE_MS + 1, picked).contains(&Transfer));
        assert!(!has_card(Transfer), "its pill brings Explorer's dialog up");
        assert_eq!(settings_page_for(Transfer), 8);
    }

    #[test]
    fn urgent_is_a_moment_or_a_timer_done() {
        assert!(urgent(&p(Volume, Moment, 1_000), 1_500));
        assert!(!urgent(&p(Volume, Moment, 1_000), 1_000 + VOLUME_MOMENT_MS), "over");
        assert!(urgent(&p(Timer, Done, 0), 100_000));
        assert!(urgent(&p(Match(3), MatchMoment, 0), 100_000));
        assert!(!urgent(&p(Match(3), MatchLive, 0), 100_000));
        assert!(!urgent(&p(Live(7), Running, 1_000), 1_500));
        assert!(urgent(&p(Live(7), Waiting, 1_000), 1_500), "Claude asking");
    }

    #[test]
    fn power_moment_leads_for_four_seconds() {
        use super::power::POWER_MOMENT_MS;
        let live = [p(Media(1), Playing, 100), p(Power, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 4_900, picked).first(), Some(&Power), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + POWER_MOMENT_MS + 1, picked);
        assert!(!after.contains(&Power), "gone after {POWER_MOMENT_MS} ms: {after:?}");
        assert_eq!(settings_page_for(Power), 8);
    }

    #[test]
    fn privacy_sits_behind_upcoming_matches_and_ahead_of_paused_media() {
        // The Buds connected long ago (no longer in front); the media paused a second ago.
        let live = [p(Media(1), Paused, 99_000), p(Privacy, Running, 99_500), p(Match(3), MatchUpcoming, 0), p(Buds, Connected { battery_low: false }, 0)];
        assert_eq!(stack_order(&live, 100_000, None), vec![Match(3), Privacy, Media(1), Buds]);
        assert_eq!(settings_page_for(Privacy), 8);
    }

    #[test]
    fn headsets_rank_like_the_buds() {
        let fresh = [p(Media(1), Playing, 100), p(Headset(7), Connected { battery_low: false }, 1_000)];
        assert_eq!(stack_order(&fresh, 1_500, None), vec![Media(1), Headset(7)], "behind playing media, as the Buds");
        let idle = [p(Media(1), Paused, 99_000), p(Headset(7), Connected { battery_low: false }, 0)];
        assert_eq!(stack_order(&idle, 100_000, None), vec![Media(1), Headset(7)], "behind paused media once settled");
        let low = [p(Match(3), MatchUpcoming, 0), p(Headset(7), Connected { battery_low: true }, 0)];
        assert_eq!(stack_order(&low, 100_000, None), vec![Headset(7), Match(3)], "ahead of an upcoming match while low");
        assert_eq!(settings_page_for(Headset(7)), 8);
    }

    #[test]
    fn system_activities_have_no_card() {
        for kind in [Media(1), Match(2), Timer, Stopwatch, Buds] {
            assert!(has_card(kind), "{kind:?}");
        }
        for kind in [Volume, Power, Privacy, Headset(3)] {
            assert!(!has_card(kind), "{kind:?}");
        }
    }

    #[test]
    fn the_buds_offer_waits_at_the_front() {
        // The case opened: the offer to connect stays ahead until it's answered or gone.
        let live = [p(Media(1), Playing, 100), p(Buds, Waiting, 1_000)];
        assert_eq!(stack_order(&live, 60_000, Some((Media(1), 2_000))).first(), Some(&Buds));
    }

    #[test]
    fn buds_mode_moment_is_urgent_and_short() {
        let live = [p(Media(1), Playing, 100), p(Buds, Moment, 1_000)];
        let picked = Some((Media(1), 2_000));
        assert_eq!(stack_order(&live, 2_500, picked).first(), Some(&Buds), "ahead of the pick while it lasts");
        let after = stack_order(&live, 1_000 + BUDS_MODE_MOMENT_MS + 1, None);
        assert_eq!(after, vec![Media(1), Buds], "then back behind the media");
    }

    #[test]
    fn the_weather_waits_at_the_back() {
        let live = [p(Weather, Running, 99_900), p(Buds, Connected { battery_low: false }, 0), p(Media(1), Paused, 99_000)];
        assert_eq!(stack_order(&live, 100_000, None), vec![Media(1), Buds, Weather]);
        assert_eq!(settings_page_for(Weather), 8);
        assert!(has_card(Weather));
    }

    #[test]
    fn the_weather_only_waits_behind_something() {
        let kinds = |p: &[Presence]| p.iter().map(|p| p.kind).collect::<Vec<_>>();
        let mut idle = vec![];
        add_idle(&mut idle, 0, true, true);
        assert_eq!(kinds(&idle), vec![Clock], "idle: the clock carries the weather");
        let mut idle = vec![];
        add_idle(&mut idle, 0, false, true);
        assert_eq!(kinds(&idle), vec![], "\"Boşta göster\" off: nothing when nothing is on");
        let mut busy = vec![p(Media(1), Playing, 0)];
        add_idle(&mut busy, 0, false, true);
        assert_eq!(kinds(&busy), vec![Media(1), Weather]);
        let mut busy = vec![p(Media(1), Playing, 0)];
        add_idle(&mut busy, 0, true, false);
        assert_eq!(kinds(&busy), vec![Media(1)], "no city, or not in the stack");
    }

    #[test]
    fn a_finished_moment_is_not_something_on() {
        // The volume's moment stays in the list until the next change, long after its pill went.
        let order = |p: &[Presence]| stack_order(p, 100_000, None);
        let mut idle = vec![p(Volume, Moment, 0)];
        add_idle(&mut idle, 100_000, true, true);
        assert_eq!(order(&idle), vec![Clock], "the clock comes back");
        // Paused 16 minutes ago: past the 10 the stack keeps it.
        let mut idle = vec![p(Volume, Moment, 0), p(Media(1), Paused, 0)];
        add_idle(&mut idle, 1_000_000, false, true);
        assert_eq!(stack_order(&idle, 1_000_000, None), vec![], "a long-paused player isn't on either");
        let mut busy = vec![p(Volume, Moment, 99_500)];
        add_idle(&mut busy, 100_000, true, true);
        assert_eq!(order(&busy), vec![Volume, Weather], "while the moment plays");
    }

    #[test]
    fn the_clock_stays_behind_everything() {
        let live = [p(Clock, Running, 5_000), p(Buds, Connected { battery_low: false }, 0), p(Media(1), Paused, 99_000)];
        assert_eq!(stack_order(&live, 100_000, None), vec![Media(1), Buds, Clock]);
        assert_eq!(settings_page_for(Clock), 8);
        assert!(has_card(Clock), "the weather card");
    }

    #[test]
    fn pill_right_click_opens_the_activitys_page() {
        assert_eq!(settings_page_for(Media(3)), 2);
        assert_eq!(settings_page_for(Timer), 3);
        assert_eq!(settings_page_for(Stopwatch), 3);
        assert_eq!(settings_page_for(Buds), 4);
        assert_eq!(settings_page_for(Match(7)), 5);
    }

    #[test]
    fn timer_done_beats_everything() {
        let live = [
            p(Media(1), Playing, 100),
            p(Buds, Connected { battery_low: true }, 0),
            p(Stopwatch, Running, 50),
            p(Timer, Done, 0),
        ];
        assert_eq!(stack_order(&live, 1_000, None), vec![Timer, Stopwatch, Media(1), Buds]);
    }

    #[test]
    fn running_timer_beats_playing_media() {
        let live = [p(Media(1), Playing, 100), p(Timer, Running, 0)];
        assert_eq!(stack_order(&live, 1_000, None), vec![Timer, Media(1)]);
    }

    #[test]
    fn buds_front_only_first_5s_or_low_battery() {
        let live = [p(Media(1), Paused, 0), p(Buds, Connected { battery_low: false }, 0)];
        assert_eq!(stack_order(&live, 4_999, None), vec![Buds, Media(1)]);
        assert_eq!(stack_order(&live, 5_000, None), vec![Media(1), Buds]);
        let low = [p(Media(1), Paused, 0), p(Buds, Connected { battery_low: true }, 0)];
        assert_eq!(stack_order(&low, 60_000, None), vec![Buds, Media(1)]);
    }

    #[test]
    fn paused_media_dropped_after_10_min() {
        let live = [p(Media(1), Paused, 0)];
        assert_eq!(stack_order(&live, 599_999, None), vec![Media(1)]);
        assert_eq!(stack_order(&live, 600_001, None), Vec::<ActivityKind>::new());
    }

    #[test]
    fn user_pick_holds_until_newer_change() {
        let mut live = vec![p(Timer, Running, 5_000), p(Buds, Connected { battery_low: false }, 0)];
        assert_eq!(stack_order(&live, 20_000, Some((Buds, 10_000))), vec![Buds, Timer]);
        live.push(p(Media(1), Playing, 10_001));
        assert_eq!(stack_order(&live, 20_000, Some((Buds, 10_000))), vec![Timer, Media(1), Buds]);
    }

    #[test]
    fn each_media_session_is_its_own_item() {
        let live = [p(Media(1), Paused, 500), p(Media(2), Playing, 100), p(Media(3), Playing, 300)];
        assert_eq!(stack_order(&live, 1_000, None), vec![Media(3), Media(2), Media(1)]);
    }

    /// A live match sits in front of playing music; pre-match and full time sit behind it.
    #[test]
    fn live_match_beats_playing_media() {
        let live = [p(Media(1), Playing, 500), p(Match(7), MatchLive, 100), p(Timer, Running, 50)];
        assert_eq!(stack_order(&live, 1_000, None), vec![Timer, Match(7), Media(1)]);
        let later = [p(Media(1), Playing, 500), p(Match(7), MatchUpcoming, 100), p(Media(2), Paused, 900)];
        assert_eq!(stack_order(&later, 1_000, None), vec![Media(1), Match(7), Media(2)]);
    }

    /// A goal brings its match to the front only while the moment lasts; the user's own pick
    /// is still there afterwards.
    #[test]
    fn moment_jumps_ahead_of_a_user_pick_only_while_it_lasts() {
        let picked = Some((Media(1), 2_000));
        let during = [p(Media(1), Playing, 500), p(Match(7), MatchMoment, 100), p(Timer, Running, 50)];
        assert_eq!(stack_order(&during, 3_000, picked), vec![Match(7), Media(1), Timer]);
        let after = [p(Media(1), Playing, 500), p(Match(7), MatchLive, 100), p(Timer, Running, 50)];
        assert_eq!(stack_order(&after, 9_000, picked), vec![Media(1), Timer, Match(7)]);
    }

    #[test]
    fn shelf_sits_behind_playing_media_and_ahead_of_the_clock() {
        let live = [p(Media(1), Playing, 100), p(Shelf, Running, 50), p(Clock, Running, 0)];
        assert_eq!(stack_order(&live, 1_000, None), vec![Media(1), Shelf, Clock]);
        let quiet = [p(Shelf, Running, 50), p(Headset(7), Connected { battery_low: false }, 0)];
        assert_eq!(stack_order(&quiet, 100_000, None).first(), Some(&Shelf), "ahead of a settled headset");
    }

    #[test]
    fn a_drop_brings_the_shelf_to_the_front_until_something_changes() {
        let picked = Some((Shelf, 2_000));
        let live = [p(Media(1), Playing, 100), p(Shelf, Running, 2_000)];
        assert_eq!(stack_order(&live, 3_000, picked).first(), Some(&Shelf));
        let changed = [p(Media(1), Playing, 2_500), p(Shelf, Running, 2_000)];
        assert_eq!(stack_order(&changed, 3_000, picked).first(), Some(&Media(1)));
    }

    #[test]
    fn shelf_has_a_card_and_the_system_page() {
        assert!(has_card(Shelf));
        assert_eq!(settings_page_for(Shelf), 8);
    }

    #[test]
    fn empty_input_empty_stack() {
        assert!(stack_order(&[], 0, Some((Media(1), 0))).is_empty());
    }
}
