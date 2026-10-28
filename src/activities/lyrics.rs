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

/// A record another source's search found: its title, artist and length, and where its
/// lyrics are.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub title: String,
    pub artist: String,
    pub duration_s: f64,
    pub lyrics_url: String,
}

/// The sources asked after lrclib, in this order. Neither wants a key; both carry many records
/// lrclib lacks, slowed and sped-up releases among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    NetEase,
    Kugou,
}

impl Source {
    pub const OTHERS: [Source; 2] = [Source::NetEase, Source::Kugou];

    /// The search for "First artist - Title" (the way Kugou wants it): the cleaned title, but an
    /// edit's as it is, since what makes it one is what finds it.
    pub fn search_url(self, artist: &str, title: &str, duration_s: u64) -> String {
        let (first, clean) = search_terms(artist, title);
        let title = if is_speed_edit(title) { without_artist(artist, title) } else { &clean };
        let keyword = encode(&format!("{first} - {title}"));
        match self {
            Source::NetEase => format!("https://music.163.com/api/cloudsearch/pc?s={keyword}&type=1&limit=10"),
            Source::Kugou => format!("https://lyrics.kugou.com/search?ver=1&man=yes&client=pc&keyword={keyword}&duration={}&hash=", duration_s * 1000),
        }
    }

    /// The records a search answered (`None`: not an answer, such as a busy server's page).
    pub fn found(self, body: &str) -> Option<Vec<Found>> {
        let json: serde_json::Value = serde_json::from_str(body).ok()?;
        let records = match self {
            Source::NetEase => &json["result"]["songs"],
            Source::Kugou => &json["candidates"],
        };
        let read = |r: &serde_json::Value| -> Option<Found> {
            Some(match self {
                Source::NetEase => Found {
                    title: r["name"].as_str()?.to_string(),
                    artist: r["ar"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", "),
                    duration_s: r["dt"].as_f64()? / 1000.0,
                    lyrics_url: format!("https://music.163.com/api/song/lyric?id={}&lv=1", r["id"].as_u64()?),
                },
                Source::Kugou => Found {
                    title: r["song"].as_str()?.to_string(),
                    artist: r["singer"].as_str()?.to_string(),
                    duration_s: r["duration"].as_f64()? / 1000.0,
                    lyrics_url: format!(
                        "https://lyrics.kugou.com/download?ver=1&client=pc&id={}&accesskey={}&fmt=lrc&charset=utf8",
                        encode(r["id"].as_str()?),
                        encode(r["accesskey"].as_str()?)
                    ),
                },
            })
        };
        Some(records.as_array().map(|a| a.iter().filter_map(read).collect()).unwrap_or_default())
    }

    /// The LRC text in a lyrics answer (Kugou's in base64), if it has one.
    pub fn lrc(self, body: &str) -> Option<String> {
        let json: serde_json::Value = serde_json::from_str(body).ok()?;
        let lrc = match self {
            Source::NetEase => json["lrc"]["lyric"].as_str()?.to_string(),
            Source::Kugou => String::from_utf8(crate::activities::api::base64(json["content"].as_str()?)?).ok()?,
        };
        Some(lrc.trim_start_matches('\u{feff}').to_string())
    }
}

/// The records in `found` that fit the track, best first (`pick`'s order: the best title and
/// artist match, then the closest length): within `DURATION_SLACK_S` of its length, and an
/// edit when it is a slowed or sped-up edit, never one when it isn't. These sources' lyrics
/// come only after the pick, so another version's timing can't be told by its lines, as
/// `pick` tells it.
pub fn rank<'a>(found: &'a [Found], title: &str, artist: &str, duration_s: u64) -> Vec<&'a Found> {
    let (our_artist, our_title) = search_terms(artist, title);
    let edit = is_speed_edit(title);
    let mut fits: Vec<(u8, f64, &Found)> = found
        .iter()
        .filter(|f| is_speed_edit(&f.title) == edit)
        .filter_map(|f| {
            let off = (f.duration_s - duration_s as f64).abs();
            let score = fit(&our_title, &our_artist, &f.title, &f.artist);
            (off <= DURATION_SLACK_S && score > 0).then_some((score, off, f))
        })
        .collect();
    fits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
    fits.into_iter().map(|(_, _, f)| f).collect()
}

/// Credits some records put among their lines in English ("Lyricist : …").
const CREDITS: &[&str] = &[
    "lyricist", "lyrics", "lyrics by", "composer", "composed by", "music", "music by", "written by", "writer", "producer", "produced by",
    "arranger", "arranged by", "mixed by", "mastered by",
];

/// Another source's LRC made fit for the pill and the card: without its credits ("作曲 : …",
/// "Lyricist : …"), the "Title - Artist" line some open with and a "pure music" note
/// ("纯音乐"), and with full-width commas in Latin lines as ", ". `None` when no timed line with
/// words is left, or the lines run past the track's end (another length's timing).
pub fn tidy(lrc: &str, title: &str, artist: &str, duration_s: u64) -> Option<String> {
    let (first, clean) = (first_artist(artist), clean_title(title));
    let names = [format!("{clean} {first}"), format!("{first} {clean}"), format!("{title} {artist}"), format!("{artist} {title}")].map(|n| norm(&n));
    let lines: Lines = parse_lrc(lrc)
        .into_iter()
        .filter(|(_, words)| !is_credit(words) && !words.contains("纯音乐") && !names.contains(&norm(words)))
        .map(|(ms, words)| match words.chars().any(is_han) {
            true => (ms, words),
            false => (ms, words.replace('，', ", ").split_whitespace().collect::<Vec<_>>().join(" ")),
        })
        .collect();
    let past_end = lines.last().is_some_and(|(ms, _)| *ms as f64 > (duration_s as f64 + DURATION_SLACK_S) * 1000.0);
    (lines.iter().any(|(_, words)| !words.is_empty()) && !past_end).then(|| lrc_text(&lines))
}

/// "作曲 : Ekin", "Lyricist : Deniz": a key in Chinese characters or a credit's English name,
/// then a colon.
fn is_credit(words: &str) -> bool {
    let Some(at) = words.find([':', '：']) else { return false };
    let key = words[..at].trim();
    !key.is_empty() && ((key.chars().count() <= 8 && key.chars().all(is_han)) || CREDITS.contains(&key.to_lowercase().as_str()))
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}')
}

/// For a slowed or sped-up edit no source has: the original's lines from lrclib's search
/// `results`, every time stretched by how much longer (or shorter) the edit is, since an edit
/// plays the whole song slower (or faster) alike. The original's length is the one most of
/// lrclib's unedited records of the song share: copies filed under other lengths scatter, those
/// under the edit's own length are copies (it has none of its own, or `pick` would have taken
/// it), and one whose lines run past its own length doesn't count. `None` for a track that
/// isn't an edit, without an original, or more than twice as slow or fast as it.
pub fn stretched(results: &serde_json::Value, title: &str, artist: &str, duration_s: u64) -> Option<String> {
    if !is_speed_edit(title) {
        return None;
    }
    let (our_artist, our_title) = search_terms(artist, title);
    let originals: Vec<(f64, Lines)> = results
        .as_array()?
        .iter()
        .filter_map(|r| {
            let name = r["trackName"].as_str()?;
            let length = r["duration"].as_f64()?;
            let lines = parse_lrc(r["syncedLyrics"].as_str()?);
            let last = lines.last()?.0;
            let fits = !is_speed_edit(name) && fit(&our_title, &our_artist, name, r["artistName"].as_str().unwrap_or_default()) > 0;
            let other_length = (length - duration_s as f64).abs() > DURATION_SLACK_S;
            (fits && other_length && last as f64 <= (length + DURATION_SLACK_S) * 1000.0).then_some((length, lines))
        })
        .collect();
    let sharing = |length: f64| originals.iter().filter(|(l, _)| (l - length).abs() <= DURATION_SLACK_S).count();
    // The first of the most shared.
    let (length, lines) = originals.iter().fold(None, |best: Option<&(f64, Lines)>, o| match best {
        Some(b) if sharing(b.0) >= sharing(o.0) => Some(b),
        _ => Some(o),
    })?;
    let ratio = duration_s as f64 / length;
    if !(0.5..=2.0).contains(&ratio) {
        return None;
    }
    Some(lrc_text(&lines.iter().map(|(ms, words)| ((*ms as f64 * ratio).round() as u64, words.clone())).collect()))
}

/// Lines back as LRC text, "[mm:ss.xxx]" a line.
fn lrc_text(lines: &Lines) -> String {
    lines.iter().map(|(ms, words)| format!("[{:02}:{:02}.{:03}]{words}\n", ms / 60_000, ms / 1000 % 60, ms % 1000)).collect()
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
    (first_artist(artist), clean_title(without_artist(artist, title)))
}

/// A title without the "Artist - " that YouTube video titles start with.
fn without_artist<'a>(artist: &str, title: &'a str) -> &'a str {
    let first = first_artist(artist);
    let title = title.trim();
    if let Some((head, tail)) = [" - ", " – ", " — "].iter().find_map(|d| title.split_once(d)) {
        let same = |a: &str, b: &str| squash(a) == squash(b);
        if same(head, &first) || same(head, artist) || same(&first_artist(head), &first) {
            return tail;
        }
    }
    title
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

/// How well a record's title and artist match ours (as `search_terms` gives them): their
/// likenesses added, 0 when either doesn't match at all.
fn fit(our_title: &str, our_artist: &str, their_title: &str, their_artist: &str) -> u8 {
    let title = likeness(our_title, &clean_title(their_title));
    let artist = likeness(our_artist, &first_artist(their_artist)).max(likeness(our_artist, their_artist).min(1));
    if title == 0 || artist == 0 {
        0
    } else {
        title + artist
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
        let score = fit(&our_title, &our_artist, r["trackName"].as_str().unwrap_or_default(), r["artistName"].as_str().unwrap_or_default());
        if score == 0 {
            continue;
        }
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
    fn other_sources_search_for_the_first_artist_and_the_title() {
        assert_eq!(
            Source::NetEase.search_url("Ekin, Deniz", "Ekin - Gece Treni (Official Video)", 200),
            "https://music.163.com/api/cloudsearch/pc?s=Ekin%20-%20Gece%20Treni&type=1&limit=10"
        );
        // An edit keeps what makes it one: Kugou finds only the title as it is filed.
        assert_eq!(
            Source::Kugou.search_url("Ekin", "Gece Treni (Slowed + Reverb)", 278),
            "https://lyrics.kugou.com/search?ver=1&man=yes&client=pc&keyword=Ekin%20-%20Gece%20Treni%20%28Slowed%20%2B%20Reverb%29&duration=278000&hash="
        );
    }

    #[test]
    fn other_sources_answers_are_read() {
        let netease = r#"{"result":{"songs":[{"id":101,"name":"Gece Treni","ar":[{"name":"Ekin"},{"name":"Deniz"}],"dt":236363}],"songCount":1},"code":200}"#;
        assert_eq!(
            Source::NetEase.found(netease),
            Some(vec![Found { title: "Gece Treni".into(), artist: "Ekin, Deniz".into(), duration_s: 236.363, lyrics_url: "https://music.163.com/api/song/lyric?id=101&lv=1".into() }])
        );
        assert_eq!(Source::NetEase.found(r#"{"result":{"songCount":0},"code":200}"#), Some(vec![]));
        let kugou = r#"{"status":200,"candidates":[{"id":"202","accesskey":"AB12","singer":"Ekin","song":"Gece Treni (Slowed + Reverb)","duration":278126}]}"#;
        assert_eq!(
            Source::Kugou.found(kugou),
            Some(vec![Found {
                title: "Gece Treni (Slowed + Reverb)".into(),
                artist: "Ekin".into(),
                duration_s: 278.126,
                lyrics_url: "https://lyrics.kugou.com/download?ver=1&client=pc&id=202&accesskey=AB12&fmt=lrc&charset=utf8".into()
            }])
        );
        assert_eq!(Source::Kugou.found("<html>busy</html>"), None, "not an answer: asked again later");
    }

    #[test]
    fn other_sources_lyrics_are_read() {
        assert_eq!(Source::NetEase.lrc(r#"{"lrc":{"version":3,"lyric":"[00:12.00]Raylar uzar\n"},"code":200}"#).as_deref(), Some("[00:12.00]Raylar uzar\n"));
        assert_eq!(Source::NetEase.lrc(r#"{"nolyric":true,"code":200}"#), None);
        // Base64, with a byte-order mark.
        let kugou = r#"{"status":200,"fmt":"lrc","content":"77u/W3RpOkdlY2UgVHJlbmldCltvZmZzZXQ6MF0KWzAwOjAwLjAwXUdlY2UgVHJlbmkgLSBFa2luClswMDoxMi4wMF1SYXlsYXIgdXphcgo="}"#;
        assert_eq!(Source::Kugou.lrc(kugou).as_deref(), Some("[ti:Gece Treni]\n[offset:0]\n[00:00.00]Gece Treni - Ekin\n[00:12.00]Raylar uzar\n"));
    }

    fn found(title: &str, artist: &str, duration_s: f64) -> Found {
        Found { title: title.into(), artist: artist.into(), duration_s, lyrics_url: format!("{title} {duration_s}") }
    }

    #[test]
    fn an_edit_takes_only_an_edits_record_and_the_original_only_an_unedited_one() {
        let records = [
            found("Gece Treni", "Ekin", 278.1),
            found("Gece Treni (Slowed + Reverb)", "Ekin", 278.0),
            found("Gece Treni (Sped Up)", "Ekin", 236.0),
            found("Gece Treni", "Ekin", 236.4),
            found("Gece Treni (Slowed)", "Ekin", 290.0),
            found("Başka Şarkı (Slowed)", "Ekin", 278.0),
        ];
        let urls = |ranked: Vec<&Found>| ranked.into_iter().map(|f| f.lyrics_url.clone()).collect::<Vec<_>>();
        assert_eq!(urls(rank(&records, "Gece Treni (Slowed + Reverb)", "Ekin", 278)), ["Gece Treni (Slowed + Reverb) 278"]);
        assert_eq!(urls(rank(&records, "Ekin - Gece Treni (Official Video)", "Ekin", 236)), ["Gece Treni 236.4"]);
    }

    #[test]
    fn tidy_drops_credits_and_the_title_line_and_spaces_full_width_commas() {
        let lrc = "[00:00.000] 作曲 : Ekin\n[00:00.500] 作词 : Deniz\n[00:00.900]Gece Treni - Ekin\n[00:01.000]Lyricist : Deniz\n\
                   [00:12.000]Raylar uzar，Gece biter\n[00:15.000]寂静的夜，你在哪里\n[00:20.000]Note: bu bir söz\n[00:30.000]";
        assert_eq!(
            tidy(lrc, "Gece Treni (Slowed + Reverb)", "Ekin", 200).as_deref(),
            Some("[00:12.000]Raylar uzar, Gece biter\n[00:15.000]寂静的夜，你在哪里\n[00:20.000]Note: bu bir söz\n[00:30.000]\n")
        );
    }

    #[test]
    fn tidy_refuses_pure_music_and_lines_past_the_end() {
        assert_eq!(tidy("[00:00.000] 作曲 : Ekin\n[00:01.000] 纯音乐，请欣赏", "Gece Treni", "Ekin", 200), None);
        assert_eq!(tidy("[00:12.000]Raylar uzar\n[03:50.000]Son", "Gece Treni", "Ekin", 200), None, "another length's timing");
        assert_eq!(tidy("Raylar uzar\nSon", "Gece Treni", "Ekin", 200), None, "plain lyrics");
    }

    #[test]
    fn a_slowed_edit_no_source_has_gets_the_originals_lines_stretched() {
        let original = "[00:20.00]Raylar uzar\n[03:51.00]Son";
        let results = serde_json::json!([record("Gece Treni", "Ekin", 236.0, Some(original))]);
        // 278 / 236: every line comes that much later.
        assert_eq!(stretched(&results, "Gece Treni (Slowed + Reverb)", "Ekin", 278).as_deref(), Some("[00:23.559]Raylar uzar\n[04:32.110]Son\n"));
    }

    #[test]
    fn the_stretch_takes_the_length_most_originals_share() {
        let lyrics = "[00:20.00]Raylar uzar\n[03:51.00]Son";
        let results = serde_json::json!([
            // Copies filed under other lengths: one too short for its own lines, one an album's.
            record("Gece Treni", "Ekin", 213.0, Some(lyrics)),
            record("Gece Treni", "Ekin", 278.0, Some(lyrics)),
            record("Gece Treni", "Ekin", 236.0, Some(lyrics)),
            record("Gece Treni", "Ekin", 236.4, Some(lyrics)),
            record("Gece Treni (Slowed)", "Ekin", 300.0, Some("[00:30.00]yavaş")),
        ]);
        assert_eq!(stretched(&results, "Gece Treni (Slowed + Reverb)", "Ekin", 278).as_deref(), Some("[00:23.559]Raylar uzar\n[04:32.110]Son\n"));
    }

    #[test]
    fn copies_filed_under_the_edits_own_length_are_no_original() {
        let lyrics = "[00:20.00]Raylar uzar\n[03:51.00]Son";
        let results = serde_json::json!([
            record("Gece Treni", "Ekin", 278.0, Some(lyrics)),
            record("Gece Treni", "Ekin", 278.5, Some(lyrics)),
            record("Gece Treni", "Ekin", 236.0, Some(lyrics)),
        ]);
        assert_eq!(stretched(&results, "Gece Treni (Slowed + Reverb)", "Ekin", 278).as_deref(), Some("[00:23.559]Raylar uzar\n[04:32.110]Son\n"));
    }

    #[test]
    fn no_stretch_for_an_unedited_track_or_an_edit_as_long_as_the_original() {
        let results = serde_json::json!([record("Gece Treni", "Ekin", 236.0, Some("[00:20.00]Raylar uzar\n[03:51.00]Son"))]);
        assert_eq!(stretched(&results, "Gece Treni", "Ekin", 278), None);
        assert_eq!(stretched(&results, "Gece Treni (Slowed)", "Ekin", 237), None);
        assert_eq!(stretched(&results, "Gece Treni (Ultra Slowed)", "Ekin", 600), None, "no edit is that slow");
    }

    #[test]
    fn pick_answers_none_for_a_non_array() {
        let error = serde_json::json!({"message": "The server is busy", "statusCode": 503});
        assert_eq!(pick(&error, "Song", "Artist", 200), None);
    }
}
