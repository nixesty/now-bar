//! Dev tool: prints every media snapshot for N seconds (default 15). Usage: `mediaprobe [seconds]`.

use pebbly::activities::media::position_at;
use pebbly::activities::media_worker::{filetime_now, MediaWorker};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(15);
    let _worker = MediaWorker::start(|live| {
        if live.is_empty() {
            println!("-- nothing playing");
        }
        for s in live {
            let pos = s.timeline.and_then(|t| position_at(&t, filetime_now())).map(|p| p / 10_000_000);
            let end = s.timeline.map(|t| t.end / 10_000_000);
            println!(
                "#{} {} | {:?} — {:?} | playing {} | pos {:?}/{:?} s | art {:?} | accent #{:02X}{:02X}{:02X} | can {:?} | shuffle {:?} repeat {:?}",
                s.id,
                s.app_name, s.title, s.artist, s.playing, pos, end, s.art_size, s.accent[0], s.accent[1], s.accent[2], s.can, s.shuffle, s.repeat
            );
        }
    });
    std::thread::sleep(std::time::Duration::from_secs(seconds));
}
