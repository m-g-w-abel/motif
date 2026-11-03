//! Lyrics (#89): from the file's tags, a sidecar `.lrc` or `.txt`, else
//! LRCLIB. LRCLIB answers, misses too, are cached in `lyrics` by audio hash.

use std::{fs, path::Path, sync::OnceLock, time::Duration};

use lofty::{
    config::ParseOptions,
    file::{AudioFile, TaggedFileExt},
    id3::v2::{Frame, SyncTextContentType, SynchronizedTextFrame, TimestampFormat},
    mpeg::MpegFile,
    prelude::ItemKey,
    probe::Probe,
};
use protocol::LyricsLine;

/// The public LRCLIB server.
pub const LRCLIB: &str = "https://lrclib.net";

/// LRCLIB is asked again this long after a miss, in case lyrics were added.
pub const RETRY_SECS: i64 = 7 * 24 * 3600;

/// LRCLIB isn't asked for this long after an error, so a widget asking on
/// every track change doesn't wait on a server that is down or refusing.
pub const BACKOFF: Duration = Duration::from_secs(60);

/// Parses LRC: `[mm:ss.xx]` stamps (several per line) and `[offset:ms]`;
/// other tags and lines without a stamp are dropped, and word stamps
/// (`<mm:ss.xx>`) are ignored. Text without a single stamp is plain lyrics.
pub fn parse(text: &str) -> Vec<LyricsLine> {
    let text = text.trim_start_matches('\u{feff}');
    let mut offset = 0i64;
    let mut synced = Vec::new();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut stamps = Vec::new();
        while let Some((tag, after)) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            match (stamp(tag), tag.strip_prefix("offset:")) {
                (Some(ms), _) => stamps.push(ms),
                (None, Some(value)) => offset = value.trim().parse().unwrap_or(0),
                _ => {}
            }
            rest = after.trim_start();
        }
        let text = without_word_stamps(rest);
        synced.extend(stamps.into_iter().map(|ms| (ms, text.clone())));
    }
    if synced.is_empty() {
        let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
        let start = lines
            .iter()
            .position(|l| !l.is_empty())
            .unwrap_or(lines.len());
        let end = lines
            .iter()
            .rposition(|l| !l.is_empty())
            .map_or(start, |i| i + 1);
        return lines[start..end]
            .iter()
            .map(|l| LyricsLine {
                time_ms: None,
                text: l.to_string(),
            })
            .collect();
    }
    // A positive offset shows lines earlier.
    synced.sort_by_key(|(ms, _)| *ms);
    synced
        .into_iter()
        .map(|(ms, text)| LyricsLine {
            time_ms: Some((ms as i64 - offset).max(0) as u64),
            text,
        })
        .collect()
}

/// `mm:ss`, `mm:ss.x` to `mm:ss.xxx` (or `:` before the fraction) in ms.
fn stamp(tag: &str) -> Option<u64> {
    let (min, sec) = tag.split_once(':')?;
    let (sec, frac) = sec.split_once(['.', ':']).unwrap_or((sec, ""));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(min) || !digits(sec) || !(frac.is_empty() || digits(frac)) {
        return None;
    }
    let frac = &frac[..frac.len().min(3)];
    let frac_ms = match frac.len() {
        0 => 0,
        n => frac.parse::<u64>().ok()? * 10u64.pow(3 - n as u32),
    };
    Some(min.parse::<u64>().ok()? * 60_000 + sec.parse::<u64>().ok()? * 1000 + frac_ms)
}

fn without_word_stamps(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        match rest[start + 1..].find('>') {
            Some(len) if stamp(&rest[start + 1..start + 1 + len]).is_some() => {
                out.push_str(&rest[..start]);
                rest = &rest[start + 2 + len..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Lines as LRC, each time moved by `offset_ms` (positive is later).
pub fn to_lrc(lines: &[LyricsLine], offset_ms: i32) -> String {
    let mut out = String::new();
    for line in lines {
        if let Some(ms) = line.time_ms {
            let ms = (ms as i64 + i64::from(offset_ms)).max(0);
            let (min, sec, cs) = (ms / 60_000, ms / 1000 % 60, ms / 10 % 100);
            out.push_str(&format!("[{min:02}:{sec:02}.{cs:02}]"));
        }
        out.push_str(&line.text);
        out.push('\n');
    }
    out
}

pub fn synced(lines: &[LyricsLine]) -> bool {
    lines.first().is_some_and(|l| l.time_ms.is_some())
}

/// The first synced candidate, else the first plain one: synced lyrics win
/// whatever their source, so `candidates` come in order of preference.
pub fn best<T>(
    candidates: impl IntoIterator<Item = (T, Vec<LyricsLine>)>,
) -> Option<(T, Vec<LyricsLine>)> {
    let mut plain = None;
    for (source, lines) in candidates {
        if lines.iter().all(|l| l.text.trim().is_empty()) {
            continue;
        }
        if synced(&lines) {
            return Some((source, lines));
        }
        plain.get_or_insert((source, lines));
    }
    plain
}

/// Lyrics in the file's tags: ID3 SYLT frames (MP3 only), then lyrics text
/// items (USLT, Vorbis `LYRICS`/`UNSYNCEDLYRICS`, MP4 `©lyr`), parsed as LRC.
pub fn embedded(path: &Path) -> Vec<Vec<LyricsLine>> {
    let options = ParseOptions::new()
        .read_properties(false)
        .read_cover_art(false);
    let mut found = Vec::new();
    // shortcut: SYLT is only read from MP3s, add other ID3 carriers (AIFF, WAV) if asked for.
    let mp3 = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    if mp3
        && let Ok(mut file) = fs::File::open(path)
        && let Ok(mpeg) = MpegFile::read_from(&mut file, options)
        && let Some(id3) = mpeg.id3v2()
    {
        for frame in id3 {
            if let Frame::Binary(binary) = frame
                && frame.id_str() == "SYLT"
                && let Ok(sylt) = SynchronizedTextFrame::parse(&binary.data, frame.flags())
                && sylt.timestamp_format == TimestampFormat::MS
                && sylt.content_type == SyncTextContentType::Lyrics
            {
                let mut lines: Vec<LyricsLine> = sylt
                    .content
                    .into_iter()
                    .map(|(ms, text)| LyricsLine {
                        time_ms: Some(u64::from(ms)),
                        text: text.trim().to_string(),
                    })
                    .collect();
                lines.sort_by_key(|l| l.time_ms);
                found.push(lines);
            }
        }
    }
    let tagged = Probe::open(path).ok().map(|p| p.options(options));
    let Some(tagged) = tagged.and_then(|p| p.guess_file_type().ok()?.read().ok()) else {
        return found;
    };
    for tag in tagged.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            found.extend(tag.get_strings(key).map(parse));
        }
    }
    found
}

/// The sidecar `.lrc`, then `.txt`, next to the track.
pub fn sidecars(path: &Path) -> Vec<Vec<LyricsLine>> {
    ["lrc", "txt"]
        .iter()
        .filter_map(|ext| fs::read(path.with_extension(ext)).ok())
        .map(|bytes| parse(&String::from_utf8_lossy(&bytes)))
        .collect()
}

/// LRCLIB's synced (LRC) and plain lyrics, as cached.
pub type Fetched = (Option<String>, Option<String>);

/// LRCLIB's synced and plain lyrics for a recording, `None` if it has none.
/// Errors (offline, server trouble) are not misses, so they aren't cached.
pub fn fetch(
    base: &str,
    artist: &str,
    title: &str,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<Option<Fetched>, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            // Fails harmlessly once a provider is installed.
            let _ = rustls::crypto::ring::default_provider().install_default();
            reqwest::blocking::Client::builder()
                .user_agent(concat!("Motif/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()?;
    let mut params = vec![
        ("artist_name", artist.to_string()),
        ("track_name", title.to_string()),
    ];
    if let Some(album) = album {
        params.push(("album_name", album.to_string()));
    }
    if let Some(ms) = duration_ms {
        params.push(("duration", ((ms + 500) / 1000).to_string()));
    }
    let url = reqwest::Url::parse_with_params(&format!("{base}/api/get"), &params)
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let found: serde_json::Value = response
        .error_for_status()
        .and_then(|r| r.json())
        .map_err(|e| e.to_string())?;
    let text = |key: &str| {
        found[key]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    };
    let (synced, plain) = (text("syncedLyrics"), text("plainLyrics"));
    Ok((synced.is_some() || plain.is_some()).then_some((synced, plain)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ms: u64, text: &str) -> LyricsLine {
        LyricsLine {
            time_ms: Some(ms),
            text: text.into(),
        }
    }

    #[test]
    fn lrc_parses_stamps_offsets_and_skips_junk() {
        let lrc = "\u{feff}[ar:Someone]\n[ti:Song]\n[offset:+500]\n\
                   [00:12.50]First\nnot a lyric line\n[00:10.00][01:02.3]Chorus <00:10.20>word <x>\n\
                   [aa:bb]Broken\n\n[00:05]  Early  ";
        assert_eq!(
            parse(lrc),
            vec![
                line(4500, "Early"),
                line(9500, "Chorus word <x>"),
                line(12000, "First"),
                line(61800, "Chorus word <x>"),
            ]
        );
        // A negative offset shows lines later; none ends before the start.
        assert_eq!(parse("[offset:-250]\n[00:00.10]A"), vec![line(350, "A")]);
        assert_eq!(parse("[offset:1000]\n[00:00.10]A"), vec![line(0, "A")]);
        assert_eq!(
            parse("[00:01.5]A\n[00:01:25]B"),
            vec![line(1250, "B"), line(1500, "A")]
        );
    }

    #[test]
    fn text_without_stamps_is_plain() {
        let plain = parse("\n[Chorus]\nLa la\n\nLa\n\n");
        assert!(!synced(&plain));
        let texts: Vec<_> = plain.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["[Chorus]", "La la", "", "La"]);
        assert!(parse("  \n").is_empty());
    }

    #[test]
    fn synced_beats_plain_and_blank_counts_as_none() {
        let plain = parse("Words");
        let synced = vec![line(0, "Sung")];
        assert_eq!(
            best([(1, vec![]), (2, plain.clone()), (3, synced.clone())]),
            Some((3, synced.clone()))
        );
        assert_eq!(
            best([(1, parse(" \n ")), (2, plain.clone())]),
            Some((2, plain))
        );
        assert_eq!(best::<u8>([]), None);
    }

    #[test]
    fn saved_lrc_reads_back_with_the_offset_baked_in() {
        let lines = vec![line(1230, "One"), line(65_000, "Two")];
        assert_eq!(parse(&to_lrc(&lines, 0)), lines);
        assert_eq!(to_lrc(&lines, -2000), "[00:00.00]One\n[01:03.00]Two\n");
        let plain = parse("Just words");
        assert_eq!(to_lrc(&plain, 500), "Just words\n");
    }
}
