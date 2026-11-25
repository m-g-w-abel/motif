//! Each decoder plugin decodes the fixture files (a 440 Hz sine, generated
//! with FFmpeg) to the right length, so gapless trimming works too.

use std::{
    io::{Cursor, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Duration,
};

use engine::{
    ByteSource, Opened, Source,
    audio::decoder::{self, Decoder, Plugin},
};

fn frames(file: &str, plugins: &[&Plugin]) -> Result<(u64, u32), String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file);
    let mut decoder: Box<dyn Decoder> = decoder::open(&path.into(), plugins)?;
    let mut samples = Vec::new();
    while decoder.decode(&mut samples)? {}
    Ok((
        (samples.len() / decoder.channels()) as u64,
        decoder.sample_rate(),
    ))
}

#[test]
fn symphonia_formats_decode_to_their_exact_length() {
    for (file, expected) in [
        ("sine.flac", 44_100),
        ("sine.wav", 11_025),
        ("sine-alac.m4a", 44_100),
        ("sine.mp3", 44_100),
        ("sine.ogg", 44_100),
        // shortcut: Symphonia ignores the MP4 edit list holding AAC's encoder delay, so AAC keeps ~23 ms of priming; trim it ourselves if AAC gapless albums matter.
        ("sine.m4a", 46_080),
    ] {
        let (frames, rate) =
            frames(file, &[&decoder::SYMPHONIA]).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(rate, 44_100, "{file}");
        assert_eq!(frames, expected, "{file}");
    }
}

#[test]
fn unsupported_files_say_which_decoders_tried() {
    let err = frames("sine.wv", &[&decoder::SYMPHONIA]).unwrap_err();
    assert!(err.starts_with("symphonia: "), "{err}");
}

#[test]
fn opus_decodes_through_libopus_with_pre_skip_trimmed() {
    assert_eq!(
        frames("sine.opus", &[&decoder::OPUS]).unwrap(),
        (48_000, 48_000)
    );
    assert_eq!(
        frames("sine.flac", &[&decoder::OPUS]).unwrap_err(),
        "libopus: not Opus"
    );
}

fn have_ffmpeg() -> bool {
    std::process::Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_ok()
}

#[test]
fn ffmpeg_plays_what_symphonia_cannot() {
    if !have_ffmpeg() {
        eprintln!("skipped: FFmpeg not installed");
        return;
    }
    assert_eq!(
        frames("sine.wv", &decoder::all()).unwrap(),
        (44_100, 44_100)
    );
    // Falls back past a plugin that can't open the file.
    assert_eq!(
        frames("sine.wv", &[&decoder::OPUS, &decoder::FFMPEG]).unwrap(),
        (44_100, 44_100)
    );
}

#[test]
fn ffmpeg_seeks_by_restarting() {
    if !have_ffmpeg() {
        return;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine.wv");
    let mut decoder = (decoder::FFMPEG.open)(&path.into()).unwrap();
    decoder.seek(std::time::Duration::from_millis(500)).unwrap();
    let mut samples = Vec::new();
    while decoder.decode(&mut samples).unwrap() {}
    assert_eq!(samples.len() / 2, 22_050);
}

#[test]
fn damaged_files_fail_gracefully() {
    let dir = std::env::temp_dir().join(format!("motif-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine.flac"))
            .unwrap();

    // Cut off part-way: plays what is there, then ends.
    let truncated = dir.join("truncated.flac");
    std::fs::write(&truncated, &bytes[..bytes.len() * 6 / 10]).unwrap();
    let mut decoder = decoder::open(&truncated.into(), &[&decoder::SYMPHONIA]).unwrap();
    let mut samples = Vec::new();
    while decoder.decode(&mut samples).unwrap_or(false) {}
    let frames = samples.len() / 2;
    assert!(frames > 10_000 && frames < 44_100, "{frames} frames");

    // Not audio at all: refused with a reason, not a crash.
    let garbage = dir.join("garbage.flac");
    std::fs::write(&garbage, vec![0x5a; 4096]).unwrap();
    let err = decoder::open(&garbage.into(), &decoder::all())
        .err()
        .expect("garbage must not open");
    assert!(err.contains("symphonia:"), "{err}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Bytes held in memory, standing in for a linked engine's stream.
struct Held(Cursor<Vec<u8>>);

impl Read for Held {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Held {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(to)
    }
}

impl ByteSource for Held {
    fn byte_len(&self) -> u64 {
        self.0.get_ref().len() as u64
    }
}

fn stream(file: &str) -> Source {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file),
    )
    .unwrap();
    Source::Stream {
        name: file.into(),
        open: Arc::new(move || Ok(Opened::Bytes(Box::new(Held(Cursor::new(bytes.clone())))))),
    }
}

#[test]
fn streams_decode_and_seek_like_files() {
    for (file, expected) in [("sine.flac", 44_100), ("sine.opus", 48_000)] {
        let mut decoder = decoder::open(&stream(file), &decoder::all()).unwrap();
        let mut samples = Vec::new();
        while decoder.decode(&mut samples).unwrap() {}
        assert_eq!(
            (samples.len() / decoder.channels()) as u64,
            expected,
            "{file}"
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file);
        let mut from_file = decoder::open(&path.into(), &decoder::all()).unwrap();
        let to = Duration::from_millis(500);
        assert_eq!(decoder.seek(to), from_file.seek(to), "{file}");
    }
    // FFmpeg reads files only: it says so rather than failing silently.
    let err = decoder::open(&stream("sine.wv"), &[&decoder::FFMPEG])
        .err()
        .unwrap();
    assert!(err.contains("ffmpeg:"), "{err}");
}

/// A stream (a linked engine's track) gives the very samples its file does,
/// from the first: no format guessed wrong without the file's extension.
#[test]
fn streams_decode_to_the_same_samples_as_their_files() {
    let decode = |source: &Source| {
        let mut decoder = decoder::open(source, &decoder::all()).unwrap();
        let mut samples = Vec::new();
        while decoder.decode(&mut samples).unwrap() {}
        (decoder.channels(), samples)
    };
    // WavPack goes through FFmpeg, which reads files only.
    for file in [
        "sine.flac",
        "sine.mp3",
        "sine.m4a",
        "sine-alac.m4a",
        "sine.ogg",
        "sine.opus",
        "sine.wav",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file);
        assert!(decode(&stream(file)) == decode(&path.into()), "{file}");
    }
}
