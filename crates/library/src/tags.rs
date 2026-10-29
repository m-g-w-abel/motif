//! Reading what the scanner stores about a file. Files are only ever opened
//! for reading.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use lofty::{
    file::{FileType, TaggedFileExt},
    picture::PictureType,
    prelude::{Accessor, AudioFile, ItemKey},
};

/// File extensions the scanner looks at.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "ogg", "oga", "opus", "m4a", "m4b", "mp4", "aac", "alac", "wav", "aif", "aiff",
    "wv", "ape", "dsf", "dff", "wma", "tta", "mpc", "caf",
];

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[derive(Debug, Default)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub bitrate: Option<u32>,
    pub musicbrainz_recording: Option<String>,
    pub isrc: Option<String>,
    /// Embedded front cover (or first picture), with a file extension.
    pub picture: Option<(Vec<u8>, &'static str)>,
}

/// Tags and audio properties. Files lofty can't parse still get a row
/// (named after the file) so they stay playable.
pub fn read(path: &Path) -> Tags {
    let tagged = match lofty::read_from_path(path) {
        Ok(tagged) => tagged,
        Err(err) => {
            tracing::debug!(path = %path.display(), %err, "lofty can't read it, trying symphonia");
            return read_with_symphonia(path).unwrap_or_default();
        }
    };
    let properties = tagged.properties();
    let mut tags = Tags {
        duration_ms: Some(properties.duration().as_millis() as u64).filter(|&ms| ms > 0),
        codec: Some(codec_name(tagged.file_type(), path)),
        sample_rate: properties.sample_rate(),
        bit_depth: properties.bit_depth().map(u32::from),
        channels: properties.channels().map(u32::from),
        bitrate: properties.audio_bitrate(),
        ..Tags::default()
    };
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return tags;
    };
    let text = |s: Option<std::borrow::Cow<'_, str>>| {
        s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let item = |key: ItemKey| {
        tag.get_string(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    tags.title = text(tag.title());
    tags.artist = text(tag.artist());
    tags.album = text(tag.album());
    tags.genre = text(tag.genre());
    tags.album_artist = item(ItemKey::AlbumArtist);
    tags.track_no = tag.track();
    tags.disc_no = tag.disk();
    tags.year = tag.date().map(|d| u32::from(d.year)).filter(|&y| y > 0);
    tags.musicbrainz_recording = item(ItemKey::MusicBrainzRecordingId);
    tags.isrc = item(ItemKey::Isrc);
    let pictures = tag.pictures();
    tags.picture = pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or(pictures.first())
        .map(|p| (p.data().to_vec(), image_ext(p.data())));
    tags
}

fn image_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else {
        "jpg"
    }
}

/// Every picture embedded in a file, in tag order, with a file extension.
pub fn pictures(path: &Path) -> Vec<(Vec<u8>, &'static str)> {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return Vec::new();
    };
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    tag.map(|t| t.pictures().iter())
        .into_iter()
        .flatten()
        .map(|p| (p.data().to_vec(), image_ext(p.data())))
        .collect()
}

/// Pixel area of an image, read from its header without decoding it.
pub fn pixels(bytes: &[u8]) -> Option<u64> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    Some(u64::from(w) * u64::from(h))
}

/// Symphonia's container readers, for files lofty can't parse (some MP4s).
fn read_with_symphonia(path: &Path) -> Option<Tags> {
    use symphonia::core::{
        codecs::CodecParameters,
        formats::{TrackType, probe::Hint},
        io::MediaSourceStream,
        meta::StandardTag as T,
        units::Timestamp,
    };
    let stream = MediaSourceStream::new(Box::new(fs::File::open(path).ok()?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut reader = symphonia::default::get_probe()
        .probe(&hint, stream, Default::default(), Default::default())
        .ok()?;
    let mut tags = Tags {
        codec: path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_uppercase),
        ..Tags::default()
    };
    if let Some(track) = reader.default_track(TrackType::Audio) {
        if let (Some(base), Some(duration)) = (track.time_base, track.duration) {
            let time = base.calc_time(Timestamp::new(duration.get() as i64));
            tags.duration_ms = time.map(|t| (t.as_secs_f64() * 1000.0) as u64);
        }
        if let Some(CodecParameters::Audio(params)) = &track.codec_params {
            tags.sample_rate = params.sample_rate;
            tags.bit_depth = params.bits_per_sample;
            tags.channels = params.channels.as_ref().map(|c| c.count() as u32);
        }
    }
    let mut metadata = reader.metadata();
    let revision = metadata.skip_to_latest()?;
    for tag in &revision.media.tags {
        let text =
            |s: &std::sync::Arc<String>| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        match &tag.std {
            Some(T::TrackTitle(s)) => tags.title = text(s),
            Some(T::Artist(s)) => tags.artist = text(s),
            Some(T::Album(s)) => tags.album = text(s),
            Some(T::AlbumArtist(s)) => tags.album_artist = text(s),
            Some(T::Genre(s)) => tags.genre = text(s),
            Some(T::IdentIsrc(s)) => tags.isrc = text(s),
            Some(T::MusicBrainzRecordingId(s)) => tags.musicbrainz_recording = text(s),
            Some(T::TrackNumber(n)) => tags.track_no = u32::try_from(*n).ok(),
            Some(T::DiscNumber(n)) => tags.disc_no = u32::try_from(*n).ok(),
            Some(T::ReleaseDate(s) | T::RecordingDate(s)) if tags.year.is_none() => {
                tags.year = s.get(..4).and_then(|y| y.parse().ok());
            }
            _ => {}
        }
    }
    tags.picture = revision.media.visuals.first().map(|v| {
        let png = v.media_type.as_deref() == Some("image/png");
        (v.data.to_vec(), if png { "png" } else { "jpg" })
    });
    Some(tags)
}

fn codec_name(file_type: FileType, path: &Path) -> String {
    match file_type {
        FileType::Flac => "FLAC".into(),
        FileType::Mpeg => "MP3".into(),
        FileType::Opus => "Opus".into(),
        FileType::Vorbis => "Vorbis".into(),
        FileType::Wav => "WAV".into(),
        FileType::Aiff => "AIFF".into(),
        FileType::WavPack => "WavPack".into(),
        FileType::Ape => "APE".into(),
        FileType::Aac => "AAC".into(),
        FileType::Mpc => "MPC".into(),
        // MP4 holds AAC or ALAC; the extension is the best cheap hint.
        _ => path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_uppercase(),
    }
}

/// Finds `cover.jpg`, `folder.png` and the like next to a file, with the
/// image's artwork key and pixel area, remembering the answer per folder.
#[derive(Default)]
pub struct FolderArt {
    seen: HashMap<PathBuf, Option<FolderImage>>,
}

#[derive(Clone)]
pub struct FolderImage {
    pub path: PathBuf,
    pub key: String,
    pub pixels: Option<u64>,
}

const ART_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];

impl FolderArt {
    pub fn find(&mut self, file: &Path) -> Option<FolderImage> {
        let dir = file.parent()?;
        self.seen
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let mut found: Vec<PathBuf> = fs::read_dir(dir)
                    .ok()?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        let stem = p
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        ART_NAMES.contains(&stem.as_str()) && is_image(p)
                    })
                    .collect();
                // Prefer names earlier in the list (cover over folder…).
                found.sort_by_key(|p| {
                    let stem = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    ART_NAMES.iter().position(|n| *n == stem)
                });
                let path = found.into_iter().next()?;
                let bytes = fs::read(&path).ok()?;
                Some(FolderImage {
                    key: crate::scan::art_key(&bytes),
                    pixels: pixels(&bytes),
                    path,
                })
            })
            .clone()
    }
}

/// Image files the artwork code can read.
pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png"))
}

/// The FLAC stream's own MD5 of its decoded audio, if the encoder set it.
pub fn flac_md5(path: &Path) -> Option<u128> {
    let mut file = fs::File::open(path).ok()?;
    let flac = lofty::flac::FlacFile::read_from(
        &mut file,
        lofty::config::ParseOptions::new().read_tags(false),
    )
    .ok()?;
    Some(flac.properties().signature()).filter(|&md5| md5 != 0)
}
