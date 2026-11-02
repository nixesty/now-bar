//! Dev tool: runs the football worker and prints every update.
//! Usage: `footballprobe <seconds> --team 436 [--team 432]` or `footballprobe <seconds> --replay <dir>`.

use pebbly::activities::football::minute_text;
use pebbly::activities::football_worker::{search_teams, FootballWorker};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(60);
    let after = |flag: &str| args.windows(2).filter(|w| w[0] == flag).map(|w| w[1].clone()).collect::<Vec<_>>();
    if let Some(q) = after("--search").first() {
        for hit in search_teams(q) {
            println!("{} {} ({})", hit.id, hit.name, hit.league);
        }
        return;
    }
    let replay = after("--replay").first().map(std::path::PathBuf::from);
    let started = std::time::Instant::now();
    let _worker = FootballWorker::start(after("--team"), replay, move |matches| {
        let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
        for m in matches {
            let clock = m.clock.map(|c| minute_text(&c, now)).unwrap_or_default();
            let events: Vec<String> = m.events.iter().map(|e| format!("{}′{:?}{:?}{}", e.minute, e.side, e.kind, e.player.as_deref().unwrap_or(""))).collect();
            println!(
                "[{:>4}s] {} {} {}-{} {} | {:?} {clock} | {} | followed {:?} | {}",
                started.elapsed().as_secs(),
                m.competition,
                m.home.name,
                m.score.0,
                m.score.1,
                m.away.name,
                m.status,
                events.join(", "),
                m.followed,
                m.venue.as_deref().unwrap_or("")
            );
        }
    });
    std::thread::sleep(std::time::Duration::from_secs(seconds));
}
