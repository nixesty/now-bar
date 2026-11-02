//! A zip of the shelf's files: names in UTF-8 (Windows' tar writes them in the OEM code page,
//! which a phone reads as garbage), folders with what's in them, already-packed files stored.
//! No ZIP64: past 4 GB or 65 535 entries it says so.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use flate2::write::DeflateEncoder;
use flate2::Compression;

/// Why a zip wasn't made beyond an IO error: it would need ZIP64.
pub const TOO_BIG: &str = "past the zip format's 4 GB";

/// Packed already: deflating them only costs time.
fn packed(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "avif" | "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "flac"
            | "zip" | "rar" | "7z" | "gz" | "xz" | "bz2" | "zst" | "docx" | "xlsx" | "pptx"
    )
}

/// MS-DOS date and time of a local time (unix seconds plus the offset); 1980 at the earliest.
pub fn dos_time(local: i64) -> (u16, u16) {
    let (y, m, d) = super::football::civil_from_days(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    if y < 1980 {
        return ((1 << 5) | 1, 0);
    }
    let date = (((y - 1980).min(127) as u16) << 9) | ((m as u16) << 5) | d as u16;
    let time = (((secs / 3600) as u16) << 11) | ((((secs % 3600) / 60) as u16) << 5) | ((secs % 60) / 2) as u16;
    (date, time)
}

/// The names `items` go into the zip as: their own, "ad (2).txt" for a second of the same.
pub fn entry_names(items: &[PathBuf]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    items
        .iter()
        .map(|p| {
            let name = p.file_name().map_or_else(|| "dosya".to_string(), |n| n.to_string_lossy().into_owned());
            let (stem, ext) = match name.rsplit_once('.') {
                Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
                _ => (name.clone(), String::new()),
            };
            let pick = (1..).map(|n| if n == 1 { name.clone() } else { format!("{stem} ({n}){ext}") }).find(|n| !used.contains(&n.to_lowercase())).unwrap_or(name);
            used.insert(pick.to_lowercase());
            pick
        })
        .collect()
}

struct Entry {
    name: String,
    dir: bool,
    deflated: bool,
    dos: (u16, u16),
    crc: u32,
    packed_size: u64,
    size: u64,
    offset: u64,
}

/// Writes `items` (files and folders) as a zip into `file`, which is `out`; each lands by its
/// name, a folder with everything in it. `offset_s`: the PC's offset from UTC, for the entries'
/// times.
pub fn write_zip(file: File, out: &Path, items: &[PathBuf], offset_s: i64) -> io::Result<()> {
    // Beside the newest, the zip may be inside a folder it zips: read back, it would grow forever.
    let own = std::fs::canonicalize(out).ok();
    let mut w = BufWriter::new(file);
    let mut entries: Vec<Entry> = Vec::new();
    for (item, name) in items.iter().zip(entry_names(items)) {
        add(&mut w, &mut entries, item, &name, offset_s, own.as_deref())?;
    }
    let start = w.stream_position()?;
    for e in &entries {
        let (method, flags) = if e.deflated { (8u16, 0x0800u16) } else { (0, 0x0800) };
        w.write_all(&0x0201_4b50u32.to_le_bytes())?;
        // Made by: version 2.0 on MS-DOS (FAT attributes); needs 2.0.
        for v in [20u16, 20, flags, method, e.dos.1, e.dos.0] {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&e.crc.to_le_bytes())?;
        w.write_all(&small(e.packed_size)?.to_le_bytes())?;
        w.write_all(&small(e.size)?.to_le_bytes())?;
        for v in [e.name.len() as u16, 0, 0, 0, 0] {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&(if e.dir { 0x10u32 } else { 0x20 }).to_le_bytes())?;
        w.write_all(&small(e.offset)?.to_le_bytes())?;
        w.write_all(e.name.as_bytes())?;
    }
    let size = w.stream_position()? - start;
    let count = u16::try_from(entries.len()).map_err(|_| io::Error::other(TOO_BIG))?;
    w.write_all(&0x0605_4b50u32.to_le_bytes())?;
    for v in [0u16, 0, count, count] {
        w.write_all(&v.to_le_bytes())?;
    }
    w.write_all(&small(size)?.to_le_bytes())?;
    w.write_all(&small(start)?.to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?;
    w.flush()
}

/// A size or an offset in the 32 bits a zip without ZIP64 has.
fn small(v: u64) -> io::Result<u32> {
    u32::try_from(v).map_err(|_| io::Error::other(TOO_BIG))
}

/// One file, or a folder and (depth first) everything in it; links and the zip itself (`own`)
/// are left out.
fn add(w: &mut BufWriter<File>, entries: &mut Vec<Entry>, path: &Path, name: &str, offset_s: i64, own: Option<&Path>) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || own.is_some_and(|own| std::fs::canonicalize(path).is_ok_and(|p| p == own)) {
        return Ok(());
    }
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
    let dos = dos_time(modified + offset_s);
    let offset = w.stream_position()?;
    if meta.is_dir() {
        let name = format!("{name}/");
        local_header(w, &name, false, dos, (0, 0, 0))?;
        entries.push(Entry { name: name.clone(), dir: true, deflated: false, dos, crc: 0, packed_size: 0, size: 0, offset });
        let mut children: Vec<PathBuf> = std::fs::read_dir(path)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        children.sort();
        for child in children {
            let child_name = format!("{name}{}", child.file_name().unwrap_or_default().to_string_lossy());
            add(w, entries, &child, &child_name, offset_s, own)?;
        }
        return Ok(());
    }
    let deflated = !packed(name);
    local_header(w, name, deflated, dos, (0, 0, 0))?;
    let data_start = w.stream_position()?;
    let (mut crc, mut size) = (crc32fast::Hasher::new(), 0u64);
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 256 * 1024];
    {
        let mut sink: Box<dyn Write + '_> = if deflated { Box::new(DeflateEncoder::new(&mut *w, Compression::default())) } else { Box::new(&mut *w) };
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            size += n as u64;
            sink.write_all(&buf[..n])?;
        }
        sink.flush()?;
    }
    let end = w.stream_position()?;
    let (crc, packed_size) = (crc.finalize(), end - data_start);
    // The header's crc and sizes, now known.
    w.seek(SeekFrom::Start(offset))?;
    local_header(w, name, deflated, dos, (crc, small(packed_size)?, small(size)?))?;
    w.seek(SeekFrom::Start(end))?;
    entries.push(Entry { name: name.to_string(), dir: false, deflated, dos, crc, packed_size, size, offset });
    Ok(())
}

fn local_header(w: &mut impl Write, name: &str, deflated: bool, (date, time): (u16, u16), (crc, packed_size, size): (u32, u32, u32)) -> io::Result<()> {
    w.write_all(&0x0403_4b50u32.to_le_bytes())?;
    // Bit 11: the name is UTF-8.
    for v in [20u16, 0x0800, if deflated { 8 } else { 0 }, time, date] {
        w.write_all(&v.to_le_bytes())?;
    }
    for v in [crc, packed_size, size] {
        w.write_all(&v.to_le_bytes())?;
    }
    w.write_all(&(name.len() as u16).to_le_bytes())?;
    w.write_all(&0u16.to_le_bytes())?;
    w.write_all(name.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pebbly-zip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The central directory's (name, flags, method, crc, packed, size, offset), read back.
    fn central(bytes: &[u8]) -> Vec<(String, u16, u16, u32, u32, u32, u32)> {
        let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let end = bytes.len() - 22;
        assert_eq!(u32_at(end), 0x0605_4b50, "the end record last");
        let (count, mut at) = (u16_at(end + 10) as usize, u32_at(end + 16) as usize);
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(u32_at(at), 0x0201_4b50);
            let n = u16_at(at + 28) as usize;
            let name = String::from_utf8(bytes[at + 46..at + 46 + n].to_vec()).expect("UTF-8 names");
            out.push((name, u16_at(at + 8), u16_at(at + 10), u32_at(at + 16), u32_at(at + 20), u32_at(at + 24), u32_at(at + 42)));
            at += 46 + n;
        }
        out
    }

    /// An entry's data from its local header.
    fn data(bytes: &[u8], offset: u32, method: u16, packed: u32) -> Vec<u8> {
        let at = offset as usize;
        let skip = 30 + u16::from_le_bytes([bytes[at + 26], bytes[at + 27]]) as usize + u16::from_le_bytes([bytes[at + 28], bytes[at + 29]]) as usize;
        let raw = &bytes[at + skip..at + skip + packed as usize];
        if method == 8 {
            let mut out = Vec::new();
            DeflateDecoder::new(raw).read_to_end(&mut out).unwrap();
            out
        } else {
            raw.to_vec()
        }
    }

    #[test]
    fn files_and_a_folder_round_trip_with_their_names() {
        let d = dir("round");
        let text = "şarkı listesi\n".repeat(500);
        std::fs::write(d.join("şarkı listesi.txt"), &text).unwrap();
        std::fs::create_dir_all(d.join("Klasör").join("alt")).unwrap();
        std::fs::write(d.join("Klasör").join("alt").join("iç.txt"), "iki").unwrap();
        std::fs::write(d.join("ekran.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
        let out = d.join("out.zip");
        write_zip(File::create(&out).unwrap(), &out, &[d.join("şarkı listesi.txt"), d.join("Klasör"), d.join("ekran.png")], 3 * 3600).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let entries = central(&bytes);
        let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, ["şarkı listesi.txt", "Klasör/", "Klasör/alt/", "Klasör/alt/iç.txt", "ekran.png"]);
        assert!(entries.iter().all(|e| e.1 & 0x0800 != 0), "every name flagged UTF-8");
        let text_entry = &entries[0];
        assert_eq!(text_entry.2, 8, "text deflates");
        assert!(text_entry.4 < text_entry.5 / 10, "and shrinks: {} of {}", text_entry.4, text_entry.5);
        assert_eq!(data(&bytes, text_entry.6, text_entry.2, text_entry.4), text.as_bytes());
        assert_eq!(text_entry.3, crc32fast::hash(text.as_bytes()));
        let inner = &entries[3];
        assert_eq!(data(&bytes, inner.6, inner.2, inner.4), b"iki");
        assert_eq!(entries[4].2, 0, "a PNG is stored");
        assert_eq!(data(&bytes, entries[4].6, 0, entries[4].4), [0x89, b'P', b'N', b'G', 1, 2, 3]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A zip to open elsewhere (Explorer, a phone): `PEBBLY_ZIP_OUT=<file> PEBBLY_ZIP_IN=<a;b>
    /// cargo t --lib live_zip -- --ignored`.
    #[test]
    #[ignore]
    fn live_zip() {
        let out = PathBuf::from(std::env::var_os("PEBBLY_ZIP_OUT").expect("PEBBLY_ZIP_OUT"));
        let items: Vec<PathBuf> = std::env::var("PEBBLY_ZIP_IN").expect("PEBBLY_ZIP_IN").split(';').map(PathBuf::from).collect();
        write_zip(File::create(&out).unwrap(), &out, &items, crate::app::tz_offset_s()).unwrap();
    }

    #[test]
    fn a_zip_inside_a_folder_it_zips_leaves_itself_out() {
        let d = dir("inside");
        std::fs::create_dir_all(d.join("Tatil")).unwrap();
        std::fs::write(d.join("Tatil").join("foto.jpg"), [1, 2, 3]).unwrap();
        let out = d.join("Tatil").join("foto ve 1 dosya.zip");
        write_zip(File::create(&out).unwrap(), &out, &[d.join("Tatil"), d.join("Tatil").join("foto.jpg")], 0).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let names: Vec<String> = central(&bytes).into_iter().map(|e| e.0).collect();
        assert_eq!(names, ["Tatil/", "Tatil/foto.jpg", "foto.jpg"], "not itself, as it was being written");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_of_the_same_name_both_go_in() {
        let items = [PathBuf::from(r"C:\a\rapor.pdf"), PathBuf::from(r"D:\b\rapor.pdf"), PathBuf::from(r"D:\b\RAPOR.pdf"), PathBuf::from(r"C:\Makefile")];
        assert_eq!(entry_names(&items), ["rapor.pdf", "rapor (2).pdf", "RAPOR (3).pdf", "Makefile"]);
    }

    #[test]
    fn dos_times() {
        // 25 October 2026, 14:05:31 local.
        let local = 1_792_886_400 + 14 * 3600 + 5 * 60 + 31;
        assert_eq!(dos_time(local), (((2026 - 1980) << 9) | (10 << 5) | 25, (14 << 11) | (5 << 5) | 15));
        assert_eq!(dos_time(0), ((1 << 5) | 1, 0), "before 1980: its first day");
    }
}
