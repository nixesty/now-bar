//! The idle pill's clock and weather: Open-Meteo's forecast for the city picked in the settings
//! or where the PC is (named by OpenStreetMap), its geocoding search for that city, WMO weather
//! codes as a symbol and a Turkish word, and the local clock and date. No keys. The pure part;
//! weather_worker.rs asks.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{n_, tr, trc, trf};
use crate::settings::WeatherCity;

/// The weather now and the next hours, in the city's own time.
#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub temp: f32,
    pub code: u8,
    pub is_day: bool,
    /// The coming hours (the current one first): unix time, temperature, code, day.
    pub hourly: Vec<(i64, f32, u8, bool)>,
    pub min: f32,
    pub max: f32,
    /// The city's offset from UTC (seconds), for the hours' labels.
    pub utc_offset_s: i64,
    /// The next two hours in quarters: unix time, precipitation (mm) and snowfall (cm) of the
    /// 15 minutes before that time.
    pub minutely: Vec<(i64, f32, f32)>,
    /// Gusts (km/h) hour by hour, with `hourly`'s times.
    pub gusts: Vec<(i64, f32)>,
    /// The quarter hours' WMO codes (thunder's start, to the quarter).
    pub quarter_codes: Vec<(i64, u8)>,
    /// The European air quality index now (its own answer; `None` without one).
    pub aqi: Option<u16>,
    /// When it was asked for (unix seconds): after a night's sleep it's yesterday's.
    pub fetched_utc: i64,
}

/// The hours from the one under way: after a sleep the forecast's first hours have gone.
pub fn hours_ahead(hourly: &[(i64, f32, u8, bool)], now_utc: i64) -> &[(i64, f32, u8, bool)] {
    let gone = hourly.iter().take_while(|h| h.0 + 3600 <= now_utc).count();
    &hourly[gone..]
}

/// Rain or snow about to start: minutes until it does (0: it already has).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RainAhead {
    pub minutes: u32,
    pub snow: bool,
}

/// The first wet quarter hour starting within the next hour; `None` for a dry hour.
pub fn rain_ahead(minutely: &[(i64, f32, f32)], now_utc: i64) -> Option<RainAhead> {
    // A trace (0.0x mm) is the model's noise, not rain anyone notices.
    const WET_MM: f32 = 0.1;
    minutely
        .iter()
        .filter(|(t, ..)| *t > now_utc && t - 900 < now_utc + 3600)
        .find(|(_, mm, snow)| *mm >= WET_MM || *snow > 0.0)
        .map(|(t, _, snow)| RainAhead { minutes: ((t - 900 - now_utc).max(0) as u32).div_ceil(60), snow: *snow > 0.0 })
}

/// Rain warns once a spell, before it starts: again only after a dry hour.
#[derive(Clone, Copy, Debug, Default)]
pub struct RainWatch {
    warned: bool,
}

impl RainWatch {
    /// A new forecast; `Some` when the rain moment should show.
    pub fn update(&mut self, ahead: Option<RainAhead>) -> Option<RainAhead> {
        match ahead {
            None => {
                self.warned = false;
                None
            }
            Some(_) if self.warned => None,
            Some(a) => {
                self.warned = true;
                (a.minutes > 0).then_some(a)
            }
        }
    }
}

/// "~20 dk içinde", to the nearest five minutes.
pub fn rain_when(minutes: u32) -> String {
    match ((minutes + 2) / 5 * 5).max(5) {
        60.. => tr("~1 saat içinde").to_string(),
        m => trf("~{} dk içinde", &[&m]),
    }
}

pub fn forecast_url(city: &WeatherCity) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={:.4}&longitude={:.4}&current=temperature_2m,weather_code,is_day\
         &hourly=temperature_2m,weather_code,is_day,wind_gusts_10m&daily=temperature_2m_max,temperature_2m_min\
         &minutely_15=precipitation,snowfall,weather_code&forecast_minutely_15=8\
         &timezone=auto&forecast_days=2&timeformat=unixtime",
        city.lat, city.lon
    )
}

pub fn geocode_url(name: &str) -> String {
    let mut q = String::new();
    for b in name.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => q.push(b as char),
            _ => q.push_str(&format!("%{b:02X}")),
        }
    }
    format!("https://geocoding-api.open-meteo.com/v1/search?name={q}&count=5&language={}&format=json", crate::i18n::lang().code())
}

/// The forecast answer: now, the next `hours` hours from now (`now_utc`), today's low and high.
pub fn parse_forecast(v: &Value, now_utc: i64, hours: usize) -> Option<Weather> {
    let c = &v["current"];
    let utc_offset_s = v["utc_offset_seconds"].as_i64().unwrap_or(0);
    let num = |x: &Value| x.as_f64().map(|f| f as f32);
    let h = &v["hourly"];
    let times = h["time"].as_array()?;
    let hour_start = now_utc - now_utc.rem_euclid(3600);
    let hourly = times
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let t = t.as_i64()?;
            (t >= hour_start).then_some(())?;
            Some((t, num(&h["temperature_2m"][i])?, h["weather_code"][i].as_u64()? as u8, h["is_day"][i].as_u64() == Some(1)))
        })
        .take(hours)
        .collect();
    // Today's low and high: the daily entry of the city's own date.
    let d = &v["daily"];
    let today = (now_utc + utc_offset_s).div_euclid(86_400);
    let day = d["time"].as_array()?.iter().position(|t| t.as_i64().is_some_and(|t| (t + utc_offset_s).div_euclid(86_400) == today)).unwrap_or(0);
    Some(Weather {
        temp: num(&c["temperature_2m"])?,
        code: c["weather_code"].as_u64()? as u8,
        is_day: c["is_day"].as_u64() == Some(1),
        hourly,
        min: num(&d["temperature_2m_min"][day])?,
        max: num(&d["temperature_2m_max"][day])?,
        utc_offset_s,
        minutely: parse_minutely(&v["minutely_15"]),
        gusts: times
            .iter()
            .enumerate()
            .filter_map(|(i, t)| Some((t.as_i64().filter(|t| *t >= hour_start)?, num(&h["wind_gusts_10m"][i])?)))
            .take(hours)
            .collect(),
        quarter_codes: v["minutely_15"]["time"]
            .as_array()
            .map(|ts| ts.iter().enumerate().filter_map(|(i, t)| Some((t.as_i64()?, v["minutely_15"]["weather_code"][i].as_u64()? as u8))).collect())
            .unwrap_or_default(),
        aqi: None,
        fetched_utc: now_utc,
    })
}

/// Open-Meteo's air quality now (the European index; no key either).
pub fn air_url(city: &WeatherCity) -> String {
    format!("https://air-quality-api.open-meteo.com/v1/air-quality?latitude={:.4}&longitude={:.4}&current=european_aqi&timezone=auto", city.lat, city.lon)
}

/// The air quality answer's index now.
pub fn parse_aqi(v: &Value) -> Option<u16> {
    v["current"]["european_aqi"].as_u64().map(|a| a.min(500) as u16)
}

/// The quarter hours (none when the answer has no `minutely_15`).
fn parse_minutely(m: &Value) -> Vec<(i64, f32, f32)> {
    let Some(times) = m["time"].as_array() else { return Vec::new() };
    let at = |key: &str, i: usize| m[key][i].as_f64().unwrap_or(0.0) as f32;
    times.iter().enumerate().filter_map(|(i, t)| Some((t.as_i64()?, at("precipitation", i), at("snowfall", i)))).collect()
}

/// The geocoding answer's places, best first.
pub fn parse_geocode(v: &Value) -> Vec<WeatherCity> {
    let Some(places) = v["results"].as_array() else { return Vec::new() };
    places
        .iter()
        .filter_map(|p| {
            // "Türkiye Cumhuriyeti" is what the geocoder calls the country in Turkish.
            let country = p["country"].as_str().unwrap_or("").trim_end_matches(" Cumhuriyeti").to_string();
            Some(WeatherCity { name: p["name"].as_str()?.to_string(), country, lat: p["latitude"].as_f64()?, lon: p["longitude"].as_f64()? })
        })
        .collect()
}

/// A point to two decimals (about a kilometre): all of where the PC is that leaves it.
pub fn rough(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// OpenStreetMap's name for a point (Nominatim: no key, a question every few hours): city level,
/// names in the interface's language.
pub fn reverse_url(lat: f64, lon: f64) -> String {
    format!("https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat={:.2}&lon={:.2}&zoom=10&accept-language={}", rough(lat), rough(lon), crate::i18n::lang().code())
}

/// The place a reverse answer names: the district or town ("Kadıköy"), else the city, the village
/// or the province; and the country. `None` for a point nothing names (the sea).
pub fn parse_reverse(v: &Value) -> Option<(String, String)> {
    let a = &v["address"];
    // A Turkish district is the answer's "town" (its "city" the whole of İstanbul).
    let name = ["town", "city", "village", "municipality", "county", "province", "state"].iter().find_map(|k| a[k].as_str().filter(|s| !s.trim().is_empty()))?;
    Some((name.trim().to_string(), a["country"].as_str().unwrap_or_default().to_string()))
}

/// How Pebbly's last look for where the PC is went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocateState {
    /// Not looking: a city is picked, or the location is off in the settings.
    #[default]
    Off,
    Finding,
    Found,
    /// Windows keeps the location from apps.
    Denied,
    /// No position now (the last place found, if any, still stands).
    Failed,
}

/// The last look for where the PC is, and the place in use: `place.json`, for the settings window.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Located {
    #[serde(default)]
    pub state: LocateState,
    #[serde(default)]
    pub city: Option<WeatherCity>,
}

/// The settings window's location row: its note ("Kadıköy · Türkiye", "Windows'ta konum kapalı")
/// and the place's name ("" while there is none).
pub fn located_text(l: &Located) -> (String, String) {
    let place = |c: &WeatherCity| {
        let note = if c.country.is_empty() { c.name.clone() } else { format!("{} · {}", c.name, c.country) };
        (note, c.name.clone())
    };
    match (l.state, &l.city) {
        (LocateState::Found | LocateState::Failed, Some(c)) => place(c),
        (LocateState::Failed, None) => (tr("Konum bulunamadı").into(), String::new()),
        (LocateState::Denied, _) => (tr("Windows'ta konum kapalı").into(), String::new()),
        _ => (tr("Bulunuyor…").into(), String::new()),
    }
}

/// Two points' distance in kilometres (latitude, longitude; along the earth's surface).
pub fn km_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la, lb) = (a.0.to_radians(), b.0.to_radians());
    let (dlat, dlon) = (lb - la, (b.1 - a.1).to_radians());
    let h = (dlat / 2.0).sin().powi(2) + la.cos() * lb.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

/// What the sky does, for the pill's and the card's moving background.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeatherFx {
    Sunny,
    ClearNight,
    Clouds,
    Fog,
    Rain,
    Storm,
    Snow,
}

/// The effect for a WMO weather code.
pub fn fx_of(code: u8, is_day: bool) -> WeatherFx {
    match code {
        0 | 1 if is_day => WeatherFx::Sunny,
        0 | 1 => WeatherFx::ClearNight,
        45 | 48 => WeatherFx::Fog,
        51..=67 | 80..=82 => WeatherFx::Rain,
        71..=77 | 85 | 86 => WeatherFx::Snow,
        95..=99 => WeatherFx::Storm,
        _ => WeatherFx::Clouds,
    }
}

/// The body colours (top, bottom) under an effect: a dark tint of its sky.
pub fn palette(fx: WeatherFx) -> ([u8; 3], [u8; 3]) {
    match fx {
        WeatherFx::Sunny => ([0x1B, 0x4A, 0x80], [0x10, 0x2C, 0x52]),
        WeatherFx::ClearNight => ([0x12, 0x1C, 0x40], [0x0A, 0x0E, 0x22]),
        WeatherFx::Clouds => ([0x2C, 0x31, 0x3A], [0x1B, 0x1E, 0x24]),
        WeatherFx::Fog => ([0x34, 0x35, 0x38], [0x20, 0x21, 0x24]),
        WeatherFx::Rain => ([0x16, 0x2A, 0x42], [0x0E, 0x18, 0x28]),
        WeatherFx::Storm => ([0x24, 0x1A, 0x3C], [0x12, 0x0E, 0x20]),
        WeatherFx::Snow => ([0x2A, 0x3C, 0x56], [0x18, 0x22, 0x32]),
    }
}

/// The effect's own colour (the circle behind the icon, the pill's sliver).
pub fn fx_accent(fx: WeatherFx) -> (u8, u8, u8) {
    match fx {
        WeatherFx::Sunny => (0xFF, 0xB4, 0x3C),
        WeatherFx::ClearNight => (0x78, 0x8C, 0xFF),
        WeatherFx::Clouds => (0xA0, 0xAA, 0xB9),
        WeatherFx::Fog => (0xAA, 0xAA, 0xAE),
        WeatherFx::Rain => (0x5A, 0xA0, 0xFF),
        WeatherFx::Storm => (0xAA, 0x78, 0xFF),
        WeatherFx::Snow => (0xBE, 0xDC, 0xFF),
    }
}

/// A colour emoji for a WMO weather code (a moon for a clear night).
pub fn symbol(code: u8, is_day: bool) -> &'static str {
    match code {
        0 if is_day => "☀\u{FE0F}",
        1 if is_day => "🌤\u{FE0F}",
        0 | 1 => "🌙",
        2 if is_day => "⛅",
        2 | 3 => "☁\u{FE0F}",
        // Segoe's fog emoji is a pale square that reads as a missing glyph; the fog's own
        // drifting bands say the rest.
        45 | 48 => "☁\u{FE0F}",
        80..=82 if is_day => "🌦\u{FE0F}",
        51..=67 | 80..=82 => "🌧\u{FE0F}",
        71..=77 | 85 | 86 => "❄\u{FE0F}",
        95..=99 => "⛈\u{FE0F}",
        _ => "☁\u{FE0F}",
    }
}

/// The code in a word or two ("Kapalı" is the sky's, hence the context).
pub fn describe(code: u8) -> &'static str {
    match code {
        0 => trc("hava", "Açık"),
        1 => trc("hava", "Az bulutlu"),
        2 => trc("hava", "Parçalı bulutlu"),
        3 => trc("hava", "Kapalı"),
        45 | 48 => trc("hava", "Sisli"),
        51..=57 => trc("hava", "Çiseleme"),
        61..=67 => trc("hava", "Yağmurlu"),
        71..=77 => trc("hava", "Karlı"),
        80..=82 => trc("hava", "Sağanak"),
        85 | 86 => trc("hava", "Kar sağanağı"),
        95..=99 => trc("hava", "Gök gürültülü"),
        _ => "",
    }
}

/// "19:42" and "Sal 21 Eki" for a local time (unix seconds already shifted to local).
pub fn clock_text(local: i64) -> (String, String) {
    const DAYS: [&str; 7] = [n_("Pzt"), n_("Sal"), n_("Çar"), n_("Per"), n_("Cum"), n_("Cmt"), n_("Paz")];
    const MONTHS: [&str; 12] = [n_("Oca"), n_("Şub"), n_("Mar"), n_("Nis"), n_("May"), n_("Haz"), n_("Tem"), n_("Ağu"), n_("Eyl"), n_("Eki"), n_("Kas"), n_("Ara")];
    let (days, secs) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let (_, month, day) = super::football::civil_from_days(days);
    // 1 January 1970 was a Thursday (index 3).
    let weekday = tr(DAYS[(days + 3).rem_euclid(7) as usize]);
    (format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60), format!("{weekday} {day} {}", tr(MONTHS[(month - 1) as usize])))
}

/// Whether an answer fetched at `fetched_utc` still says what the weather is now (two hours).
pub fn shows_now(fetched_utc: i64, now_utc: i64) -> bool {
    now_utc - fetched_utc <= 2 * 3600
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_older_than_two_hours_is_not_shown() {
        assert!(shows_now(1_000_000, 1_000_000 + 3_600));
        assert!(!shows_now(1_000_000, 1_000_000 + 2 * 3_600 + 1), "a day offline: not last night's sky as now's");
    }

    /// Open-Meteo's answer for İstanbul (real, trimmed to six hours; 1792602000 = 21 Oct 2026 20:00 local).
    const FORECAST: &str = r#"{"utc_offset_seconds":10800,"timezone":"Europe/Istanbul",
        "current":{"time":1792602000,"interval":900,"temperature_2m":17.9,"weather_code":0,"is_day":0},
        "hourly":{"time":[1792594800,1792598400,1792602000,1792605600,1792609200,1792612800,1792616400,1792620000],
                  "temperature_2m":[19.7,18.7,17.9,16.7,15.4,14.5,13.9,13.5],
                  "weather_code":[0,0,0,0,0,1,1,45],"is_day":[1,0,0,0,0,0,0,0]},
        "daily":{"time":[1792530000,1792616400],"temperature_2m_max":[21.4,21.5],"temperature_2m_min":[10.8,13.4]}}"#;

    #[test]
    fn past_hours_are_skipped() {
        let h = |t: i64| (t, 10.0, 0, true);
        let hours = [h(0), h(3600), h(7200), h(10_800)];
        assert_eq!(hours_ahead(&hours, 3_599).len(), 4, "the first hour is still under way");
        assert_eq!(hours_ahead(&hours, 3_600)[0].0, 3600);
        assert_eq!(hours_ahead(&hours, 9_000)[0].0, 7200, "after a sleep");
        assert!(hours_ahead(&hours, 20_000).is_empty());
    }

    #[test]
    fn forecast_now_and_the_next_hours() {
        let w = parse_forecast(&serde_json::from_str(FORECAST).unwrap(), 1_792_602_300, 4).unwrap();
        assert_eq!((w.temp, w.code, w.is_day), (17.9, 0, false));
        assert_eq!((w.min, w.max), (10.8, 21.4), "today's");
        assert_eq!(w.utc_offset_s, 10_800);
        let times: Vec<i64> = w.hourly.iter().map(|h| h.0).collect();
        assert_eq!(times, vec![1_792_602_000, 1_792_605_600, 1_792_609_200, 1_792_612_800], "from the current hour");
        assert_eq!(w.hourly[3], (1_792_612_800, 14.5, 1, false));
        assert_eq!(parse_forecast(&serde_json::json!({"error": true}), 0, 4), None);
    }

    #[test]
    fn forecast_keeps_the_quarter_hours() {
        let mut v: serde_json::Value = serde_json::from_str(FORECAST).unwrap();
        assert!(parse_forecast(&v, 1_792_602_300, 4).unwrap().minutely.is_empty(), "not asked for: none");
        v["minutely_15"] = serde_json::json!({"time": [1792602000, 1792602900], "precipitation": [0.0, 0.4], "snowfall": [0.0, 0.0]});
        assert_eq!(parse_forecast(&v, 1_792_602_300, 4).unwrap().minutely, vec![(1_792_602_000, 0.0, 0.0), (1_792_602_900, 0.4, 0.0)]);
        assert!(forecast_url(&WeatherCity { name: "x".into(), country: "y".into(), lat: 41.0, lon: 29.0 }).contains("&minutely_15=precipitation,snowfall,weather_code&forecast_minutely_15=8"));
    }

    #[test]
    fn forecast_keeps_the_gusts_and_the_quarters_codes() {
        let mut v: serde_json::Value = serde_json::from_str(FORECAST).unwrap();
        let w = parse_forecast(&v, 1_792_602_300, 4).unwrap();
        assert!(w.gusts.is_empty() && w.quarter_codes.is_empty() && w.aqi.is_none(), "an answer without them");
        v["hourly"]["wind_gusts_10m"] = serde_json::json!([10.0, 12.5, 64.8, 70.2, 30.0, 20.0, 15.0, 5.0]);
        v["minutely_15"] = serde_json::json!({"time": [1792602000, 1792602900], "precipitation": [0.0, 0.4], "snowfall": [0.0, 0.0], "weather_code": [3, 95]});
        let w = parse_forecast(&v, 1_792_602_300, 4).unwrap();
        // Hour by hour with the temperatures, from the current hour.
        assert_eq!(w.gusts, vec![(1_792_602_000, 64.8), (1_792_605_600, 70.2), (1_792_609_200, 30.0), (1_792_612_800, 20.0)]);
        assert_eq!(w.quarter_codes, vec![(1_792_602_000, 3), (1_792_602_900, 95)]);
        let url = forecast_url(&WeatherCity { name: "x".into(), country: "y".into(), lat: 41.0, lon: 29.0 });
        assert!(url.contains("&hourly=temperature_2m,weather_code,is_day,wind_gusts_10m&"), "{url}");
    }

    #[test]
    fn the_air_comes_from_its_own_answer() {
        let city = WeatherCity { name: "x".into(), country: "y".into(), lat: 41.0, lon: 29.0 };
        assert_eq!(air_url(&city), "https://air-quality-api.open-meteo.com/v1/air-quality?latitude=41.0000&longitude=29.0000&current=european_aqi&timezone=auto");
        assert_eq!(parse_aqi(&serde_json::json!({"current": {"time": "2026-10-23T19:00", "european_aqi": 72}})), Some(72));
        assert_eq!(parse_aqi(&serde_json::json!({"current": {"european_aqi": null}})), None);
        assert_eq!(parse_aqi(&serde_json::json!({"error": true})), None);
    }

    /// Quarters from `t0` (each value covers the 15 minutes before its time).
    fn quarters(t0: i64, mm: &[f32]) -> Vec<(i64, f32, f32)> {
        mm.iter().enumerate().map(|(i, v)| (t0 + 900 * i as i64, *v, 0.0)).collect()
    }

    #[test]
    fn rain_ahead_is_the_first_wet_quarter_within_the_hour() {
        let now = 10_000;
        // The quarter ending at now+900 is the one under way; the third one starts at now+1800.
        assert_eq!(rain_ahead(&quarters(now - 300, &[0.0; 8]), now), None, "dry");
        assert_eq!(rain_ahead(&quarters(now + 900, &[0.0, 0.0, 0.6, 1.2]), now), Some(RainAhead { minutes: 30, snow: false }));
        assert_eq!(rain_ahead(&quarters(now + 600, &[0.3, 0.0]), now), Some(RainAhead { minutes: 0, snow: false }), "wet now");
        assert_eq!(rain_ahead(&quarters(now + 900, &[0.0, 0.0, 0.0, 0.0, 0.0, 0.8]), now), None, "starts after the hour");
        assert_eq!(rain_ahead(&quarters(now - 900, &[2.0, 0.0, 0.0]), now), None, "that quarter is over");
        assert_eq!(rain_ahead(&[(now + 1_800, 0.05, 0.0)], now), None, "a trace isn't rain");
        assert_eq!(rain_ahead(&[(now + 1_800, 0.2, 0.3)], now), Some(RainAhead { minutes: 15, snow: true }));
    }

    #[test]
    fn rain_warns_once_a_spell_and_only_before_it() {
        let rain = |minutes| Some(RainAhead { minutes, snow: false });
        let mut w = RainWatch::default();
        assert_eq!(w.update(None), None);
        assert_eq!(w.update(rain(30)), rain(30));
        assert_eq!(w.update(rain(15)), None, "already warned");
        assert_eq!(w.update(rain(0)), None);
        assert_eq!(w.update(None), None, "a dry hour ends the spell");
        assert_eq!(w.update(rain(45)), rain(45));
        // Already raining at the first look: no warning for this spell.
        let mut late = RainWatch::default();
        assert_eq!(late.update(rain(0)), None);
        assert_eq!(late.update(rain(20)), None, "the same spell");
    }

    #[test]
    fn rain_when_rounds_to_five_minutes() {
        assert_eq!(rain_when(1), "~5 dk içinde");
        assert_eq!(rain_when(22), "~20 dk içinde");
        assert_eq!(rain_when(23), "~25 dk içinde");
        assert_eq!(rain_when(58), "~1 saat içinde");
    }

    #[test]
    fn geocoding_keeps_places_best_first() {
        let v = serde_json::json!({"results": [
            {"name": "İstanbul", "latitude": 41.01384, "longitude": 28.94966, "country": "Türkiye Cumhuriyeti", "admin1": "İstanbul"},
            {"name": "Ankara", "latitude": 39.91987, "longitude": 32.85427, "country": "Türkiye Cumhuriyeti"}
        ]});
        let places = parse_geocode(&v);
        assert_eq!(places.len(), 2);
        assert_eq!(places[0], WeatherCity { name: "İstanbul".into(), country: "Türkiye".into(), lat: 41.01384, lon: 28.94966 });
        assert!(parse_geocode(&serde_json::json!({"generationtime_ms": 0.5})).is_empty(), "nothing found");
        assert!(geocode_url("Şanlıurfa").contains("name=%C5%9Eanl%C4%B1urfa&"));
    }

    #[test]
    fn weather_words_in_english() {
        use crate::i18n::{in_language, Language};
        in_language(Language::English, || {
            assert_eq!((describe(3), describe(95), describe(0)), ("Overcast", "Thunderstorm", "Clear"));
            assert_eq!((rain_when(22), rain_when(58)), ("in ~20 min".to_string(), "in ~1 hour".to_string()));
            // Wednesday 21 October 2026, 19:42.
            assert_eq!(clock_text(1_792_611_720), ("19:42".to_string(), "Wed 21 Oct".to_string()));
        });
    }

    #[test]
    fn a_located_point_is_named_by_its_district() {
        // Nominatim's answers for public points (2026-10-24), as they came.
        let kadikoy = serde_json::json!({"name": "Osmanağa Mahallesi", "addresstype": "suburb", "address": {"suburb": "Osmanağa Mahallesi", "town": "Kadıköy", "province": "İstanbul", "ISO3166-2-lvl4": "TR-34", "region": "Marmara Bölgesi", "postcode": "34714", "country": "Türkiye", "country_code": "tr"}});
        assert_eq!(parse_reverse(&kadikoy), Some(("Kadıköy".into(), "Türkiye".into())));
        let berlin = serde_json::json!({"name": "Berlin", "addresstype": "city", "address": {"city": "Berlin", "ISO3166-2-lvl4": "DE-BE", "country": "Almanya", "country_code": "de"}});
        assert_eq!(parse_reverse(&berlin), Some(("Berlin".into(), "Almanya".into())));
        let village = serde_json::json!({"address": {"village": "Ağva", "province": "İstanbul", "country": "Türkiye"}});
        assert_eq!(parse_reverse(&village).map(|p| p.0), Some("Ağva".into()));
        let province = serde_json::json!({"address": {"province": "Muğla", "country": "Türkiye"}});
        assert_eq!(parse_reverse(&province).map(|p| p.0), Some("Muğla".into()));
        assert_eq!(parse_reverse(&serde_json::json!({"error": "Unable to geocode"})), None, "the sea");
    }

    #[test]
    fn only_a_rough_point_leaves_the_pc() {
        assert_eq!(rough(40.98765), 40.99);
        assert_eq!(rough(-0.004), -0.0);
        let url = crate::i18n::in_language(crate::i18n::Language::Turkish, || reverse_url(40.98765, 29.03123));
        assert_eq!(url, "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat=40.99&lon=29.03&zoom=10&accept-language=tr");
    }

    #[test]
    fn place_names_come_in_the_interfaces_language() {
        use crate::i18n::{in_language, Language};
        assert!(in_language(Language::English, || reverse_url(48.14, 11.58)).ends_with("&accept-language=en"), "Munich, not München or Münih");
        assert!(in_language(Language::English, || geocode_url("Munich")).contains("&language=en&"));
        assert!(in_language(Language::Turkish, || geocode_url("Münih")).contains("&language=tr&"));
    }

    #[test]
    fn the_location_row_says_where_or_why_not() {
        let kadikoy = WeatherCity { name: "Kadıköy".into(), country: "Türkiye".into(), lat: 40.99, lon: 29.03 };
        let l = |state, city: Option<&WeatherCity>| Located { state, city: city.cloned() };
        assert_eq!(located_text(&l(LocateState::Found, Some(&kadikoy))), ("Kadıköy · Türkiye".into(), "Kadıköy".into()));
        assert_eq!(located_text(&l(LocateState::Failed, Some(&kadikoy))), ("Kadıköy · Türkiye".into(), "Kadıköy".into()), "the last place stands");
        assert_eq!(located_text(&l(LocateState::Failed, None)), ("Konum bulunamadı".into(), String::new()));
        assert_eq!(located_text(&l(LocateState::Denied, None)), ("Windows'ta konum kapalı".into(), String::new()));
        assert_eq!(located_text(&l(LocateState::Finding, None)), ("Bulunuyor…".into(), String::new()));
        assert_eq!(located_text(&l(LocateState::Off, None)), ("Bulunuyor…".into(), String::new()), "just switched on: Pebbly hasn't looked yet");
        let unnamed = WeatherCity { name: "Konumun".into(), country: String::new(), lat: 40.99, lon: 29.03 };
        assert_eq!(located_text(&l(LocateState::Found, Some(&unnamed))).0, "Konumun", "no country, no dot");
        let file: Located = serde_json::from_str(r#"{"state": "found", "city": {"name": "Berlin", "country": "Almanya", "lat": 52.52, "lon": 13.4}}"#).expect("parses");
        assert_eq!(file.state, LocateState::Found);
        assert_eq!(serde_json::from_str::<Located>("{}").ok(), Some(Located::default()));
    }

    #[test]
    fn distances_in_kilometres() {
        let (kadikoy, fatih) = ((40.99, 29.03), (41.01, 28.95));
        let d = km_between(kadikoy, fatih);
        assert!((6.5..7.5).contains(&d), "{d}");
        assert_eq!(km_between(kadikoy, kadikoy), 0.0);
        assert!((km_between((0.0, 0.0), (0.0, 1.0)) - 111.2).abs() < 0.5, "a degree at the equator");
    }

    #[test]
    fn codes_in_colour_emoji_and_words() {
        // Emoji presentation (U+FE0F) so the colour font draws them, not the text glyphs.
        assert_eq!((symbol(0, true), describe(0)), ("☀\u{FE0F}", "Açık"));
        assert_eq!(symbol(0, false), "🌙");
        assert_eq!(symbol(1, true), "🌤\u{FE0F}");
        assert_eq!((symbol(2, true), describe(2)), ("⛅", "Parçalı bulutlu"));
        assert_eq!(symbol(2, false), "☁\u{FE0F}", "no sun at night");
        assert_eq!((symbol(3, true), describe(3)), ("☁\u{FE0F}", "Kapalı"));
        assert_eq!((symbol(63, true), describe(63)), ("🌧\u{FE0F}", "Yağmurlu"));
        assert_eq!(symbol(81, true), "🌦\u{FE0F}", "showers with sun");
        assert_eq!((symbol(73, true), describe(73)), ("❄\u{FE0F}", "Karlı"));
        assert_eq!((symbol(95, true), describe(95)), ("⛈\u{FE0F}", "Gök gürültülü"));
        // Segoe's fog emoji is a pale square: at 20 DIP it reads as a missing glyph.
        assert_eq!((symbol(45, true), describe(45)), ("☁\u{FE0F}", "Sisli"));
    }

    #[test]
    fn codes_choose_the_effect() {
        use WeatherFx::*;
        assert_eq!(fx_of(0, true), Sunny);
        assert_eq!(fx_of(1, true), Sunny);
        assert_eq!(fx_of(0, false), ClearNight);
        assert_eq!(fx_of(1, false), ClearNight);
        assert_eq!(fx_of(2, true), Clouds);
        assert_eq!(fx_of(3, false), Clouds);
        assert_eq!(fx_of(48, true), Fog);
        assert_eq!(fx_of(53, true), Rain, "drizzle");
        assert_eq!(fx_of(65, true), Rain);
        assert_eq!(fx_of(82, false), Rain, "showers");
        assert_eq!(fx_of(71, true), Snow);
        assert_eq!(fx_of(86, true), Snow, "snow showers");
        assert_eq!(fx_of(99, true), Storm);
        // Each has its own colours, dark enough for white text.
        let all = [Sunny, ClearNight, Clouds, Fog, Rain, Storm, Snow];
        for fx in all {
            let ([r, g, b], _) = palette(fx);
            assert!((r as u32 + g as u32 + b as u32) / 3 < 80, "{fx:?} too light for white text");
        }
        let tops: std::collections::HashSet<_> = all.iter().map(|fx| palette(*fx).0).collect();
        assert_eq!(tops.len(), all.len(), "one colour each");
    }

    #[test]
    fn clock_and_date_in_turkish() {
        // 21 Oct 2026 20:00 in İstanbul.
        assert_eq!(clock_text(1_792_602_000 + 10_800), ("20:00".to_string(), "Çar 21 Eki".to_string()));
        assert_eq!(clock_text(0), ("00:00".to_string(), "Per 1 Oca".to_string()), "1 Jan 1970 was a Thursday");
    }
}
