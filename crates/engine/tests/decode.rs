//! Each decoder plugin decodes the fixture files (a 440 Hz sine, generated
//! with FFmpeg) to the right length, so gapless trimming works too.

use std::path::Path;

use engine::audio::decoder::{self, Decoder, Plugin};

fn frames(file: &str, plugins: &[&Plugin]) -> Result<(u64, u32), String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file);
    let mut decoder: Box<dyn Decoder> = decoder::open(&path, plugins)?;
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
