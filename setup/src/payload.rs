//! The files a setup carries: `NBP1`, the version, then each file's name, size and zlib stream
//! (its Adler-32 catches a cut or damaged download). Shared with build.rs, which packs it.

/// One file out of the payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub bytes: Vec<u8>,
}

const MAGIC: &[u8; 4] = b"NBP1";
/// No exe of ours comes near it: a bigger size is damage, not a file to allocate for.
const MAX_FILE: u64 = 256 << 20;
pub const DAMAGED: &str = "Kurulum dosyası bozuk; yeniden indir.";

/// build.rs packs; the setup only unpacks.
#[cfg_attr(not(test), allow(dead_code))]
pub fn pack(version: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    let put = |out: &mut Vec<u8>, s: &str| {
        out.extend_from_slice(&(s.len() as u16).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    };
    put(&mut out, version);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    for (name, bytes) in files {
        put(&mut out, name);
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(bytes, 9);
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        out.extend_from_slice(&packed);
    }
    out
}

/// The version and the files; `Err(DAMAGED)` for anything short, long or altered.
pub fn unpack(data: &[u8]) -> Result<(String, Vec<File>), String> {
    let mut r = Reader { data, at: 0 };
    if r.take(4)? != MAGIC {
        return Err(DAMAGED.into());
    }
    let version = r.text()?;
    let count = r.u16()?;
    let mut files = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = r.text()?;
        let size = r.u64()?;
        let packed_len = r.u64()?;
        if size > MAX_FILE || packed_len > data.len() as u64 {
            return Err(DAMAGED.into());
        }
        let packed = r.take(packed_len as usize)?;
        let bytes = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(packed, size as usize).map_err(|_| DAMAGED.to_string())?;
        if bytes.len() as u64 != size {
            return Err(DAMAGED.into());
        }
        files.push(File { name, bytes });
    }
    if r.at != data.len() {
        return Err(DAMAGED.into());
    }
    Ok((version, files))
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.data.len()).ok_or(DAMAGED)?;
        let s = &self.data[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| DAMAGED)?))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| DAMAGED)?))
    }

    fn text(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| DAMAGED.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let exe: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        pack("0.3.0", &[("pebbly.exe", &exe), ("pebbly-settings.exe", b"settings")])
    }

    #[test]
    fn round_trip_keeps_names_bytes_and_version() {
        let (version, files) = unpack(&sample()).unwrap();
        assert_eq!(version, "0.3.0");
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["pebbly.exe", "pebbly-settings.exe"]);
        assert_eq!(files[0].bytes.len(), 50_000);
        assert_eq!(files[0].bytes[300], (300 % 251) as u8);
        assert_eq!(files[1].bytes, b"settings");
    }

    #[test]
    fn a_flipped_byte_is_caught() {
        let good = sample();
        // Somewhere inside the first file's zlib stream.
        let mut bad = good.clone();
        let at = good.len() / 3;
        bad[at] ^= 0x40;
        assert_eq!(unpack(&bad), Err(DAMAGED.to_string()));
    }

    #[test]
    fn a_cut_payload_is_caught() {
        let good = sample();
        for n in 0..good.len() {
            assert!(unpack(&good[..n]).is_err(), "cut at {n}");
        }
    }

    #[test]
    fn an_absurd_length_is_refused() {
        let mut bad = pack("1", &[("a", b"abc")]);
        // MAGIC, version (2 + 1), count (2), name (2 + 1): then the size.
        let at = 4 + 3 + 2 + 3;
        bad[at..at + 8].copy_from_slice(&(1u64 << 40).to_le_bytes());
        assert_eq!(unpack(&bad), Err(DAMAGED.to_string()));
    }

    #[test]
    fn an_empty_payload_is_no_files() {
        assert!(unpack(&[]).is_err());
        assert_eq!(unpack(&pack("0.2.0", &[])).unwrap().1, vec![]);
    }
}
