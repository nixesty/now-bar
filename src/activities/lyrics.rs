//! Synced lyrics for the media card: LRC text from lrclib.net, which line is on now, and the
//! request for a track.

/// A track's lines: when each starts (ms from the start of the track) and its text.
pub type Lines = Vec<(u64, String)>;

/// The timed lines of an LRC text, in time order; metadata tags (`[ar:…]`) and untimed lines are
/// left out. A line may carry several time tags (a repeated chorus).
pub fn parse_lrc(text: &str) -> Lines {
    let mut lines = Lines::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some(end) = tag.find(']') else { break };
            match tag_ms(&tag[..end]) {
                Some(ms) => times.push(ms),
                None => break,
            }
            rest = &tag[end + 1..];
        }
        let words = rest.trim();
        for ms in times {
            lines.push((ms, words.to_string()));
        }
    }
    lines.sort_by_key(|(ms, _)| *ms);
    lines
}

/// "mm:ss.xx" (or "mm:ss.xxx", "mm:ss") in milliseconds.
fn tag_ms(tag: &str) -> Option<u64> {
    let (m, s) = tag.split_once(':')?;
    let m: u64 = m.trim().parse().ok()?;
    let (sec, frac) = s.split_once('.').unwrap_or((s, ""));
    let sec: u64 = sec.trim().parse().ok()?;
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 100,
        2 => frac.parse::<u64>().ok()? * 10,
        _ => frac.get(..3)?.parse::<u64>().ok()?,
    };
    Some(m * 60_000 + sec * 1000 + frac_ms)
}

/// The line on at `pos_ms`: the last one that has started (`None` before the first).
pub fn line_at(lines: &Lines, pos_ms: u64) -> Option<usize> {
    lines.iter().rposition(|(ms, _)| *ms <= pos_ms)
}

/// What the pill shows at `pos_ms`: the line on, when it has words (between lines and before the
/// first, the title), and the ms until the next line starts (`None` after the last).
pub fn pill_line(lines: &Lines, pos_ms: u64) -> (Option<&str>, Option<u64>) {
    let on = line_at(lines, pos_ms);
    let words = on.map(|i| lines[i].1.trim()).filter(|l| !l.is_empty());
    let next = lines.get(on.map_or(0, |i| i + 1)).map(|(ms, _)| ms.saturating_sub(pos_ms));
    (words, next)
}

/// A slow singer's pace: a line's words take at most this long a letter. A longer wait for
/// the next line is a pause after them (an instrumental bit), not singing.
const SLOWEST_MS_PER_LETTER: u64 = 180;

/// Where the singing is in the line on at `pos_ms`: ms since it started, and how long its words
/// take (until the next line starts, at most a slow singer's pace); `None` when the pill shows
/// the title (before the first line, between lines).
pub fn sung(lines: &Lines, pos_ms: u64) -> Option<(u64, u64)> {
    let i = line_at(lines, pos_ms)?;
    let (start, words) = &lines[i];
    let letters = words.trim().chars().count() as u64;
    if letters == 0 {
        return None;
    }
    let slowest = letters * SLOWEST_MS_PER_LETTER;
    let span = lines.get(i + 1).map_or(slowest, |(next, _)| (next - start).min(slowest));
    Some((pos_ms - start, span))
}

/// lrclib.net's exact-match request for a track (duration in whole seconds).
pub fn query_url(artist: &str, title: &str, album: &str, duration_s: u64) -> String {
    format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}&duration={duration_s}",
        encode(artist),
        encode(title),
        encode(album)
    )
}

/// lrclib.net's keyword search for a track (title and artist as `search_terms` gives them).
pub fn search_url(artist: &str, title: &str) -> String {
    format!("https://lrclib.net/api/search?track_name={}&artist_name={}", encode(title), encode(artist))
}

/// How far a record's duration may be from the track's: the lines are timed for that length,
/// so a slowed or sped-up edit only takes a record of its own length, never the original's.
const DURATION_SLACK_S: f64 = 3.0;

/// Words that make a " - …" tail a decoration ("- Remastered 2011", "- Live at …", "- Radio Edit").
const TAIL_WORDS: &[&str] = &[
    "remaster", "remastered", "remasterizado", "remasterizada", "live", "edit", "version", "versión", "mix", "remix",
    "mono", "stereo", "slowed", "sped", "speed", "nightcore", "reverb", "bonus", "single", "acoustic", "akustik", "demo",
    "lyrics", "lyric", "video", "audio", "official", "radio", "extended", "instrumental", "visualizer",
];

/// A title without what players and YouTube add to it: bracketed parts ("(Official Video)",
/// "[Lyrics]", "(feat. …)", "(Slowed)"), a bare "feat. …", a " | …" tail and decoration tails
/// like " - Remastered 2011". A title that is nothing but brackets is kept as it is.
pub fn clean_title(title: &str) -> String {
    let mut t = title.split(" | ").next().unwrap_or(title).to_string();
    t = without_brackets(&t);
    let lower = t.to_ascii_lowercase();
    if let Some(at) = [" feat. ", " feat ", " ft. ", " ft ", " featuring "].iter().filter_map(|sep| lower.find(sep)).min() {
        t.truncate(at);
    }
    while let Some((head, tail)) = split_last_dash(&t) {
        let tail = tail.to_lowercase();
        let decoration = tail.starts_with("from ") || tail.split(|c: char| !c.is_alphanumeric()).any(|w| TAIL_WORDS.contains(&w));
        if !decoration {
            break;
        }
        t = head.to_string();
    }
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        title.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        t
    }
}

fn without_brackets(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '【' => depth += 1,
            ')' | ']' | '}' | '】' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// "head - tail" at the last spaced dash (hyphen, en or em dash).
fn split_last_dash(s: &str) -> Option<(&str, &str)> {
    [" - ", " – ", " — "].iter().filter_map(|d| s.rfind(d).map(|at| (at, d.len()))).max().map(|(at, len)| (&s[..at], &s[at + len..]))
}

/// The first of the artists a media session joins into one name ("A, B ve C", "A & B",
/// "A x B", "A feat. B"), without YouTube's " - Topic" and "…VEVO".
pub fn first_artist(artist: &str) -> String {
    let mut a = artist.trim().strip_suffix(" - Topic").unwrap_or(artist.trim()).to_string();
    if let Some(name) = a.strip_suffix("VEVO").filter(|n| !n.is_empty()) {
        // "LuisFonsiVEVO": the channel name runs the words together.
        let mut spaced = String::new();
        let mut prev_lower = false;
        for c in name.chars() {
            if prev_lower && c.is_uppercase() {
                spaced.push(' ');
            }
            prev_lower = c.is_lowercase();
            spaced.push(c);
        }
        a = spaced;
    }
    let lower = a.to_ascii_lowercase();
    let seps = [", ", " & ", " x ", " feat. ", " ft. ", " featuring ", " ve ", " and "];
    if let Some(at) = seps.iter().filter_map(|sep| lower.find(sep)).min() {
        a.truncate(at);
    }
    let a = a.trim();
    if a.is_empty() {
        artist.trim().to_string()
    } else {
        a.to_string()
    }
}

/// What to search lrclib for: the first artist, and the cleaned title without the
/// "Artist - " that YouTube video titles start with.
pub fn search_terms(artist: &str, title: &str) -> (String, String) {
    let first = first_artist(artist);
    let mut title = title.trim();
    if let Some((head, tail)) = [" - ", " – ", " — "].iter().find_map(|d| title.split_once(d)) {
        let same = |a: &str, b: &str| squash(a) == squash(b);
        if same(head, &first) || same(head, artist) || same(&first_artist(head), &first) {
            title = tail;
        }
    }
    (first, clean_title(title))
}

/// Whether a title names a slowed, sped-up or nightcore edit.
pub fn is_speed_edit(title: &str) -> bool {
    let t = norm(title);
    let words: Vec<&str> = t.split(' ').collect();
    words.iter().any(|w| matches!(*w, "slowed" | "nightcore" | "daycore"))
        || words.windows(2).any(|p| matches!(p, ["sped" | "speed", "up"]))
}

/// Lower case, accents and Turkish letters folded to ASCII, punctuation to spaces, single spaces.
fn norm(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
            'ì'..='ï' | 'ı' => 'i',
            'ñ' | 'ń' => 'n',
            'ò'..='ö' | 'ø' | 'ő' => 'o',
            'ş' | 'ś' | 'š' | 'ș' => 's',
            'ù'..='ü' | 'ű' | 'ů' => 'u',
            'ğ' => 'g',
            'ý' | 'ÿ' => 'y',
            'ž' | 'ź' | 'ż' => 'z',
            'ł' => 'l',
            '\u{300}'..='\u{36f}' => continue,
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn squash(s: &str) -> String {
    norm(s).replace(' ', "")
}

/// How well two names match: 2 the same, 1 one holds the other's words, 0 not at all.
fn likeness(ours: &str, theirs: &str) -> u8 {
    let (a, b) = (norm(ours), norm(theirs));
    if a.is_empty() || b.is_empty() {
        0
    } else if a == b || squash(&a) == squash(&b) {
        2
    } else if format!(" {b} ").contains(&format!(" {a} ")) || format!(" {a} ").contains(&format!(" {b} ")) {
        1
    } else {
        0
    }
}

/// When a record's synced lyrics start their last line (ms), if they have any.
fn last_line_ms(r: &serde_json::Value) -> Option<u64> {
    let lrc = r["syncedLyrics"].as_str()?;
    parse_lrc(lrc).last().map(|(ms, _)| *ms)
}

/// The synced LRC of the record in lrclib's `results` (a search's array) that fits the track:
/// it has synced lyrics that end within the track, its duration is within `DURATION_SLACK_S`,
/// and its title and artist match; the best title and artist match wins, then the closest
/// duration. For a slowed or sped-up edit, lyrics that end where an unedited version of
/// another length ends theirs are that version's timing (lrclib has many such copies filed
/// under the edit's length) and would run ahead of or behind the edit, so they're left out.
pub fn pick(results: &serde_json::Value, title: &str, artist: &str, duration_s: u64) -> Option<String> {
    let (our_artist, our_title) = search_terms(artist, title);
    let records = results.as_array()?;
    let off_of = |r: &serde_json::Value| (r["duration"].as_f64().unwrap_or(f64::MAX) - duration_s as f64).abs();
    // Where unedited versions of other lengths end their lyrics.
    let other_endings: Vec<u64> = if is_speed_edit(title) {
        records
            .iter()
            .filter(|r| off_of(r) > DURATION_SLACK_S && !is_speed_edit(r["trackName"].as_str().unwrap_or_default()))
            .filter_map(last_line_ms)
            .collect()
    } else {
        Vec::new()
    };
    let mut best: Option<(u8, f64, &str)> = None;
    for r in records {
        let Some(lrc) = r["syncedLyrics"].as_str().filter(|l| !l.trim().is_empty()) else { continue };
        let off = off_of(r);
        if off > DURATION_SLACK_S {
            continue;
        }
        let Some(end) = last_line_ms(r) else { continue };
        if end > (duration_s as f64 + DURATION_SLACK_S) as u64 * 1000 || other_endings.iter().any(|e| e.abs_diff(end) <= 500) {
            continue;
        }
        let their_artist = r["artistName"].as_str().unwrap_or_default();
        let title_score = likeness(&our_title, &clean_title(r["trackName"].as_str().unwrap_or_default()));
        let artist_score = likeness(&our_artist, &first_artist(their_artist)).max(likeness(&our_artist, their_artist).min(1));
        if title_score == 0 || artist_score == 0 {
            continue;
        }
        let score = title_score + artist_score;
        if best.is_none_or(|(s, o, _)| score > s || (score == s && off < o)) {
            best = Some((score, off, lrc));
        }
    }
    best.map(|(_, _, lrc)| lrc.to_string())
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_odd_fraction_is_skipped_not_a_crash() {
        // A multi-byte character right after the dot used to panic the lyrics thread.
        assert_eq!(parse_lrc("[00:12.öö]x\n[00:13.50]y").len(), 1);
    }

    #[test]
    fn lrc_parses_timestamps_and_skips_metadata() {
        let lines = parse_lrc("[ar:ZXVRF]\n[ti:Gem]\n[00:12.34]Bir\n[01:02.5][00:20.00]İki ve üç\n\nsöz yok\n[00:30]\n");
        assert_eq!(
            lines,
            vec![(12_340, "Bir".into()), (20_000, "İki ve üç".into()), (30_000, String::new()), (62_500, "İki ve üç".into())]
        );
    }

    #[test]
    fn line_at_picks_the_last_started_line() {
        let lines = parse_lrc("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");
        assert_eq!(line_at(&lines, 5_000), None);
        assert_eq!(line_at(&lines, 10_000), Some(0));
        assert_eq!(line_at(&lines, 29_999), Some(1));
        assert_eq!(line_at(&lines, 99_000), Some(2));
    }

    #[test]
    fn the_pill_shows_a_line_with_words_and_wakes_for_the_next() {
        let lines = parse_lrc("[00:01.00]Gece yarısı\n[00:03.00]\n[00:05.00]  Tren geçti  ");
        assert_eq!(pill_line(&lines, 500), (None, Some(500)), "before the first: the title");
        assert_eq!(pill_line(&lines, 1_000), (Some("Gece yarısı"), Some(2_000)));
        assert_eq!(pill_line(&lines, 3_500), (None, Some(1_500)), "an empty line is a gap: the title");
        assert_eq!(pill_line(&lines, 6_000), (Some("Tren geçti"), None), "the last stays");
        assert_eq!(pill_line(&Lines::new(), 6_000), (None, None));
    }

    #[test]
    fn the_singing_takes_until_the_next_line() {
        let lines = parse_lrc("[00:01.00]Gece yarısı tren geçti\n[00:04.00]\n[00:09.00]Son");
        assert_eq!(sung(&lines, 2_000), Some((1_000, 3_000)));
        assert_eq!(sung(&lines, 500), None, "before the first: the title");
        assert_eq!(sung(&lines, 5_000), None, "an empty line is a gap: the title");
    }

    #[test]
    fn a_long_wait_for_the_next_line_is_a_pause_not_singing() {
        // 22 letters at a slow singer's pace, not the 20 s until the next line.
        let lines = parse_lrc("[00:01.00]  Gece yarısı tren geçti  \n[00:21.00]Son");
        assert_eq!(sung(&lines, 1_000), Some((0, 22 * SLOWEST_MS_PER_LETTER)));
        assert_eq!(sung(&lines, 22_000), Some((1_000, 3 * SLOWEST_MS_PER_LETTER)), "the last: that pace too");
    }

    #[test]
    fn query_url_encodes_turkish_and_spaces() {
        assert_eq!(
            query_url("Sezen Aksu", "Şarkı & Söz", "", 201),
            "https://lrclib.net/api/get?artist_name=Sezen%20Aksu&track_name=%C5%9Eark%C4%B1%20%26%20S%C3%B6z&album_name=&duration=201"
        );
    }

    #[test]
    fn search_url_asks_for_title_and_artist() {
        assert_eq!(search_url("Tarkan", "Şımarık"), "https://lrclib.net/api/search?track_name=%C5%9E%C4%B1mar%C4%B1k&artist_name=Tarkan");
    }

    #[test]
    fn clean_title_strips_bracketed_decorations() {
        for (raw, clean) in [
            ("Song (Official Video)", "Song"),
            ("Song [Official Music Video]", "Song"),
            ("Song (Lyrics)", "Song"),
            ("Song (feat. X)", "Song"),
            ("Song (Sped Up)", "Song"),
            ("Song (Remastered 2009)", "Song"),
            ("Song [Lyrics] (Slowed + Reverb)", "Song"),
            ("HEAVENLY JUMPSTYLE (Ultra Slowed)", "HEAVENLY JUMPSTYLE"),
            ("GEM DELÍRIO (Slowed)", "GEM DELÍRIO"),
            ("Ed Sheeran - Shape of You (Official Music Video)", "Ed Sheeran - Shape of You"),
        ] {
            assert_eq!(clean_title(raw), clean, "{raw}");
        }
    }

    #[test]
    fn clean_title_strips_dash_suffix_decorations() {
        for (raw, clean) in [
            ("Bohemian Rhapsody - Remastered 2011", "Bohemian Rhapsody"),
            ("Song - Live at Wembley", "Song"),
            ("Song - Radio Edit", "Song"),
            ("Song - Slowed", "Song"),
            ("Song – 2015 Remaster", "Song"),
            ("Let It Go - From \"Frozen\"", "Let It Go"),
            ("Ed Sheeran - Shape of You", "Ed Sheeran - Shape of You"),
        ] {
            assert_eq!(clean_title(raw), clean, "{raw}");
        }
    }

    #[test]
    fn clean_title_cuts_at_a_pipe_and_a_bare_feat() {
        assert_eq!(clean_title("Song | Official Video"), "Song");
        assert_eq!(clean_title("Despacito ft. Daddy Yankee"), "Despacito");
        assert_eq!(clean_title("Song Feat. X & Y"), "Song");
    }

    #[test]
    fn clean_title_keeps_a_plain_title_and_never_empties_it() {
        assert_eq!(clean_title("Şımarık"), "Şımarık");
        assert_eq!(clean_title("  Kuzu   Kuzu "), "Kuzu Kuzu");
        assert_eq!(clean_title("(I Can't Get No) Satisfaction"), "Satisfaction");
        assert_eq!(clean_title("(Intro)"), "(Intro)");
    }

    #[test]
    fn first_artist_splits_joined_artists() {
        for (raw, first) in [
            ("TWXNY, Sxilwix ve Innxcence", "TWXNY"),
            ("Luis Fonsi & Daddy Yankee", "Luis Fonsi"),
            ("A x B", "A"),
            ("A feat. B", "A"),
            ("Simon and Garfunkel", "Simon"),
            ("Lil Nas X", "Lil Nas X"),
            ("ZXVRF", "ZXVRF"),
        ] {
            assert_eq!(first_artist(raw), first, "{raw}");
        }
    }

    #[test]
    fn first_artist_strips_topic_and_vevo() {
        assert_eq!(first_artist("Tarkan - Topic"), "Tarkan");
        assert_eq!(first_artist("TarkanVEVO"), "Tarkan");
        assert_eq!(first_artist("LuisFonsiVEVO"), "Luis Fonsi");
    }

    #[test]
    fn search_terms_drop_the_artist_prefix_youtube_puts_in_titles() {
        let terms = search_terms;
        assert_eq!(terms("Ed Sheeran", "Ed Sheeran - Shape of You (Official Music Video)"), ("Ed Sheeran".into(), "Shape of You".into()));
        assert_eq!(terms("LuisFonsiVEVO", "Luis Fonsi - Despacito ft. Daddy Yankee"), ("Luis Fonsi".into(), "Despacito".into()));
        assert_eq!(terms("Oasis", "Live Forever - Remastered"), ("Oasis".into(), "Live Forever".into()));
        assert_eq!(terms("TWXNY, Sxilwix ve Innxcence", "HEAVENLY JUMPSTYLE (Ultra Slowed)"), ("TWXNY".into(), "HEAVENLY JUMPSTYLE".into()));
        assert_eq!(terms("Tarkan", "Şımarık"), ("Tarkan".into(), "Şımarık".into()));
    }

    fn record(title: &str, artist: &str, duration: f64, synced: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "trackName": title, "artistName": artist, "duration": duration,
            "instrumental": synced.is_none(), "plainLyrics": synced.map(|_| "words"), "syncedLyrics": synced,
        })
    }

    #[test]
    fn pick_skips_records_without_synced_lyrics() {
        let mut plain = record("Shape of You", "Ed Sheeran", 234.0, None);
        plain["plainLyrics"] = "words only".into();
        let results = serde_json::json!([plain, record("Shape of You", "Ed Sheeran", 234.0, Some(""))]);
        assert_eq!(pick(&results, "Shape of You", "Ed Sheeran", 234), None);
    }

    #[test]
    fn pick_needs_the_duration_within_3_seconds() {
        let results = serde_json::json!([record("Shape of You", "Ed Sheeran", 230.0, Some("[00:01.00]far"))]);
        assert_eq!(pick(&results, "Shape of You", "Ed Sheeran", 234), None);
        let results = serde_json::json!([record("Shape of You", "Ed Sheeran", 236.9, Some("[00:01.00]near"))]);
        assert_eq!(pick(&results, "Shape of You", "Ed Sheeran", 234).as_deref(), Some("[00:01.00]near"));
    }

    #[test]
    fn pick_prefers_the_best_title_match_then_the_closest_duration() {
        let results = serde_json::json!([
            record("Shape of You Acoustic", "Ed Sheeran", 234.0, Some("[00:01.00]acoustic")),
            record("Shape of You", "Ed Sheeran", 236.0, Some("[00:01.00]two off")),
            record("Shape Of You [Shape of You, 2017]", "Ed Sheeran", 235.0, Some("[00:01.00]one off")),
        ]);
        assert_eq!(pick(&results, "Ed Sheeran - Shape of You (Official Music Video)", "Ed Sheeran", 234).as_deref(), Some("[00:01.00]one off"));
    }

    #[test]
    fn pick_folds_case_and_accents() {
        let results = serde_json::json!([record("Simarik", "TARKAN", 235.0, Some("[00:01.00]kiss"))]);
        assert_eq!(pick(&results, "Şımarık", "Tarkan", 235).as_deref(), Some("[00:01.00]kiss"));
        let results = serde_json::json!([record("Gem Delirio", "zxvrf", 140.0, Some("[00:01.00]gem"))]);
        assert_eq!(pick(&results, "GEM DELÍRIO (Slowed)", "ZXVRF", 141).as_deref(), Some("[00:01.00]gem"));
    }

    #[test]
    fn pick_rejects_another_song_or_another_artist() {
        let results = serde_json::json!([
            record("Shape of My Heart", "Sting", 234.0, Some("[00:01.00]sting")),
            record("Shape of You", "Fame on Fire", 234.0, Some("[00:01.00]cover")),
        ]);
        assert_eq!(pick(&results, "Shape of You", "Ed Sheeran", 234), None);
    }

    #[test]
    fn pick_matches_the_first_of_joined_artists() {
        let results = serde_json::json!([record("HEAVENLY JUMPSTYLE (Ultra Slowed)", "TWXNY, Sxilwix & Innxcence", 162.0, Some("[00:01.00]slowed"))]);
        assert_eq!(pick(&results, "HEAVENLY JUMPSTYLE (Ultra Slowed)", "TWXNY, Sxilwix ve Innxcence", 161).as_deref(), Some("[00:01.00]slowed"));
    }

    #[test]
    fn pick_never_puts_the_originals_timing_on_a_slowed_edit() {
        let results = serde_json::json!([
            record("HEAVENLY JUMPSTYLE", "TWXNY", 115.0, Some("[00:01.00]original")),
            record("HEAVENLY JUMPSTYLE (Slowed)", "TWXNY", 128.0, Some("[00:01.00]slowed")),
        ]);
        assert_eq!(pick(&results, "HEAVENLY JUMPSTYLE (Ultra Slowed)", "TWXNY, Sxilwix ve Innxcence", 161), None);
    }

    #[test]
    fn speed_edits_are_told_by_their_title() {
        for title in ["HEAVENLY JUMPSTYLE (Ultra Slowed)", "Song - slowed + reverb", "Song (Sped Up)", "Song [speed up]", "Nightcore - Song"] {
            assert!(is_speed_edit(title), "{title}");
        }
        for title in ["Shape of You", "Slow Dancing in the Dark", "Speed of Sound"] {
            assert!(!is_speed_edit(title), "{title}");
        }
    }

    #[test]
    fn pick_rejects_lyrics_that_run_past_the_end_of_the_track() {
        let results = serde_json::json!([record("Song", "Artist", 200.0, Some("[00:10.00]a\n[03:30.00]too late"))]);
        assert_eq!(pick(&results, "Song", "Artist", 200), None);
    }

    #[test]
    fn pick_rejects_the_originals_lyrics_relabelled_with_a_slowed_edits_length() {
        let original = "[00:01.35]a\n[01:55.44]end";
        let results = serde_json::json!([
            record("HEAVENLY JUMPSTYLE", "TWXNY", 115.0, Some(original)),
            record("HEAVENLY JUMPSTYLE", "TWXNY", 162.0, Some(original)),
        ]);
        assert_eq!(pick(&results, "HEAVENLY JUMPSTYLE (Ultra Slowed)", "TWXNY, Sxilwix ve Innxcence", 161), None);
        // The unslowed track at that length is a different matter: its own record.
        assert_eq!(pick(&results, "HEAVENLY JUMPSTYLE", "TWXNY", 115).as_deref(), Some(original));
    }

    #[test]
    fn pick_takes_a_slowed_edits_own_timing() {
        let results = serde_json::json!([
            record("Metamorphosis", "INTERWORLD", 126.0, Some("[00:05.00]a\n[02:26.88]end")),
            record("METAMORPHOSIS (Slowed + Reverb)", "INTERWORLD", 172.0, Some("[00:06.00]a\n[02:51.03]slow end")),
        ]);
        assert_eq!(pick(&results, "METAMORPHOSIS (Slowed + Reverb)", "INTERWORLD", 172).as_deref(), Some("[00:06.00]a\n[02:51.03]slow end"));
    }

    #[test]
    fn pick_answers_none_for_a_non_array() {
        let error = serde_json::json!({"message": "The server is busy", "statusCode": 503});
        assert_eq!(pick(&error, "Song", "Artist", 200), None);
    }
}
