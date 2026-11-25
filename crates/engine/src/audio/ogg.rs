//! Opus files for Symphonia's Ogg reader, which refuses packets over 16 MiB.
//! A cover in the comment header can be bigger (#137).

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

use symphonia::core::{checksum::Crc32, io::MediaSource, io::Monitor as _};

use crate::Source;

/// The bytes of `source` to demux. In Ogg Opus whose comment header spans pages
/// (a large embedded cover), that header reads as an empty one. Its first
/// page is shortened in place and nothing after it says "OggS" until the
/// audio, so the reader skips the rest while looking for the next page; every
/// other byte keeps its offset, so seeking still works. The engine reads no
/// tags from it. Opening such a file reads its whole comment header once.
pub fn source(source: &Source) -> io::Result<Box<dyn MediaSource>> {
    match source {
        Source::File(path) => {
            let file = File::open(path)?;
            let len = file.metadata()?.len();
            patched(file, len)
        }
        Source::Stream { open, .. } => {
            let stream = open()?;
            let len = stream.byte_len();
            patched(stream, len)
        }
    }
}

fn patched<R: Read + Seek + Send + Sync + 'static>(
    mut file: R,
    len: u64,
) -> io::Result<Box<dyn MediaSource>> {
    let patches = empty_comment(&mut file).unwrap_or_default();
    file.rewind()?;
    Ok(Box::new(Patched {
        file,
        pos: 0,
        len,
        patches,
    }))
}

/// An Ogg page header and its lacing values.
struct Page {
    header: [u8; 27],
    lacing: Vec<u8>,
}

impl Page {
    fn read(file: &mut impl ReadSeek, at: u64) -> io::Result<Self> {
        let mut header = [0; 27];
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(&mut header)?;
        if &header[..4] != b"OggS" {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut lacing = vec![0; header[26] as usize];
        file.read_exact(&mut lacing)?;
        Ok(Self { header, lacing })
    }

    fn len(&self) -> u64 {
        (27 + self.lacing.len() + self.lacing.iter().map(|&l| l as usize).sum::<usize>()) as u64
    }

    /// Whether its last packet goes on in the next page.
    fn continues(&self) -> bool {
        self.lacing.last() == Some(&255)
    }
}

/// The patches that empty a comment header spanning pages, if the file is
/// Ogg Opus with one.
fn empty_comment(file: &mut impl ReadSeek) -> io::Result<Vec<(u64, Vec<u8>)>> {
    let head = Page::read(file, 0)?;
    let mut magic = [0; 8];
    file.read_exact(&mut magic)?;
    let at = head.len();
    let first = Page::read(file, at)?;
    if &magic != b"OpusHead" || !first.continues() {
        return Ok(Vec::new());
    }
    // The comment header ends a page, and audio starts on the next one.
    let mut end = at + first.len();
    let last = loop {
        let page = Page::read(file, end)?;
        end += page.len();
        if !page.continues() {
            break page;
        }
    };
    // The first page holds one packet: "OpusTags", no vendor, no comments.
    // It takes the last one's number, so the audio pages follow on from it.
    let mut page = first.header.to_vec();
    page[18..22].copy_from_slice(&last.header[18..22]);
    page[22..26].fill(0);
    page[26] = 1;
    page.push(16);
    page.extend_from_slice(b"OpusTags\0\0\0\0\0\0\0\0");
    let mut crc = Crc32::new(0);
    crc.process_buf_bytes(&page);
    page[22..26].copy_from_slice(&crc.crc().to_le_bytes());
    let mut from = at + page.len() as u64;
    let mut patches = vec![(at, page)];
    // Up to there, nothing may look like a page: the reader would sync to it
    // and fail. That is every page start and any "OggS" in the comments.
    file.seek(SeekFrom::Start(from))?;
    let mut read = vec![0; 1 << 16];
    let mut chunk = Vec::new();
    while from < end {
        let n = file.read(&mut read[..(end - from).min(1 << 16) as usize])?;
        if n == 0 {
            break;
        }
        let base = from - chunk.len() as u64;
        chunk.extend_from_slice(&read[..n]);
        for (i, window) in chunk.windows(4).enumerate() {
            if window == b"OggS" {
                patches.push((base + i as u64, b"Skip".to_vec()));
            }
        }
        from += n as u64;
        // The last 3 bytes may start one that ends in the next read.
        chunk.drain(..chunk.len().saturating_sub(3));
    }
    Ok(patches)
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A file read with some bytes replaced.
struct Patched<R> {
    file: R,
    pos: u64,
    len: u64,
    /// Offsets and the bytes that replace those there.
    patches: Vec<(u64, Vec<u8>)>,
}

impl<R: Read> Read for Patched<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        let end = self.pos + n as u64;
        for (at, bytes) in &self.patches {
            let from = (*at).max(self.pos);
            let to = (at + bytes.len() as u64).min(end);
            if from < to {
                buf[(from - self.pos) as usize..(to - self.pos) as usize]
                    .copy_from_slice(&bytes[(from - at) as usize..(to - at) as usize]);
            }
        }
        self.pos = end;
        Ok(n)
    }
}

impl<R: Seek> Seek for Patched<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = self.file.seek(to)?;
        Ok(self.pos)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for Patched<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use crate::audio::decoder::{self, OPUS};
    use std::path::Path;
    use std::time::Duration;

    /// A second of stereo Opus whose comment header holds a 17 MiB cover and
    /// text that looks like page starts.
    fn big_cover_opus(dir: &Path) -> std::path::PathBuf {
        use lofty::{
            picture::{MimeType, Picture},
            prelude::TaggedFileExt as _,
            tag::TagExt as _,
        };
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("big.opus");
        let ffmpeg = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", "sine=duration=1"])
            .args(["-ac", "2", "-c:a", "libopus"])
            .arg(&path)
            .status()
            .expect("ffmpeg is needed for this test");
        assert!(ffmpeg.success());
        let mut file = lofty::read_from_path(&path).unwrap();
        let tag = file.primary_tag_mut().unwrap();
        // Real covers are base64 text, where "OggS" turns up by chance.
        tag.insert_text(lofty::tag::ItemKey::Comment, "OggS\0".repeat(20_000));
        tag.push_picture(
            Picture::unchecked(vec![0; 17 << 20])
                .mime_type(MimeType::Png)
                .build(),
        );
        tag.save_to_path(&path, Default::default()).unwrap();
        path
    }

    #[test]
    fn opus_with_a_cover_over_16_mib_plays_and_seeks() {
        let dir = std::env::temp_dir().join(format!("motif-ogg-{}", std::process::id()));
        let path = big_cover_opus(&dir);
        let decoded = (|| {
            let mut decoder = decoder::open(&path.clone().into(), &[&OPUS])?;
            let mut samples = Vec::new();
            while decoder.decode(&mut samples)? {}
            let reached = decoder.seek(Duration::from_millis(500))?;
            let mut after = Vec::new();
            while decoder.decode(&mut after)? {}
            Ok::<_, String>((samples.len() / 2, reached, after.len() / 2))
        })();
        std::fs::remove_dir_all(&dir).unwrap();
        let (frames, reached, after) = decoded.unwrap();
        assert_eq!(frames, 48_000, "the whole second, pre-skip trimmed");
        assert!(reached.abs_diff(Duration::from_millis(500)) < Duration::from_millis(30));
        assert!(
            after.abs_diff(24_000) < 1_500,
            "{after} frames after the seek"
        );
    }
}
