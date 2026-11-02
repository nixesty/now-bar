//! Asks Open-Meteo for the weather every 15 minutes (and at once when the place changes) on its
//! own thread, and posts it; the settings window's city search is here too. Every quarter hour,
//! so the rain warning comes from a forecast at most 15 minutes old. The place is the city picked
//! in the settings or, without one, where the PC is: Windows' location service every 3 hours
//! (and after a wake), named by OpenStreetMap.

use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
use windows::Foundation::TimeSpan;

use super::football_worker::fetch_text;
use super::weather::{air_url, forecast_url, geocode_url, km_between, parse_aqi, parse_forecast, parse_geocode, parse_reverse, reverse_url, rough, LocateState, Located, Weather};
use crate::platform::log::log;
use crate::settings::{Settings, WeatherCity};

const EVERY: Duration = Duration::from_secs(15 * 60);
/// The rest of the day (the brief's rain line); the cards show the first [`CARD_HOURS`].
pub const HOURS: usize = 24;
pub const CARD_HOURS: usize = 6;
/// A laptop moves: where the PC is, looked for again this often.
const LOCATE_EVERY: Duration = Duration::from_secs(3 * 3600);
/// Closer than this to the last place found, its name still holds (no new question to
/// OpenStreetMap).
const SAME_PLACE_KM: f64 = 5.0;
/// The name of a place OpenStreetMap couldn't be asked about.
const UNNAMED: &str = "Konumun";

/// The latest weather (`None`: no place, or no answer yet).
pub type WeatherMailbox = Arc<Mutex<Option<Weather>>>;

/// Where the weather is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// Neither a city nor the location: the clock alone.
    Off,
    City(WeatherCity),
    /// Where the PC is.
    Locate,
}

impl Place {
    /// The settings' place: a picked city wins over the location.
    pub fn of(s: &Settings) -> Place {
        match (&s.weather_city, s.weather_locate) {
            (Some(c), _) => Place::City(c.clone()),
            (None, true) => Place::Locate,
            (None, false) => Place::Off,
        }
    }
}

/// The place the weather is for now (the picked city, or where the PC was found).
static IN_USE: Mutex<Option<WeatherCity>> = Mutex::new(None);

/// The place the weather is for now, once known.
pub fn place() -> Option<WeatherCity> {
    IN_USE.lock().ok().and_then(|p| p.clone())
}

fn located_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Pebbly").join("place.json")
}

/// What Pebbly last found of where the PC is (the settings window's location row).
pub fn read_located() -> Located {
    std::fs::read_to_string(located_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Where the PC is: Windows' location service (a position up to 10 minutes old does), the point
/// to two decimals, named by OpenStreetMap unless it's within a few kilometres of `last`.
/// `Err(true)` when Windows keeps the location from apps, `Err(false)` when there's no fix now.
fn locate(last: Option<&WeatherCity>) -> Result<WeatherCity, bool> {
    let access = Geolocator::RequestAccessAsync().and_then(|op| op.join());
    match access {
        Ok(GeolocationAccessStatus::Allowed) => {}
        Ok(_) => return Err(true),
        Err(e) => {
            log(&format!("weather: location access: {e:?}"));
            return Err(false);
        }
    }
    let ticks = |s: i64| TimeSpan { Duration: s * 10_000_000 };
    let fix = Geolocator::new().and_then(|g| {
        let _ = g.SetDesiredAccuracy(PositionAccuracy::Default);
        g.GetGeopositionAsyncWithAgeAndTimeout(ticks(600), ticks(20))?.join()
    });
    let point = fix.and_then(|f| f.Coordinate()?.Point()?.Position()).map_err(|e| {
        log(&format!("weather: no position: {e:?}"));
        false
    })?;
    let (lat, lon) = (rough(point.Latitude), rough(point.Longitude));
    if let Some(l) = last.filter(|l| l.name != UNNAMED && km_between((l.lat, l.lon), (lat, lon)) < SAME_PLACE_KM) {
        return Ok(WeatherCity { lat, lon, ..l.clone() });
    }
    let named = crate::platform::http::get_text(&reverse_url(lat, lon)).ok().and_then(|t| serde_json::from_str(&t).ok()).and_then(|v| parse_reverse(&v));
    let (name, country) = named.unwrap_or_else(|| (UNNAMED.to_string(), String::new()));
    Ok(WeatherCity { name, country, lat, lon })
}

pub struct WeatherWorker {
    tx: Sender<Place>,
}

impl WeatherWorker {
    /// `post` runs on the worker thread after each answer (the mailbox then holds it).
    pub fn start(place: Place, mailbox: WeatherMailbox, post: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = channel::<Place>();
        std::thread::Builder::new()
            .name("weather".into())
            .spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
                }
                let mut place = place;
                // Where the PC was found, and when (`None`: look now).
                let mut found: Option<WeatherCity> = None;
                let mut found_at: Option<Instant> = None;
                // How the last look went.
                let mut state = LocateState::Off;
                let mut said = Located::default();
                let mut fails = 0usize;
                loop {
                    let mut look = Located::default();
                    let city = match &place {
                        Place::Off => None,
                        Place::City(c) => Some(c.clone()),
                        Place::Locate => {
                            if found_at.is_none_or(|at| at.elapsed() >= LOCATE_EVERY) {
                                state = match locate(found.as_ref()) {
                                    Ok(c) => {
                                        if found.as_ref().is_none_or(|f| f.name != c.name) {
                                            log(&format!("weather: located {} ({})", c.name, c.country));
                                        }
                                        found = Some(c);
                                        LocateState::Found
                                    }
                                    Err(true) => {
                                        found = None;
                                        LocateState::Denied
                                    }
                                    Err(false) => LocateState::Failed,
                                };
                                // A failed look is tried again with the next forecast.
                                found_at = (state != LocateState::Failed).then(Instant::now);
                            }
                            look = Located { state, city: found.clone() };
                            found.clone()
                        }
                    };
                    if look != said {
                        if look.state == LocateState::Denied && said.state != LocateState::Denied {
                            log("weather: Windows keeps the location from apps");
                        }
                        let path = located_path();
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let _ = serde_json::to_string(&look).map(|t| std::fs::write(&path, t));
                        said = look;
                    }
                    if let Ok(mut p) = IN_USE.lock() {
                        *p = city.clone();
                    }
                    let answer = city.as_ref().and_then(|c| {
                        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                        let mut w = fetch_text(&forecast_url(c))
                            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
                            .inspect_err(|e| log(&format!("weather: {e}")))
                            .ok()
                            .and_then(|v| parse_forecast(&v, now, HOURS))?;
                        // The air has its own answer; without it the forecast still stands.
                        w.aqi = fetch_text(&air_url(c)).ok().and_then(|t| serde_json::from_str(&t).ok()).and_then(|v| parse_aqi(&v));
                        Some(w)
                    });
                    // A failed ask keeps the last answer; no place clears it.
                    let failed = answer.is_none() && city.is_some();
                    if !failed {
                        if let Ok(mut slot) = mailbox.lock() {
                            *slot = answer;
                        }
                        post();
                    }
                    // After a wake the network is often not back yet: try again soon, then less often.
                    fails = if failed { fails + 1 } else { 0 };
                    let wait = match fails {
                        0 => EVERY,
                        n => Duration::from_secs([10, 30, 60, 120, 300][(n - 1).min(4)]),
                    };
                    match rx.recv_timeout(wait) {
                        Ok(next) => {
                            // Asked again for the location (a wake, the setting turned on): look now.
                            if next == Place::Locate {
                                found_at = None;
                            }
                            place = next;
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            })
            .expect("spawn weather thread");
        WeatherWorker { tx }
    }

    pub fn set_place(&self, place: Place) {
        let _ = self.tx.send(place);
    }
}

/// Places named `name`, best first (blocking: the settings window calls it off its UI thread);
/// `None` when Open-Meteo couldn't be reached.
pub fn geocode(name: &str) -> Option<Vec<WeatherCity>> {
    if name.trim().chars().count() < 2 {
        return Some(Vec::new());
    }
    let text = fetch_text(&geocode_url(name)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(parse_geocode(&v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_city_wins_over_the_location() {
        let ankara = WeatherCity { name: "Ankara".into(), country: "Türkiye".into(), lat: 39.92, lon: 32.85 };
        let s = |city: Option<&WeatherCity>, locate| Settings { weather_city: city.cloned(), weather_locate: locate, ..Settings::default() };
        assert_eq!(Place::of(&s(Some(&ankara), true)), Place::City(ankara.clone()));
        assert_eq!(Place::of(&s(None, true)), Place::Locate);
        assert_eq!(Place::of(&s(None, false)), Place::Off);
        assert_eq!(Place::of(&Settings::default()), Place::Locate, "a new install looks where it is");
    }

    /// Asks Open-Meteo for real: `cargo test --lib live_weather -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_weather() {
        let places = geocode("İstanbul").expect("Open-Meteo reachable");
        println!("geocode: {:?}", places.first());
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let text = fetch_text(&forecast_url(&places[0])).unwrap();
        println!("forecast: {:?}", parse_forecast(&serde_json::from_str(&text).unwrap(), now, HOURS));
        let air = fetch_text(&air_url(&places[0])).unwrap();
        println!("air: {:?}", parse_aqi(&serde_json::from_str(&air).unwrap()));
        // A public point (Kadıköy's centre), not this PC's.
        let named = crate::platform::http::get_text(&reverse_url(40.99, 29.03)).ok().and_then(|t| serde_json::from_str(&t).ok()).and_then(|v| parse_reverse(&v));
        println!("reverse: {named:?}");
    }

    /// Where this PC is, for real (Windows may ask for the location first):
    /// `cargo t --lib live_locate -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_locate() {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        println!("located: {:?}", locate(None).map(|c| (c.name, c.country)));
    }
}
