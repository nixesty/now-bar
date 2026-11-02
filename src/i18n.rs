//! The interface's language. Every string in the code is Turkish (the source); English comes from
//! `i18n/en/LC_MESSAGES/pebbly-ui.po`, built into a table at compile time. The settings window's
//! Slint strings use the same file through Slint's bundled translations.

use std::cell::Cell;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[path = "po.rs"]
mod po;
pub use po::parse_po;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    Turkish,
    English,
}

impl Language {
    /// Its ISO 639-1 code, as web services take it.
    pub fn code(self) -> &'static str {
        match self {
            Language::Turkish => "tr",
            Language::English => "en",
        }
    }
}

/// The settings' choice: Windows' own language, or one of the two.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePick {
    #[default]
    Auto,
    Turkish,
    English,
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// A test's language (tests run in parallel threads; the process-wide one stays Turkish).
    static OVERRIDE: Cell<Option<Language>> = const { Cell::new(None) };
}

include!(concat!(env!("OUT_DIR"), "/en.rs"));

/// The language the interface speaks now.
pub fn lang() -> Language {
    OVERRIDE.with(Cell::get).unwrap_or(if LANGUAGE.load(Ordering::Relaxed) == 1 { Language::English } else { Language::Turkish })
}

pub fn set_language(l: Language) {
    LANGUAGE.store(u8::from(l == Language::English), Ordering::Relaxed);
}

/// The language a setting picks: for `Auto`, Turkish only on a Turkish Windows.
pub fn pick(p: LanguagePick, windows_turkish: bool) -> Language {
    match p {
        LanguagePick::Turkish => Language::Turkish,
        LanguagePick::English => Language::English,
        LanguagePick::Auto if windows_turkish => Language::Turkish,
        LanguagePick::Auto => Language::English,
    }
}

/// The language a setting gives on this PC: `PEBBLY_LANG` ("tr" or "en": the dev sheets and the
/// setup's shots in either language) first, else [`pick`] with Windows' own.
pub fn chosen(p: LanguagePick) -> Language {
    match std::env::var("PEBBLY_LANG").as_deref() {
        Ok("en") => Language::English,
        Ok("tr") => Language::Turkish,
        _ => pick(p, windows_is_turkish()),
    }
}

/// Runs `f` with this thread speaking `l` (tests: the process-wide language stays as it is).
pub fn in_language<R>(l: Language, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Language>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(OVERRIDE.with(|o| o.replace(Some(l))));
    f()
}

/// `s` in the interface's language: the English for it, or `s` itself (in Turkish, and for a
/// string the `.po` doesn't have yet).
pub fn tr(s: &str) -> &str {
    if lang() == Language::Turkish {
        return s;
    }
    EN.binary_search_by(|(turkish, _)| (*turkish).cmp(s)).map_or(s, |i| EN[i].1)
}

/// `s` in the interface's language where the same Turkish means another thing elsewhere ("Kapalı":
/// "Off" for a switch, "Overcast" for the sky): the `.po` entry with `ctx` as its msgctxt.
pub fn trc<'a>(ctx: &str, s: &'a str) -> &'a str {
    if lang() == Language::Turkish {
        return s;
    }
    let key = || ctx.bytes().chain(std::iter::once(4u8)).chain(s.bytes());
    EN.binary_search_by(|(k, _)| k.bytes().cmp(key())).map_or(s, |i| EN[i].1)
}

/// `pattern` in the interface's language, its holes filled with `args` (see [`fill`]).
pub fn trf(pattern: &str, args: &[&dyn Display]) -> String {
    fill(tr(pattern), args)
}

/// Marks a string kept in a table as one to translate where it's shown (`tr(TABLE[i])`): the
/// completeness test and the extractor find it.
pub const fn n_(s: &str) -> &str {
    s
}

/// "%48" in Turkish, "48%" in English.
pub fn percent(v: impl Display) -> String {
    match lang() {
        Language::Turkish => format!("%{v}"),
        Language::English => format!("{v}%"),
    }
}

/// The language's decimal separator.
pub fn decimal_sep() -> char {
    match lang() {
        Language::Turkish => ',',
        Language::English => '.',
    }
}

/// One decimal with the language's separator: "1,2" / "1.2".
pub fn decimal1(v: f64) -> String {
    let s = format!("{v:.1}");
    match lang() {
        Language::Turkish => s.replace('.', ","),
        Language::English => s,
    }
}

/// Fills a pattern's holes: each `{}` takes the next argument, `{0}`, `{1}`… a given one (an
/// English sentence may put them in another order than the Turkish).
pub fn fill(pattern: &str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let (mut at, mut next) = (0, 0);
    while at < pattern.len() {
        let rest = &pattern[at..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            out.push_str(&rest[..1]);
            at += 2;
            continue;
        }
        if let Some(end) = rest.strip_prefix('{').and_then(|r| r.find('}')) {
            let inner = &rest[1..=end];
            let index = if inner.is_empty() { Some(next) } else { inner.parse::<usize>().ok() };
            if let Some(i) = index {
                next += usize::from(inner.is_empty());
                match args.get(i) {
                    Some(a) => out.push_str(&a.to_string()),
                    None => out.push_str(&rest[..end + 2]),
                }
                at += end + 2;
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        at += c.len_utf8();
    }
    out
}

#[link(name = "kernel32")]
extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

/// Windows' user interface language is Turkish.
pub fn windows_is_turkish() -> bool {
    // LANG_TURKISH in the low 10 bits of the language id.
    unsafe { GetUserDefaultUILanguage() & 0x3FF == 0x1F }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_po_file_gives_its_translated_entries() {
        let po = concat!(
            "# A comment\n",
            "msgid \"\"\n",
            "msgstr \"\"\n",
            "\"Content-Type: text/plain; charset=UTF-8\\n\"\n",
            "\n",
            "#: src/app.rs\n",
            "msgid \"Otomatik\"\n",
            "msgstr \"Automatic\"\n",
            "\n",
            "msgid \"\"\n",
            "\"Uzun \"\n",
            "\"bir satır\"\n",
            "msgstr \"A long \"\n",
            "\"line\"\n",
            "\n",
            "msgid \"\\\"Tırnak\\\" ve \\\\ ters\\n\"\n",
            "msgstr \"\\\"Quote\\\" and \\\\ back\\n\"\n",
            "\n",
            "msgid \"Henüz yok\"\n",
            "msgstr \"\"\n",
            "\n",
            "msgctxt \"menu\"\n",
            "msgid \"Aç\"\n",
            "msgstr \"Open\"\n",
            "\n",
            "msgid \"{} dosya\"\n",
            "msgid_plural \"{} dosya\"\n",
            "msgstr[0] \"{} file\"\n",
            "msgstr[1] \"{} files\"\n",
        );
        assert_eq!(
            parse_po(po),
            vec![
                ("Otomatik".to_string(), "Automatic".to_string()),
                ("Uzun bir satır".into(), "A long line".into()),
                ("\"Tırnak\" ve \\ ters\n".into(), "\"Quote\" and \\ back\n".into()),
                ("menu\u{4}Aç".into(), "Open".into()),
            ],
            "the header, an untranslated entry and a plural are left out; a context goes before its msgid"
        );
        assert!(parse_po("").is_empty());
    }

    #[test]
    fn holes_are_filled_in_order_or_by_index() {
        assert_eq!(fill("{} dk", &[&5]), "5 dk");
        assert_eq!(fill("%{} · {}", &[&48, &"18 MB/sn"]), "%48 · 18 MB/sn");
        assert_eq!(fill("{1}, {0}", &[&"a", &"b"]), "b, a", "an English order of its own");
        assert_eq!(fill("{} ve {}", &[&1]), "1 ve {}", "a hole without an argument stays");
        assert_eq!(fill("{{}} {}", &[&3]), "{} 3", "doubled braces are braces");
        assert_eq!(fill("yok", &[&1]), "yok");
    }

    #[test]
    fn the_language_follows_the_setting_or_windows() {
        assert_eq!(pick(LanguagePick::Auto, true), Language::Turkish);
        assert_eq!(pick(LanguagePick::Auto, false), Language::English);
        assert_eq!(pick(LanguagePick::English, true), Language::English);
        assert_eq!(pick(LanguagePick::Turkish, false), Language::Turkish);
    }

    /// The string literal starting at `text[at]` (a quote), escapes undone, and where it ends.
    fn literal(text: &str, at: usize) -> Option<(String, usize)> {
        let mut s = String::new();
        let mut chars = text[at + 1..].char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((s, at + 1 + i + 1)),
                '\\' => match chars.next().map(|(_, c)| c) {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('u') => {
                        let hex: String = chars.by_ref().skip(1).map(|(_, c)| c).take_while(|c| *c != '}').collect();
                        s.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                    }
                    Some(other) => s.push(other),
                    None => {}
                },
                c => s.push(c),
            }
        }
        None
    }

    /// The keys of the strings the code puts through `tr(`, `trf(`, `n_(`, `trc(ctx, …)` and
    /// Slint's `@tr(…)` / `@tr(ctx => …)`: "context\u{4}text" for those with a context.
    fn wrapped(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        for call in ["tr(\"", "trf(\"", "n_(\"", "trc(\""] {
            for (at, _) in text.match_indices(call) {
                if text[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    continue;
                }
                let Some((first, end)) = literal(text, at + call.len() - 1) else { continue };
                let rest = text[end..].trim_start();
                // A second literal after "," (trc) or "=>" (Slint) is the text; the first its context.
                let joint = if call == "trc(\"" { rest.strip_prefix(',') } else { rest.strip_prefix("=>") };
                let second = joint.map(str::trim_start).filter(|r| r.starts_with('"')).and_then(|r| literal(r, 0));
                out.push(match second {
                    Some((text, _)) => format!("{first}\u{4}{text}"),
                    None => first,
                });
            }
        }
        out
    }

    fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs" || x == "slint") {
                out.push(path);
            }
        }
    }

    /// Every string the code puts through `tr`, `trf` or `@tr` has its English in the `.po`
    /// (`tools/i18n-extract.py` adds the missing ones, empty, to fill in).
    #[test]
    fn every_string_has_english() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        for dir in ["src", "ui", "setup/src", "setup/ui"] {
            sources(&root.join(dir), &mut files);
        }
        let mut missing = Vec::new();
        for f in files.iter().filter(|f| !f.ends_with("i18n.rs") && !f.ends_with("po.rs")) {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            for s in wrapped(&text) {
                if EN.binary_search_by(|(turkish, _)| (*turkish).cmp(s.as_str())).is_err() {
                    missing.push(format!("{}: {s:?}", f.strip_prefix(root).unwrap_or(f).display()));
                }
            }
        }
        assert!(missing.is_empty(), "no English for {} strings (tools/i18n-extract.py):\n{}", missing.len(), missing.join("\n"));
    }

    #[test]
    fn literals_are_found_as_the_code_writes_them() {
        let code = r#"tr("Bir"), i18n::trf("{} dk", &[&n]); title: @tr("Dil"); attr("x"); s.tr("y"); tr("Tırnak \"iç\"\n")"#;
        assert_eq!(wrapped(code), ["Bir", "Dil", "Tırnak \"iç\"\n", "{} dk"], "a method named tr isn't ours");
        let contexts = r#"trc("hava", "Kapalı"); text: @tr("menü" => "Aç"); @tr("Tek")"#;
        assert_eq!(wrapped(contexts), ["menü\u{4}Aç", "Tek", "hava\u{4}Kapalı"]);
    }

    #[test]
    fn numbers_read_the_languages_way() {
        assert_eq!((percent(48), decimal1(1.23)), ("%48".to_string(), "1,2".to_string()));
        in_language(Language::English, || assert_eq!((percent(48), decimal1(1.23)), ("48%".to_string(), "1.2".to_string())));
    }

    #[test]
    fn strings_are_english_only_in_english() {
        assert_eq!(tr("Otomatik"), "Otomatik", "Turkish by default");
        in_language(Language::English, || {
            assert_eq!(lang(), Language::English);
            assert_eq!(tr("Otomatik"), "Automatic");
            assert_eq!(tr("Bu metin hiç çevrilmedi"), "Bu metin hiç çevrilmedi", "no English yet: the Turkish");
            assert_eq!(trf("{} dk", &[&5]), "5 min");
            assert_eq!((tr("Kapalı"), trc("hava", "Kapalı")), ("Off", "Overcast"), "the same Turkish, two Englishes");
        });
        assert_eq!(trc("hava", "Kapalı"), "Kapalı");
        assert_eq!(lang(), Language::Turkish, "the thread's language is back");
    }
}
