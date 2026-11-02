//! Live football from ESPN's public site API (see docs/superpowers/designs/2026-10-20-sports/
//! data-sources.md): the pure part — parsing schedules, matches and searches, Turkish names,
//! statuses, the live minute, goal/red-card moments and the polling cadence.

use serde_json::Value;

use crate::i18n::{tr, trc, trf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchStatus {
    Scheduled,
    Live1,
    HalfTime,
    Live2,
    /// Before or between the halves of extra time.
    EtBreak,
    Et1,
    Et2,
    Pens,
    FullTime,
    FullTimeAet,
    FullTimePens,
    Postponed,
    Cancelled,
    Suspended,
    Abandoned,
}

impl MatchStatus {
    pub fn is_live(self) -> bool {
        matches!(self, Self::Live1 | Self::Live2 | Self::Et1 | Self::Et2 | Self::Pens | Self::HalfTime | Self::EtBreak)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, Self::FullTime | Self::FullTimeAet | Self::FullTimePens)
    }

    /// Called off, or stopped without a result.
    pub fn is_off(self) -> bool {
        matches!(self, Self::Postponed | Self::Cancelled | Self::Abandoned | Self::Suspended)
    }

    /// The minute can't pass this while the status lasts (45 in the first half, ...).
    fn minute_cap(self, minute: u16) -> u16 {
        match self {
            Self::Live1 => 45,
            Self::Live2 => 90,
            Self::Et1 => 105,
            Self::Et2 => 120,
            _ => minute,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Home,
    Away,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Goal,
    PenaltyGoal,
    /// On the side of the team credited with it.
    OwnGoal,
    RedCard,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchEvent {
    pub minute: u16,
    pub added: Option<u8>,
    pub side: Side,
    pub kind: EventKind,
    /// Surname ("Aydin"); `None` for staff cards.
    pub player: Option<String>,
}

/// ESPN's match clock when it was fetched; the UI ticks it on locally up to `cap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub minute: u16,
    pub added: Option<u8>,
    pub as_of_ms: u64,
    pub cap: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub short: String,
    /// "FB", "GS", ...
    pub code: String,
    /// (primary, secondary); `None` when only placeholders are known (take the crest's colour).
    pub colors: Option<TeamColors>,
    pub logo: Option<String>,
}

/// One upcoming match of a followed team, from its schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixture {
    pub event_id: String,
    pub league: String,
    pub kickoff_utc: i64,
    pub time_known: bool,
}

/// A team-search result for the settings window.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamHit {
    pub id: String,
    pub name: String,
    pub league: String,
    pub abbr: String,
    pub colors: Option<TeamColors>,
    pub logo: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchSnapshot {
    pub id: String,
    pub league: String,
    pub competition: String,
    pub kickoff_utc: i64,
    pub time_known: bool,
    pub status: MatchStatus,
    pub clock: Option<Clock>,
    pub home: Team,
    pub away: Team,
    pub score: (u8, u8),
    pub pens: Option<(u8, u8)>,
    pub events: Vec<MatchEvent>,
    pub url: Option<String>,
    /// The side of a followed team (home if both are followed).
    pub followed: Option<Side>,
    pub venue: Option<String>,
    /// Last five results, oldest first ("DWLWW").
    pub form: Option<(String, String)>,
    /// When the worker first saw it finished (unix seconds).
    pub ended_utc: Option<i64>,
    /// Basketball while a game is on: the quarter (5 and up: overtime) and the time left ("04:12").
    pub period: Option<(u8, String)>,
    /// Basketball: each side's points per quarter played (overtime last).
    pub quarters: Option<(Vec<u8>, Vec<u8>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    Goal(Side),
    /// The score went down: a goal taken back (VAR).
    Disallowed(Side),
    RedCard(Side),
}

/// (primary, secondary) team colours.
pub type TeamColors = ([u8; 3], [u8; 3]);

/// Turkish name, short name, code and pinned colours.
type LocalTeam = (&'static str, &'static str, &'static str, Option<TeamColors>);

/// Turkish names, codes and pinned colours for the clubs (and the national team) ESPN spells in
/// ASCII or colours with placeholders (data-sources.md, Appendix A).
/// Our short code and colours for a team we know better than ESPN does (Süper Lig, Türkiye).
pub fn known_team(id: &str) -> Option<(&'static str, Option<TeamColors>)> {
    local_team(id).map(|(_, _, code, colors)| (code, colors))
}

fn local_team(id: &str) -> Option<LocalTeam> {
    let c = |p: u32, s: u32| Some((rgb(p), rgb(s)));
    Some(match id {
        "436" => ("Fenerbahçe", "Fenerbahçe", "FB", c(0x0B2A6F, 0xFFE500)),
        "432" => ("Galatasaray", "Galatasaray", "GS", c(0xA90432, 0xFDB912)),
        "1895" => ("Beşiktaş", "Beşiktaş", "BJK", c(0x000000, 0xFFFFFF)),
        "997" => ("Trabzonspor", "Trabzonspor", "TS", c(0x8E003B, 0x5699EB)),
        "7914" => ("RAMS Başakşehir", "Başakşehir", "İBFK", None),
        "7648" => ("Konyaspor", "Konyaspor", "KON", None),
        "20070" => ("Gaziantep FK", "Gaziantep", "GFK", None),
        "789" => ("Göztepe", "Göztepe", "GÖZ", None),
        "6870" => ("Kasımpaşa", "Kasımpaşa", "KAS", None),
        "995" => ("Kocaelispor", "Kocaelispor", "KOC", None),
        "11429" => ("Samsunspor", "Samsunspor", "SAM", None),
        "9078" => ("Alanyaspor", "Alanyaspor", "ALN", None),
        "7656" => ("Çaykur Rizespor", "Rizespor", "RİZ", None),
        "996" => ("Gençlerbirliği", "Gençlerbirliği", "GB", c(0xE00712, 0x1C1C1C)),
        "20729" => ("Eyüpspor", "Eyüpspor", "EYÜ", None),
        "19267" => ("Erzurumspor FK", "Erzurumspor", "ERZ", None),
        "465" => ("Türkiye", "Türkiye", "TÜR", c(0xE30A17, 0xFFFFFF)),
        _ => return None,
    })
}

fn rgb(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn hex_rgb(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok().map(rgb)).flatten()
}

/// The competition in the interface's language, from ESPN's league slug.
pub fn competition_name(slug: &str) -> String {
    let base = slug.trim_end_matches("_qual");
    let name = match base {
        "tur.1" => "Trendyol Süper Lig",
        "uefa.champions" => tr("Şampiyonlar Ligi"),
        "uefa.europa" => tr("Avrupa Ligi"),
        "uefa.europa.conf" => tr("Konferans Ligi"),
        "uefa.super_cup" => tr("UEFA Süper Kupa"),
        "uefa.nations" => tr("UEFA Uluslar Ligi"),
        "fifa.worldq.uefa" | "fifa.wcq.ply" => tr("Dünya Kupası Elemeleri"),
        "fifa.world" => tr("Dünya Kupası"),
        "uefa.euro" => "EURO",
        "uefa.euroq" => tr("EURO Elemeleri"),
        "fifa.friendly" | "club.friendly" => tr("Hazırlık Maçı"),
        "euroleague" => "EuroLeague",
        "eurocup" => "EuroCup",
        other => other,
    };
    if slug.ends_with("_qual") { trf("{} · Ön Eleme", &[&name]) } else { name.to_string() }
}

/// "2026-10-20T16:45Z" (ESPN leaves out the seconds) or with seconds → unix seconds.
pub fn parse_kickoff(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (hh, mm) = (t.next()??, t.next()??);
    let ss = t.next().flatten().unwrap_or(0);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// "45'+3'" → (45, Some(3)); "90'" → (90, None).
pub fn parse_display_clock(s: &str) -> Option<(u16, Option<u8>)> {
    let mut parts = s.trim().split('+');
    let minute = parts.next()?.trim_end_matches('\'').parse::<u16>().ok()?;
    let added = match parts.next() {
        Some(a) => Some(a.trim_end_matches('\'').parse::<u8>().ok()?),
        None => None,
    };
    Some((minute, added))
}

/// The minute to show, ticking on locally from the last fetch ("67′", "45+2′").
pub fn minute_text(c: &Clock, now_ms: u64) -> String {
    match c.added {
        Some(a) => format!("{}+{}′", c.minute, a),
        None => {
            let ticked = c.minute as u64 + now_ms.saturating_sub(c.as_of_ms) / 60_000;
            format!("{}′", ticked.min(c.cap.max(c.minute) as u64))
        }
    }
}

fn status_of(state: &str, completed: bool, name: &str, period: i64) -> MatchStatus {
    use MatchStatus::*;
    match state {
        "pre" => Scheduled,
        "in" => match name {
            "STATUS_HALFTIME" => HalfTime,
            "STATUS_HALFTIME_ET" | "STATUS_END_OF_REGULATION" | "STATUS_END_OF_EXTRATIME" => EtBreak,
            "STATUS_SHOOTOUT" => Pens,
            // STATUS_IN_PROGRESS flickers in mid-half: go by the period.
            _ => match period {
                1 => Live1,
                3 => Et1,
                4 => Et2,
                5 => Pens,
                _ => Live2,
            },
        },
        "post" if completed => match name {
            "STATUS_FINAL_AET" => FullTimeAet,
            "STATUS_FINAL_PEN" => FullTimePens,
            _ => FullTime,
        },
        "post" => match name {
            "STATUS_POSTPONED" => Postponed,
            "STATUS_CANCELED" => Cancelled,
            "STATUS_ABANDONED" | "STATUS_FORFEIT" => Abandoned,
            _ => Suspended,
        },
        _ => Scheduled,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// ESPN gives scores as "2" on scoreboards and {value, displayValue} on schedules.
fn score_of(v: &Value) -> Option<u8> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64().map(|n| n as u8),
        Value::Object(o) => o.get("value").and_then(|x| x.as_f64()).map(|f| f as u8),
        _ => None,
    }
}

fn team_of(t: &Value) -> Team {
    let id = text(&t["id"]).unwrap_or_default();
    let espn_name = text(&t["displayName"]).unwrap_or_default();
    let espn_colors = hex_rgb(t["color"].as_str().unwrap_or("")).filter(|c| *c != [0, 0, 0]).map(|p| {
        let s = hex_rgb(t["alternateColor"].as_str().unwrap_or("")).unwrap_or([0xFF, 0xFF, 0xFF]);
        (p, s)
    });
    let logo = text(&t["logo"]).or_else(|| text(&t["logos"][0]["href"]));
    match local_team(&id) {
        Some((name, short, code, colors)) => Team { id, name: name.into(), short: short.into(), code: code.into(), colors: colors.or(espn_colors), logo },
        None => Team {
            id,
            short: text(&t["shortDisplayName"]).unwrap_or_else(|| espn_name.clone()),
            code: text(&t["abbreviation"]).unwrap_or_default(),
            name: espn_name,
            colors: espn_colors,
            logo,
        },
    }
}

/// "G. Mikautadze" → "Mikautadze".
fn surname(short: &str) -> String {
    match short.split_once(". ") {
        Some((initial, rest)) if initial.chars().count() <= 2 => rest.to_string(),
        _ => short.to_string(),
    }
}

/// One match from a scoreboard event (`/scoreboard/{id}` or `events[i]` of `/scoreboard`).
pub fn parse_event(e: &Value, fixture: &Fixture, followed: &[String]) -> Option<MatchSnapshot> {
    let c = &e["competitions"][0];
    let competitors = c["competitors"].as_array()?;
    let side_of = |w: &str| competitors.iter().find(|x| x["homeAway"] == w);
    let (home_c, away_c) = (side_of("home")?, side_of("away")?);
    let (home, away) = (team_of(&home_c["team"]), team_of(&away_c["team"]));
    let st = &c["status"];
    let status = status_of(
        st["type"]["state"].as_str().unwrap_or("pre"),
        st["type"]["completed"].as_bool().unwrap_or(false),
        st["type"]["name"].as_str().unwrap_or(""),
        st["period"].as_i64().unwrap_or(0),
    );
    let clock = (status != MatchStatus::Scheduled)
        .then(|| st["displayClock"].as_str().and_then(parse_display_clock))
        .flatten()
        .map(|(minute, added)| Clock { minute, added, as_of_ms: 0, cap: status.minute_cap(minute) });
    let events = c["details"]
        .as_array()
        .map(|details| {
            details
                .iter()
                .filter(|d| !d["shootout"].as_bool().unwrap_or(false))
                .filter_map(|d| {
                    let kind = if d["redCard"].as_bool().unwrap_or(false) {
                        EventKind::RedCard
                    } else if d["scoringPlay"].as_bool().unwrap_or(false) {
                        if d["ownGoal"].as_bool().unwrap_or(false) {
                            EventKind::OwnGoal
                        } else if d["penaltyKick"].as_bool().unwrap_or(false) {
                            EventKind::PenaltyGoal
                        } else {
                            EventKind::Goal
                        }
                    } else {
                        return None;
                    };
                    let (minute, added) = parse_display_clock(d["clock"]["displayValue"].as_str().unwrap_or("0'")).unwrap_or((0, None));
                    let side = if d["team"]["id"] == home_c["team"]["id"] { Side::Home } else { Side::Away };
                    let player = d["athletesInvolved"][0]["shortName"].as_str().map(surname);
                    Some(MatchEvent { minute, added, side, kind, player })
                })
                .collect()
        })
        .unwrap_or_default();
    let followed = if followed.contains(&home.id) {
        Some(Side::Home)
    } else if followed.contains(&away.id) {
        Some(Side::Away)
    } else {
        None
    };
    let pens = match (score_of(&home_c["shootoutScore"]), score_of(&away_c["shootoutScore"])) {
        (Some(h), Some(a)) => Some((h, a)),
        _ => None,
    };
    let form = match (text(&home_c["form"]), text(&away_c["form"])) {
        (Some(h), Some(a)) => Some((h, a)),
        _ => None,
    };
    Some(MatchSnapshot {
        id: text(&e["id"])?,
        league: fixture.league.clone(),
        competition: competition_name(&fixture.league),
        kickoff_utc: e["date"].as_str().and_then(parse_kickoff).unwrap_or(fixture.kickoff_utc),
        time_known: c["timeValid"].as_bool().unwrap_or(fixture.time_known),
        status,
        clock,
        score: (score_of(&home_c["score"]).unwrap_or(0), score_of(&away_c["score"]).unwrap_or(0)),
        home,
        away,
        pens,
        events,
        url: text(&e["links"][0]["href"]),
        followed,
        venue: text(&c["venue"]["fullName"]),
        form,
        ended_utc: None,
        period: None,
        quarters: None,
    })
}

/// A team's upcoming matches from `/soccer/all/teams/{id}/schedule?fixture=true`.
pub fn parse_schedule(v: &Value) -> Vec<Fixture> {
    v["events"]
        .as_array()
        .map(|events| {
            events
                .iter()
                .filter_map(|e| {
                    Some(Fixture {
                        event_id: text(&e["id"])?,
                        league: text(&e["league"]["slug"])?,
                        kickoff_utc: parse_kickoff(e["date"].as_str()?)?,
                        time_known: e["competitions"][0]["timeValid"].as_bool().unwrap_or(true),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Men's senior teams from ESPN's team search, with Turkish names where known.
pub fn parse_search(v: &Value) -> Vec<TeamHit> {
    let junior_or_women = |league: &str| [".w.", "wworld", "wchampions", "u17", "u19", "u20", "u21", "u23"].iter().any(|x| league.contains(x));
    v["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    text(&it["id"])?;
                    let league = text(&it["defaultLeagueSlug"]).unwrap_or_default();
                    let t = team_of(it);
                    (!t.name.is_empty()).then_some(TeamHit { id: t.id, name: t.name, league, abbr: t.code, colors: t.colors, logo: t.logo })
                })
                .filter(|h| !junior_or_women(&h.league))
                .collect()
        })
        .unwrap_or_default()
}

/// The pill's centre: kick-off time (local, `tz_offset_s` east of UTC), the minute, or the
/// status in Turkish short form.
pub fn centre_text(m: &MatchSnapshot, now_ms: u64, tz_offset_s: i64) -> String {
    use MatchStatus::*;
    // Basketball on: the quarter and the time left.
    if let (true, Some((quarter, left))) = (m.status.is_live(), &m.period) {
        return super::basketball::period_text(*quarter, left);
    }
    match m.status {
        Scheduled => {
            let local = m.kickoff_utc + tz_offset_s;
            if m.time_known {
                let secs = local.rem_euclid(86_400);
                format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
            } else {
                // Only the day is fixed: "26.10".
                let days = local.div_euclid(86_400);
                let (_, month, day) = civil_from_days(days);
                format!("{day:02}.{month:02}")
            }
        }
        Live1 | Live2 | Et1 | Et2 => m.clock.map(|c| minute_text(&c, now_ms)).unwrap_or_default(),
        HalfTime => tr("İY").into(),
        EtBreak => trc("maç", "UZ").into(),
        Pens => "PEN".into(),
        FullTime | FullTimeAet | FullTimePens => tr("MS").into(),
        Postponed => tr("ERT").into(),
        Cancelled => tr("İPT").into(),
        Abandoned | Suspended => tr("TAT").into(),
    }
}

/// (year, month, day) of a day count since 1970-01-01 (Howard Hinnant's algorithm).
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// What changed between two polls of the same match that deserves a moment.
pub fn moments(old: &MatchSnapshot, new: &MatchSnapshot) -> Vec<Moment> {
    let mut out = Vec::new();
    for (side, before, after) in [(Side::Home, old.score.0, new.score.0), (Side::Away, old.score.1, new.score.1)] {
        if after > before {
            out.push(Moment::Goal(side));
        } else if after < before {
            out.push(Moment::Disallowed(side));
        }
    }
    let reds = |m: &MatchSnapshot, side: Side| m.events.iter().filter(|e| e.kind == EventKind::RedCard && e.side == side).count();
    for side in [Side::Home, Side::Away] {
        if reds(new, side) > reds(old, side) {
            out.push(Moment::RedCard(side));
        }
    }
    out
}

/// Seconds until the next poll of one match (data-sources.md §6.2); errors back off ×2 up to
/// 5 min (or the phase's own interval if that is longer). `u64::MAX` = stop.
pub fn next_poll_s(status: MatchStatus, secs_to_kickoff: i64, failures: u32) -> u64 {
    use MatchStatus::*;
    let base: u64 = match status {
        Scheduled => match secs_to_kickoff {
            s if s > 24 * 3600 => 6 * 3600,
            s if s > 30 * 60 => 1800,
            s if s > 2 * 60 => 120,
            s if s > -30 * 60 => 30,
            _ => 120,
        },
        Live1 | Live2 | Et1 | Et2 | Pens => 20,
        HalfTime | EtBreak => 60,
        Suspended => 120,
        FullTime | FullTimeAet | FullTimePens => 120,
        Postponed | Cancelled | Abandoned => return u64::MAX,
    };
    if failures == 0 {
        return base;
    }
    (base << failures.min(16)).min(base.max(300))
}

/// Whether Pebbly shows the match now (unix seconds): from `before_min` ahead of kickoff
/// until `keep_min` after the end (or after kickoff, for matches called off).
/// When a match ended: kept once known; `now` when it was seen live and is finished now; else
/// unknown, so a result found late (a cold start, last night's match) isn't news again.
pub fn end_time(prev_status: Option<MatchStatus>, prev_ended: Option<i64>, status: MatchStatus, now_utc: i64) -> Option<i64> {
    prev_ended.or((status.is_finished() && prev_status.is_some_and(MatchStatus::is_live)).then_some(now_utc))
}

pub fn shown(m: &MatchSnapshot, now_utc: i64, before_min: u32, keep_min: u32) -> bool {
    let (before, keep) = (before_min as i64 * 60, keep_min as i64 * 60);
    // Live until four hours after kickoff at most: past that the updates stopped coming.
    if m.status.is_live() {
        return now_utc <= m.kickoff_utc + 4 * 3600;
    }
    if m.status.is_finished() {
        let ended = m.ended_utc.unwrap_or(m.kickoff_utc + 115 * 60);
        return now_utc <= ended + keep;
    }
    let off_end = if m.status.is_off() { m.kickoff_utc + keep } else { m.kickoff_utc + 3 * 3600 };
    now_utc >= m.kickoff_utc - before && now_utc <= off_end
}

/// A live match's source has been silent this long (any answer counts, changed or not: a
/// half-time's or a time-out's score doesn't move): the network or the worker went. Above the
/// slowest live poll (a suspended match's 2 min).
pub const STALE_AFTER_MS: u64 = 200_000;

pub fn is_stale(m: &MatchSnapshot, heard_ms: u64, now_ms: u64) -> bool {
    m.status.is_live() && now_ms.saturating_sub(heard_ms) > STALE_AFTER_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let text = match name {
            "final" => include_str!("../../tests/fixtures/espn/final-fb-slavia.json"),
            "live" => include_str!("../../tests/fixtures/espn/live-liv-vil.json"),
            "pre" => include_str!("../../tests/fixtures/espn/pre-gs-fb.json"),
            "schedule" => include_str!("../../tests/fixtures/espn/schedule-fb.json"),
            "search" => include_str!("../../tests/fixtures/espn/search-fener.json"),
            "tur1" => include_str!("../../tests/fixtures/espn/tur1-20261018.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    fn ucl() -> Fixture {
        Fixture { event_id: String::new(), league: "uefa.champions".into(), kickoff_utc: 0, time_known: true }
    }

    fn fb() -> Vec<String> {
        vec!["436".into()]
    }

    #[test]
    fn kickoff_times_parse() {
        assert_eq!(parse_kickoff("2026-10-20T16:45Z"), Some(1_792_514_700));
        assert_eq!(parse_kickoff("1970-01-01T00:00Z"), Some(0));
        assert_eq!(parse_kickoff("nope"), None);
    }

    #[test]
    fn schedule_lists_upcoming_fixtures_with_their_league() {
        let f = parse_schedule(&fixture("schedule"));
        assert_eq!(f.len(), 3);
        assert_eq!((f[0].event_id.as_str(), f[0].league.as_str()), ("401888265", "tur.1"));
        assert_eq!(f[2].league, "uefa.champions");
        assert_eq!(f[0].kickoff_utc, parse_kickoff("2026-10-26T18:30Z").unwrap());
        assert!(f[0].time_known);
    }

    #[test]
    fn search_finds_fenerbahce_and_drops_womens_teams() {
        let hits = parse_search(&fixture("search"));
        assert_eq!(hits[0].id, "436");
        assert_eq!(hits[0].name, "Fenerbahçe");
        assert!(hits.iter().all(|h| !h.league.contains(".w.")));
    }

    /// The settings window shows results with their crest, colours and short code.
    #[test]
    fn search_hits_carry_code_colours_and_logo() {
        let hits = parse_search(&fixture("search"));
        let fb = &hits[0];
        // Our own table wins over ESPN's yellow: the design's navy and yellow, "FB".
        assert_eq!((fb.abbr.as_str(), fb.colors), ("FB", Some(([0x0B, 0x2A, 0x6F], [0xFF, 0xE5, 0x00]))));
        assert_eq!(fb.logo.as_deref(), Some("https://a.espncdn.com/i/teamlogos/soccer/500/436.png"));
        assert_eq!(competition_name(&fb.league), "Trendyol Süper Lig");
    }

    #[test]
    fn pre_match_derby() {
        let m = parse_event(&fixture("pre"), &Fixture { league: "tur.1".into(), ..ucl() }, &fb()).unwrap();
        assert_eq!(m.status, MatchStatus::Scheduled);
        assert_eq!(m.competition, "Trendyol Süper Lig");
        assert_eq!((m.home.name.as_str(), m.home.code.as_str()), ("Galatasaray", "GS"));
        assert_eq!((m.away.name.as_str(), m.away.code.as_str()), ("Fenerbahçe", "FB"));
        assert_eq!(m.followed, Some(Side::Away));
        assert_eq!(m.kickoff_utc, parse_kickoff("2026-10-26T18:30Z").unwrap());
        assert_eq!(m.venue.as_deref(), Some("RAMS Park"));
        assert!(m.events.is_empty());
    }

    #[test]
    fn live_first_half_with_goals() {
        let m = parse_event(&fixture("live"), &ucl(), &[]).unwrap();
        assert_eq!(m.status, MatchStatus::Live1);
        assert_eq!(m.score, (0, 2));
        assert_eq!(m.clock.map(|c| (c.minute, c.added)), Some((33, None)));
        let goals: Vec<(u16, Side, Option<&str>)> = m.events.iter().map(|e| (e.minute, e.side, e.player.as_deref())).collect();
        assert_eq!(goals, vec![(4, Side::Away, Some("Mikautadze")), (29, Side::Away, Some("Cardona"))]);
        assert_eq!(m.followed, None);
        assert_eq!(m.competition, "Şampiyonlar Ligi");
    }

    #[test]
    fn full_time_with_stoppage_clock() {
        let m = parse_event(&fixture("final"), &ucl(), &fb()).unwrap();
        assert_eq!(m.status, MatchStatus::FullTime);
        assert_eq!(m.score, (2, 2));
        assert_eq!(m.clock.map(|c| (c.minute, c.added)), Some((90, Some(3))));
        assert_eq!(m.events.len(), 4, "yellow cards are left out");
        assert_eq!(m.home.colors, Some(([0x0B, 0x2A, 0x6F], [0xFF, 0xE5, 0x00])), "Fenerbahçe in navy, as designed");
        assert_eq!(m.url.as_deref(), Some("https://www.espn.com/soccer/match/_/gameId/401915403/slavia-prague-fenerbahce"));
    }

    #[test]
    fn own_goal_penalty_and_staff_red_card() {
        let day = fixture("tur1");
        let events = day["events"].as_array().unwrap();
        let tur = Fixture { league: "tur.1".into(), ..ucl() };
        let koc = parse_event(events.iter().find(|e| e["id"] == "401888272").unwrap(), &tur, &[]).unwrap();
        let kinds: Vec<(u16, Side, EventKind)> = koc.events.iter().map(|e| (e.minute, e.side, e.kind)).collect();
        // Kocaelispor (home) 1–2 Göztepe: an own goal credited to Göztepe, a Kocaelispor penalty.
        assert_eq!(kinds, vec![(24, Side::Away, EventKind::OwnGoal), (77, Side::Away, EventKind::Goal), (82, Side::Home, EventKind::PenaltyGoal)]);
        assert_eq!(koc.home.name, "Kocaelispor");
        assert_eq!(koc.away.name, "Göztepe");
        let gaz = parse_event(events.iter().find(|e| e["id"] == "401888275").unwrap(), &tur, &[]).unwrap();
        let red = gaz.events.iter().find(|e| e.kind == EventKind::RedCard).unwrap();
        assert_eq!((red.minute, red.added, red.side, red.player.as_deref()), (90, Some(5), Side::Away, None));
        assert_eq!(gaz.home.name, "RAMS Başakşehir");
    }

    #[test]
    fn pill_centre_texts() {
        let tr = 3 * 3600;
        let pre = parse_event(&fixture("pre"), &Fixture { league: "tur.1".into(), ..ucl() }, &fb()).unwrap();
        assert_eq!(centre_text(&pre, 0, tr), "21:30", "18:30Z in Istanbul");
        let live = parse_event(&fixture("live"), &ucl(), &[]).unwrap();
        assert_eq!(centre_text(&live, 0, tr), "33′");
        let mut m = live.clone();
        for (status, text) in [(MatchStatus::HalfTime, "İY"), (MatchStatus::FullTime, "MS"), (MatchStatus::FullTimeAet, "MS"),
            (MatchStatus::EtBreak, "UZ"), (MatchStatus::Pens, "PEN"), (MatchStatus::Postponed, "ERT"), (MatchStatus::Cancelled, "İPT"),
            (MatchStatus::Abandoned, "TAT"), (MatchStatus::Suspended, "TAT")] {
            m.status = status;
            assert_eq!(centre_text(&m, 0, tr), text, "{status:?}");
        }
    }

    #[test]
    fn display_clocks() {
        assert_eq!(parse_display_clock("45'+3'"), Some((45, Some(3))));
        assert_eq!(parse_display_clock("90'"), Some((90, None)));
        assert_eq!(parse_display_clock("HT"), None);
    }

    #[test]
    fn minute_ticks_locally_between_polls() {
        let c = Clock { minute: 66, added: None, as_of_ms: 1_000, cap: 90 };
        assert_eq!(minute_text(&c, 1_000), "66′");
        assert_eq!(minute_text(&c, 61_000), "67′");
        assert_eq!(minute_text(&Clock { minute: 44, ..c }, 1_000 + 5 * 60_000), "49′");
        assert_eq!(minute_text(&Clock { minute: 44, cap: 45, ..c }, 1_000 + 5 * 60_000), "45′");
        assert_eq!(minute_text(&Clock { minute: 45, added: Some(2), ..c }, 1_000 + 60_000), "45+2′");
    }

    fn snap(score: (u8, u8), reds: usize) -> MatchSnapshot {
        let mut m = parse_event(&fixture("live"), &ucl(), &[]).unwrap();
        m.score = score;
        m.events.retain(|e| e.kind != EventKind::RedCard);
        for _ in 0..reds {
            m.events.push(MatchEvent { minute: 70, added: None, side: Side::Home, kind: EventKind::RedCard, player: Some("Konaté".into()) });
        }
        m
    }

    #[test]
    fn moments_from_score_and_cards() {
        assert_eq!(moments(&snap((0, 2), 0), &snap((1, 2), 0)), vec![Moment::Goal(Side::Home)]);
        assert_eq!(moments(&snap((1, 2), 0), &snap((1, 1), 0)), vec![Moment::Disallowed(Side::Away)]);
        assert_eq!(moments(&snap((1, 2), 0), &snap((1, 2), 1)), vec![Moment::RedCard(Side::Home)]);
        assert!(moments(&snap((1, 2), 1), &snap((1, 2), 1)).is_empty());
    }

    #[test]
    fn a_result_found_late_has_no_end_time() {
        use MatchStatus::*;
        assert_eq!(end_time(None, None, FullTime, 100), None, "found already finished");
        assert_eq!(end_time(Some(Scheduled), None, FullTime, 100), None, "slept through it");
        assert_eq!(end_time(Some(Live2), None, FullTime, 100), Some(100), "seen to the end");
        assert_eq!(end_time(Some(FullTime), Some(50), FullTime, 100), Some(50), "kept");
        assert_eq!(end_time(Some(Live2), None, Live2, 100), None, "still on");
    }

    #[test]
    fn polling_cadence() {
        use MatchStatus::*;
        assert_eq!(next_poll_s(Live2, -600, 0), 20);
        assert_eq!(next_poll_s(Live2, -600, 2), 80);
        assert_eq!(next_poll_s(Live1, -600, 9), 300);
        assert_eq!(next_poll_s(HalfTime, -2_900, 0), 60);
        assert_eq!(next_poll_s(Scheduled, 3 * 3600, 0), 1800);
        assert_eq!(next_poll_s(Scheduled, 20 * 60, 0), 120);
        assert_eq!(next_poll_s(Scheduled, 60, 0), 30);
        assert_eq!(next_poll_s(Scheduled, -40 * 60, 0), 120);
        assert_eq!(next_poll_s(FullTime, -7_000, 0), 120);
        assert_eq!(next_poll_s(Postponed, 0, 0), u64::MAX);
    }

    #[test]
    fn stale_only_when_the_source_went_quiet_during_a_live_match() {
        let mut m = snap((1, 0), 0);
        m.status = MatchStatus::HalfTime;
        assert!(!is_stale(&m, 1_000, 1_000 + 150_000), "a half-time poll every minute, nothing changing: heard all along");
        assert!(is_stale(&m, 1_000, 1_000 + 201_000));
        m.status = MatchStatus::FullTime;
        assert!(!is_stale(&m, 1_000, 10_000_000), "over: nothing to hear");
    }

    #[test]
    fn shown_from_before_kickoff_until_after_the_end() {
        let mut m = parse_event(&fixture("pre"), &Fixture { league: "tur.1".into(), ..ucl() }, &fb()).unwrap();
        let k = m.kickoff_utc;
        assert!(!shown(&m, k - 61 * 60, 60, 30));
        assert!(shown(&m, k - 59 * 60, 60, 30));
        m.status = MatchStatus::FullTime;
        m.ended_utc = Some(k + 110 * 60);
        assert!(shown(&m, k + 139 * 60, 60, 30));
        assert!(!shown(&m, k + 141 * 60, 60, 30));
    }

    #[test]
    fn a_live_match_not_heard_of_for_hours_is_no_longer_shown() {
        // The network or the worker went away mid-match: it must not stay "live" for 22 hours.
        let mut m = parse_event(&fixture("pre"), &Fixture { league: "tur.1".into(), ..ucl() }, &fb()).unwrap();
        let k = m.kickoff_utc;
        m.status = MatchStatus::Live2;
        assert!(shown(&m, k + 3 * 3600, 60, 30), "extra time and penalties still fit");
        assert!(!shown(&m, k + 5 * 3600, 60, 30));
    }
}
