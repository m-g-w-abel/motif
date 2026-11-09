//! Scans the fixture folder (two small albums) and checks what the library
//! answers.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use library::Library;
use protocol::{Command, ContextKind, LibraryQuery, QueryResult, SortKey, SortRule, TrackRef};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("motif-test-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn wait_until(library: &Library, done: impl Fn(&protocol::LibraryStatus) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(&library.status()) {
        assert!(
            Instant::now() < deadline,
            "timed out: {:?}",
            library.status()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn track(library: &Library, title: &str) -> protocol::LibraryTrack {
    let query = LibraryQuery::Tracks {
        search: None,
        sort: vec![],
        offset: 0,
        limit: 100,
    };
    let QueryResult::Tracks { tracks, .. } = library.query(query).unwrap() else {
        panic!()
    };
    tracks
        .into_iter()
        .find(|t| {
            t.track.title.as_deref() == Some(title) || t.track.id.ends_with(&format!("{title}.mp3"))
        })
        .unwrap_or_else(|| panic!("no track {title}"))
}

fn titles(library: &Library, search: Option<&str>) -> Vec<String> {
    let query = LibraryQuery::Tracks {
        search: search.map(Into::into),
        sort: vec![],
        offset: 0,
        limit: 100,
    };
    let QueryResult::Tracks { tracks, .. } = library.query(query).unwrap() else {
        panic!()
    };
    tracks.into_iter().map(|t| t.track.title.unwrap()).collect()
}

#[test]
fn scans_queries_matches_and_rescans() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning && s.hashing == 0);

    assert_eq!(
        titles(&library, None),
        ["Opening", "Closing", "Beginning", "Ending"]
    );
    assert_eq!(titles(&library, Some("clos")), ["Closing"]);

    let QueryResult::Groups { groups } = library
        .query(LibraryQuery::Groups {
            search: None,
            sort: vec![],
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        groups
            .iter()
            .map(|g| (g.start, g.count))
            .collect::<Vec<_>>(),
        [(0, 2), (2, 2)]
    );

    let query = LibraryQuery::Albums {
        search: None,
        sort: vec![],
        offset: 0,
        limit: 10,
    };
    let QueryResult::Albums { total, albums, .. } = library.query(query).unwrap() else {
        panic!()
    };
    assert_eq!(total, 2);
    assert_eq!(albums[1].title, "Second Album");
    let art = albums[1]
        .art
        .clone()
        .expect("folder cover.png is the album art");
    // Made in the background before hashing, so ready by now.
    let bar_thumb = data.join(format!("art/{art}-{}.jpg", library::BAR_ART_SIZE));
    assert!(bar_thumb.is_file(), "the scan pre-caches the bar size");
    assert!(library.art(&art, Some(32)).unwrap().is_file());
    let cover = music.join("Artist B/Second Album/cover.png");
    assert_eq!(
        library.art(&art, Some(128)).unwrap(),
        cover,
        "never upscaled"
    );
    assert_eq!(albums[0].art, None);

    // Other images in the folder are listed on demand, the shown art first.
    // Not a copy of the cover (identical images list once): one byte more.
    let other = [fs::read(&cover).unwrap(), vec![0]].concat();
    fs::write(cover.with_file_name("back.png"), &other).unwrap();
    let beginning = cover.with_file_name("01 Beginning.mp3");
    let images = library.images(beginning.to_str().unwrap()).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].key, art);
    assert_eq!(images[0].kind, Some(protocol::ImageKind::Front));
    assert_eq!(images[1].kind, Some(protocol::ImageKind::Back));
    let back = library.art(&images[1].key, None).unwrap();
    assert_eq!(back, cover.with_file_name("back.png"));

    // Listing and serving them never waits on the scanner writing (#18).
    let writer = rusqlite::Connection::open(data.join("library.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    fs::remove_file(cover.with_file_name("back.png")).unwrap();
    fs::write(cover.with_file_name("booklet.png"), &other).unwrap();
    let started = Instant::now();
    let images = library.images(beginning.to_str().unwrap()).unwrap();
    assert_eq!(images[1].kind, Some(protocol::ImageKind::Booklet));
    let booklet = library.art(&images[1].key, None).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "waited for the lock"
    );
    assert_eq!(booklet, cover.with_file_name("booklet.png"));
    writer.execute_batch("ROLLBACK").unwrap();

    let query = LibraryQuery::AlbumPosition {
        search: None,
        sort: vec![],
        album: albums[1].key.clone(),
    };
    assert_eq!(
        library.query(query).unwrap(),
        QueryResult::Position { index: Some(2) }
    );

    // A reference from another device: matched by ISRC, or by artist + title.
    let elsewhere = |isrc: Option<&str>, title: &str, artist: &str| TrackRef {
        source: "local".into(),
        id: "/some/other/device/file.flac".into(),
        keys: protocol::TrackKeys {
            isrc: isrc.map(Into::into),
            ..Default::default()
        },
        title: Some(title.into()),
        artist: Some(artist.into()),
        ..Default::default()
    };
    let found = library
        .find(&elsewhere(Some("USABC1234567"), "?", "?"))
        .unwrap();
    assert!(found.ends_with("01 Opening.flac"), "{found:?}");
    let found = library
        .find(&elsewhere(None, "ending", "ARTIST B"))
        .unwrap();
    assert!(found.ends_with("02 Ending.ogg"), "{found:?}");
    assert_eq!(library.find(&elsewhere(None, "Missing", "Nobody")), None);

    let Command::SetContext { tracks: refs, .. } = library
        .selection(&LibraryQuery::Play {
            search: None,
            sort: vec![],
            start: 0,
        })
        .unwrap()
    else {
        panic!()
    };
    let hashes: Vec<_> = refs
        .iter()
        .map(|r| r.keys.audio_hash.clone().unwrap())
        .collect();
    assert!(
        hashes[0].starts_with("flac-md5:") && hashes[2].starts_with("pcm-b3:"),
        "{hashes:?}"
    );

    // Other rules reorder every list the same way.
    let by_title = vec![SortRule {
        key: SortKey::Title,
        descending: false,
    }];
    let QueryResult::Tracks { tracks, .. } = library
        .query(LibraryQuery::Tracks {
            search: None,
            sort: by_title.clone(),
            offset: 1,
            limit: 2,
        })
        .unwrap()
    else {
        panic!()
    };
    let names: Vec<_> = tracks
        .iter()
        .map(|t| t.track.title.clone().unwrap())
        .collect();
    assert_eq!(names, ["Closing", "Ending"]);
    let QueryResult::Groups { groups } = library
        .query(LibraryQuery::Groups {
            search: None,
            sort: by_title.clone(),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(groups.len(), 4, "albums alternate by title");
    assert_eq!(
        library
            .query(LibraryQuery::AlbumPosition {
                search: None,
                sort: by_title.clone(),
                album: albums[1].key.clone(),
            })
            .unwrap(),
        QueryResult::Position { index: Some(0) }
    );
    let Command::SetContext { tracks: refs, .. } = library
        .selection(&LibraryQuery::Play {
            search: None,
            sort: by_title.clone(),
            start: 0,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(refs[0].title.as_deref(), Some("Beginning"));
    let Command::SetContext {
        tracks: refs,
        kind,
        label,
        ..
    } = library
        .selection(&LibraryQuery::PlayAlbum {
            album: albums[0].key.clone(),
            sort: vec![SortRule {
                key: SortKey::Track,
                descending: true,
            }],
            start: None,
        })
        .unwrap()
    else {
        panic!()
    };
    let names: Vec<_> = refs.iter().map(|r| r.title.clone().unwrap()).collect();
    assert_eq!(names, ["Closing", "Opening"]);
    assert_eq!(kind, ContextKind::Album);
    assert_eq!(label, refs[0].album);
    // An album page plays from the row pressed.
    let Command::SetContext { start, .. } = library
        .selection(&LibraryQuery::PlayAlbum {
            album: albums[0].key.clone(),
            sort: vec![],
            start: Some(1),
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(start, Some(1));
    // Album tracks come album by album, in the order asked for.
    let QueryResult::Tracks { tracks, .. } = library
        .query(LibraryQuery::AlbumTracks {
            albums: vec![albums[1].key.clone(), albums[0].key.clone()],
            sort: vec![],
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(tracks[0].track.album, albums[1].title.clone().into());
    assert_eq!(tracks.last().unwrap().track.album, refs[0].album);
    let QueryResult::Albums { albums: desc, .. } = library
        .query(LibraryQuery::Albums {
            search: None,
            sort: vec![SortRule {
                key: SortKey::Album,
                descending: true,
            }],
            offset: 0,
            limit: 10,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(desc[0].title, "Second Album");

    // A file that disappears stays in the library, marked missing, and comes
    // back (same row) when it reappears.
    let closing = music.join("Artist A/First Album/02 Closing.flac");
    let kept = fs::read(&closing).unwrap();
    let id_before = track(&library, "Closing").id;
    fs::remove_file(&closing).unwrap();
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| s.missing == 1 && !s.scanning);
    assert_eq!(library.status().tracks, 4);
    assert!(track(&library, "Closing").missing);
    fs::write(&closing, kept).unwrap();
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| s.missing == 0 && !s.scanning);
    assert_eq!(track(&library, "Closing").id, id_before);

    // A damaged file is listed, with the reason it can't be played.
    fs::write(
        music.join("Artist A/First Album/03 Broken.mp3"),
        vec![0u8; 2048],
    )
    .unwrap();
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| s.tracks == 5 && !s.scanning && s.hashing == 0);
    assert!(track(&library, "03 Broken").problem.is_some());

    // A sibling folder whose name starts the same is a separate root.
    let sibling = PathBuf::from(format!("{}2", music.display()));
    fs::create_dir_all(&sibling).unwrap();
    fs::copy(
        music.join("Artist B/Second Album/01 Beginning.mp3"),
        sibling.join("Other.mp3"),
    )
    .unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: sibling.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 6 && !s.scanning);
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| !s.scanning);
    assert_eq!((library.status().tracks, library.status().missing), (6, 0));

    // An emptied folder (an unmounted drive's mount point) is reported, not
    // marked missing.
    fs::remove_file(sibling.join("Other.mp3")).unwrap();
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| !s.scanning && s.error.is_some());
    assert_eq!(library.status().missing, 0);
    assert!(library.status().error.unwrap().contains("not available"));

    library
        .query(LibraryQuery::SetWatching { enabled: false })
        .unwrap();
    assert!(!library.status().watching);

    // Removing a folder in Motif removes its tracks, and only its tracks.
    library
        .query(LibraryQuery::RemoveRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 1);
    let _ = fs::remove_dir_all(sibling);
    let _ = (fs::remove_dir_all(data), fs::remove_dir_all(music));
}

#[test]
fn held_background_work_waits_until_released() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let library = Library::open(&data).unwrap();
    library.hold_background(true);
    library.hold_background(true);
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    // Scans still run while held; only hashing and thumbnails wait.
    wait_until(&library, |s| s.tracks == 4 && !s.scanning);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(library.status().hashing, 4);

    library.hold_background(false);
    wait_until(&library, |s| s.hashing == 0);
}

#[test]
fn a_folder_copy_of_the_embedded_cover_lists_once() {
    use lofty::{
        config::WriteOptions,
        picture::{MimeType, Picture, PictureType},
        prelude::*,
    };
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cover = fs::read(fixtures.join("Artist B/Second Album/cover.png")).unwrap();
    let flac = music.join("01 Opening.flac");
    fs::copy(fixtures.join("Artist A/First Album/01 Opening.flac"), &flac).unwrap();
    let mut tagged = lofty::read_from_path(&flac).unwrap();
    let tag = tagged.primary_tag_mut().unwrap();
    tag.push_picture(
        Picture::unchecked(cover.clone())
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .build(),
    );
    tag.save_to_path(&flac, WriteOptions::default()).unwrap();
    fs::write(music.join("cover.png"), &cover).unwrap();

    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 1 && !s.scanning);
    let images = library.images(flac.to_str().unwrap()).unwrap();
    assert_eq!(images.len(), 1, "{images:?}");
    assert_eq!(
        Some(&images[0].key),
        track(&library, "Opening").art.as_ref()
    );
}

#[test]
fn waveforms_are_decoded_then_cached() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning);
    let waveform = |path: String| match library.query(LibraryQuery::Waveform { path }) {
        Ok(QueryResult::Waveform { levels }) => levels,
        other => panic!("{other:?}"),
    };
    let path = track(&library, "Opening").track.id;
    let levels = waveform(path.clone());
    assert_eq!(levels.len(), 1000);
    assert_eq!(levels.iter().max(), Some(&255));
    // Cached by the FLAC's hash (written on a thread of its own).
    let db = rusqlite::Connection::open(data.join("library.db")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while db
        .query_row("SELECT count(*) FROM waveforms", [], |r| r.get::<_, u32>(0))
        .unwrap()
        == 0
    {
        assert!(Instant::now() < deadline, "never cached");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(waveform(path), levels);
    assert!(
        library
            .query(LibraryQuery::Waveform {
                path: "/not/in/the/library.flac".into()
            })
            .is_err()
    );
}

/// Writes a WAV of `seconds` of a 1 kHz sine at `level` (full scale 1).
fn write_sine(path: &Path, seconds: u32, level: f32) {
    let (rate, frames) = (44_100u32, 44_100 * seconds);
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + frames * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    // 16-byte PCM header: mono, 16-bit.
    for v in [16u32, 1 | 1 << 16, rate, rate * 2, 2 | 16 << 16] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend(b"data");
    bytes.extend((frames * 2).to_le_bytes());
    for i in 0..frames {
        let s = level * (i as f32 * 1000.0 * std::f32::consts::TAU / rate as f32).sin();
        bytes.extend(((s * 32767.0) as i16).to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn loudness_is_measured_for_untagged_files_then_gives_album_gain() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    // One untagged album: a loud track and a quiet one.
    let (loud, quiet) = (music.join("loud.wav"), music.join("quiet.wav"));
    write_sine(&loud, 2, 0.5);
    write_sine(&quiet, 2, 0.05);
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 2 && !s.scanning);
    assert_eq!(library.replay_gain(&loud), Default::default(), "untagged");

    library.measure(vec![loud.clone()], true);
    assert!(library.take_measured());
    let gain = library.replay_gain(&loud);
    // A 0.5 sine is -6 dBFS RMS, -9 LUFS with K-weighting at 1 kHz: about -9 dB.
    let track_gain = gain.track_gain.unwrap();
    assert!((track_gain + 9.0).abs() < 1.0, "{gain:?}");
    assert!((gain.track_peak.unwrap() - 0.5).abs() < 0.01, "{gain:?}");
    assert_eq!(gain.album_gain, None, "the quiet track isn't measured yet");

    library.measure(vec![quiet.clone()], false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !library.take_measured() {
        assert!(Instant::now() < deadline, "never measured");
        std::thread::sleep(Duration::from_millis(20));
    }
    let gain = library.replay_gain(&loud);
    let album_gain = gain.album_gain.unwrap();
    assert!(
        album_gain > track_gain && album_gain < track_gain + 3.1,
        "{gain:?}"
    );
    assert_eq!(library.replay_gain(&quiet).album_gain, Some(album_gain));
    // Cached: nothing to measure again.
    library.measure(vec![loud], true);
    assert!(!library.take_measured());
}

#[test]
fn musicbrainz_release_ids_are_read() {
    use lofty::{config::WriteOptions, file::TaggedFileExt, prelude::ItemKey, tag::TagExt};
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let file = music.join("Artist A/First Album/01 Opening.flac");
    let mut tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    let id = "b84ee12a-09ef-421b-82de-0441a926375b";
    tag.insert_text(ItemKey::MusicBrainzReleaseId, id.into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning);
    let path = track(&library, "Opening").track.id;
    assert_eq!(library.musicbrainz_album(&path).as_deref(), Some(id));
    let other = track(&library, "Closing").track.id;
    assert_eq!(library.musicbrainz_album(&other), None);
}

#[test]
fn both_years_are_kept_and_original_sorts_first() {
    use lofty::{config::WriteOptions, file::TaggedFileExt, prelude::ItemKey, tag::TagExt};
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let file = music.join("Artist A/First Album/01 Opening.flac");
    let mut tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    tag.insert_text(ItemKey::RecordingDate, "2010-05-01".into());
    tag.insert_text(ItemKey::OriginalReleaseDate, "1975-03-02".into());
    tag.save_to_path(&file, WriteOptions::default()).unwrap();
    let other = music.join("Artist A/First Album/02 Closing.flac");
    let mut tag = lofty::read_from_path(&other)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    tag.insert_text(ItemKey::RecordingDate, "2010-05-01".into());
    tag.save_to_path(&other, WriteOptions::default()).unwrap();
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning);
    let opening = track(&library, "Opening");
    assert_eq!(
        (opening.year, opening.original_year),
        (Some(2010), Some(1975))
    );
    let closing = track(&library, "Closing");
    assert_eq!((closing.year, closing.original_year), (Some(2010), None));
    let QueryResult::Tracks { tracks, .. } = library
        .query(LibraryQuery::Tracks {
            search: None,
            sort: vec![protocol::SortRule {
                key: protocol::SortKey::Year,
                descending: true,
            }],
            offset: 0,
            limit: 100,
        })
        .unwrap()
    else {
        panic!()
    };
    // 2010 (release year only) before 1975 (original year), unyeared files last.
    assert_eq!(tracks[0].track.title.as_deref(), Some("Closing"));
    assert_eq!(tracks[1].track.title.as_deref(), Some("Opening"));
}

/// A stand-in for LRCLIB answering requests in turn with `answers` (status,
/// JSON body). Returns its URL and each request's first line.
fn fake_lrclib(answers: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (stream, (status, body)) in listener.incoming().zip(answers) {
            let mut stream = stream.unwrap();
            let mut lines = BufReader::new(&stream).lines();
            log.lock().unwrap().push(lines.next().unwrap().unwrap());
            for line in lines.by_ref() {
                if line.unwrap().is_empty() {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (url, seen)
}

#[test]
fn synced_lyrics_win_then_sidecars_then_tags_then_lrclib() {
    use lofty::{
        TextEncoding,
        config::WriteOptions,
        file::TaggedFileExt,
        id3::v2::{
            BinaryFrame, Frame, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame,
            TimestampFormat,
        },
        prelude::ItemKey,
        tag::TagExt,
    };
    use protocol::{Lyrics, LyricsLine, LyricsSource};
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let (album_a, album_b) = (
        music.join("Artist A/First Album"),
        music.join("Artist B/Second Album"),
    );
    // Embedded: Vorbis comments with both kinds, an MP3 with a SYLT frame.
    let ogg = album_b.join("02 Ending.ogg");
    let mut tag = lofty::read_from_path(&ogg)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    tag.insert_text(ItemKey::UnsyncLyrics, "Plain words".into());
    tag.insert_text(ItemKey::Lyrics, "[00:01.00]Ogg line".into());
    tag.save_to_path(&ogg, WriteOptions::default()).unwrap();
    let sylt = SynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        TimestampFormat::MS,
        SyncTextContentType::Lyrics,
        None,
        vec![(2500, "Second".into()), (500, "First".into())],
    );
    let mut id3 = Id3v2Tag::new();
    id3.insert(Frame::Binary(BinaryFrame::new(
        FrameId::Valid("SYLT".into()),
        sylt.as_bytes(WriteOptions::default()).unwrap(),
    )));
    id3.save_to_path(album_b.join("01 Beginning.mp3"), WriteOptions::default())
        .unwrap();
    // Synced beats plain whatever the source, then sidecars beat tags: the
    // Ogg's .lrc wins, the MP3's plain .txt doesn't hide its SYLT.
    fs::write(album_b.join("02 Ending.lrc"), "[00:02.00]Sidecar ending\n").unwrap();
    fs::write(album_b.join("01 Beginning.txt"), "Plain sidecar\n").unwrap();
    // Sidecar: the synced .lrc beats the plain .txt.
    fs::write(album_a.join("01 Opening.txt"), "Sidecar words\n").unwrap();
    fs::write(
        album_a.join("01 Opening.lrc"),
        "[ti:Opening]\n[00:03.00]Sidecar line\n",
    )
    .unwrap();

    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning);
    let line = |ms, text: &str| LyricsLine {
        time_ms: Some(ms),
        text: text.into(),
    };
    let found = |source, lines| Some(Lyrics { source, lines });
    let (lrclib, seen) = fake_lrclib(vec![
        (404, r#"{"code":404}"#),
        (
            200,
            r#"{"plainLyrics":"Plain","syncedLyrics":"[00:04.00]Fetched","instrumental":false}"#,
        ),
    ]);
    let lyrics = |title: &str, lrclib: Option<&str>| {
        library
            .lyrics(&track(&library, title).track.id, lrclib)
            .unwrap()
    };
    assert_eq!(
        lyrics("Ending", Some(&lrclib)),
        found(LyricsSource::Sidecar, vec![line(2000, "Sidecar ending")])
    );
    assert_eq!(
        lyrics("Beginning", Some(&lrclib)),
        found(
            LyricsSource::Embedded,
            vec![line(500, "First"), line(2500, "Second")]
        )
    );
    assert_eq!(
        lyrics("Opening", Some(&lrclib)),
        found(LyricsSource::Sidecar, vec![line(3000, "Sidecar line")])
    );
    // Nothing local: LRCLIB, never asked while it is off.
    assert_eq!(lyrics("Closing", None), None);
    assert!(seen.lock().unwrap().is_empty());
    // A miss is cached, and asked again once it is old.
    assert_eq!(lyrics("Closing", Some(&lrclib)), None);
    let request = seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with(
            "GET /api/get?artist_name=Artist+A&track_name=Closing&album_name=First+Album&duration="
        ),
        "{request}"
    );
    let db = rusqlite::Connection::open(data.join("library.db")).unwrap();
    let cached = || {
        db.query_row("SELECT count(*) FROM lyrics WHERE fetched > 0", [], |r| {
            r.get::<_, u32>(0)
        })
        .unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while cached() == 0 {
        assert!(Instant::now() < deadline, "never cached");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(lyrics("Closing", Some(&lrclib)), None);
    assert_eq!(seen.lock().unwrap().len(), 1, "the miss was cached");
    db.execute("UPDATE lyrics SET fetched = 0", []).unwrap();
    let fetched = found(LyricsSource::Lrclib, vec![line(4000, "Fetched")]);
    assert_eq!(lyrics("Closing", Some(&lrclib)), fetched);
    let deadline = Instant::now() + Duration::from_secs(10);
    while cached() == 0 {
        assert!(Instant::now() < deadline, "never cached");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Cached now, so even with LRCLIB gone it answers; saving bakes in the offset.
    assert_eq!(lyrics("Closing", Some("http://127.0.0.1:9")), fetched);
    let closing = track(&library, "Closing").track.id;
    library.save_lyrics(&closing, -1500, Some(&lrclib)).unwrap();
    assert_eq!(
        fs::read_to_string(album_a.join("02 Closing.lrc")).unwrap(),
        "[00:02.50]Fetched\n"
    );
    assert_eq!(
        lyrics("Closing", None),
        found(LyricsSource::Sidecar, vec![line(2500, "Fetched")])
    );
    assert_eq!(seen.lock().unwrap().len(), 2);
    // Saving over the sidecar it came from only moves its offset.
    let lrc = album_a.join("02 Closing.lrc");
    fs::write(&lrc, "[ar:Someone]\n[00:02.50]Fetched <00:03.00>word\n").unwrap();
    library.save_lyrics(&closing, 500, None).unwrap();
    assert_eq!(
        fs::read_to_string(&lrc).unwrap(),
        "[offset:-500]\n[ar:Someone]\n[00:02.50]Fetched <00:03.00>word\n"
    );
    // Another .lrc (here plain, hidden by the embedded synced lyrics) is never replaced.
    let beginning = track(&library, "Beginning").track.id;
    fs::write(album_b.join("01 Beginning.lrc"), "Mine\n").unwrap();
    assert!(library.save_lyrics(&beginning, 0, None).is_err());
    assert_eq!(
        fs::read_to_string(album_b.join("01 Beginning.lrc")).unwrap(),
        "Mine\n"
    );
    // After an error LRCLIB is left alone for a while.
    fs::remove_file(album_a.join("02 Closing.lrc")).unwrap();
    db.execute("DELETE FROM lyrics", []).unwrap();
    assert_eq!(lyrics("Closing", Some("http://127.0.0.1:9")), None);
    assert_eq!(lyrics("Closing", Some(&lrclib)), None);
    assert_eq!(seen.lock().unwrap().len(), 2, "backing off");
    assert!(
        library
            .save_lyrics("/not/in/the/library.flac", 0, None)
            .is_err()
    );
}

#[test]
fn embedded_covers_are_read_from_the_track_not_copied() {
    use lofty::{
        config::WriteOptions,
        picture::{MimeType, Picture, PictureType},
        prelude::*,
    };
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cover = fs::read(fixtures.join("Artist B/Second Album/cover.png")).unwrap();
    for name in ["01 Opening.flac", "02 Closing.flac"] {
        let flac = music.join(name);
        fs::copy(fixtures.join("Artist A/First Album").join(name), &flac).unwrap();
        let mut tagged = lofty::read_from_path(&flac).unwrap();
        let tag = tagged.primary_tag_mut().unwrap();
        // A second cover for Closing, whose file goes missing below.
        let bytes = [cover.clone(), vec![name.len() as u8]].concat();
        tag.push_picture(
            Picture::unchecked(if name.starts_with("01") {
                cover.clone()
            } else {
                bytes
            })
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Png)
            .build(),
        );
        tag.save_to_path(&flac, WriteOptions::default()).unwrap();
    }
    let art_files = || {
        let mut names: Vec<String> = fs::read_dir(data.join("art"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };

    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 2 && !s.scanning && s.hashing == 0);
    let key = track(&library, "Opening").art.unwrap();
    let closing = track(&library, "Closing").art.unwrap();
    let bar = |key: &str| format!("{key}-{}.jpg", library::BAR_ART_SIZE);
    let mut thumbs = vec![bar(&key), bar(&closing)];
    thumbs.sort();
    assert_eq!(art_files(), thumbs, "only thumbnails are cached");
    assert_eq!(
        library.art_image(&key, None).unwrap(),
        (cover.clone(), "image/png")
    );
    let small = library.art(&key, Some(32)).unwrap();
    assert!(small.starts_with(data.join("art")) && small.is_file());

    // Engines from before kept full-size copies: an upgrade removes them
    // (and copies of other pictures nobody refers to), except the only copy
    // of a missing track's cover.
    let thumbs = art_files();
    drop(library);
    let old = rusqlite::Connection::open(data.join("library.db")).unwrap();
    for k in [&key, &closing] {
        let copy = data.join(format!("art/{k}.png"));
        fs::write(&copy, if *k == key { &cover[..] } else { b"old copy" }).unwrap();
        old.execute(
            "UPDATE art SET path = ?1 WHERE key = ?2",
            [copy.to_str().unwrap(), k],
        )
        .unwrap();
    }
    fs::write(data.join("art/0123456789abcdef0123456789abcdef.jpg"), b"x").unwrap();
    old.execute("UPDATE tracks SET missing = 1 WHERE art = ?1", [&closing])
        .unwrap();
    old.pragma_update(None, "user_version", 9).unwrap();
    drop(old);
    fs::remove_file(music.join("02 Closing.flac")).unwrap();
    let library = Library::open(&data).unwrap();
    let mut kept = [thumbs, vec![format!("{closing}.png")]].concat();
    kept.sort();
    assert_eq!(art_files(), kept);
    assert_eq!(library.art_image(&key, None).unwrap().0, cover);
    let _ = (fs::remove_dir_all(data), fs::remove_dir_all(music));
}

#[test]
fn a_shared_cover_survives_losing_the_track_it_was_read_from() {
    use lofty::{
        config::WriteOptions,
        picture::{MimeType, Picture, PictureType},
        prelude::*,
    };
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cover = fs::read(fixtures.join("Artist B/Second Album/cover.png")).unwrap();
    for name in ["01 Opening.flac", "02 Closing.flac"] {
        let flac = music.join(name);
        fs::copy(fixtures.join("Artist A/First Album").join(name), &flac).unwrap();
        let mut tagged = lofty::read_from_path(&flac).unwrap();
        let tag = tagged.primary_tag_mut().unwrap();
        tag.push_picture(
            Picture::unchecked(cover.clone())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::Png)
                .build(),
        );
        tag.save_to_path(&flac, WriteOptions::default()).unwrap();
    }
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 2 && !s.scanning && s.hashing == 0);
    let key = track(&library, "Opening").art.unwrap();
    let db = rusqlite::Connection::open(data.join("library.db")).unwrap();
    let source: String = db
        .query_row("SELECT path FROM art WHERE key = ?1", [&key], |r| r.get(0))
        .unwrap();

    // Deleted (not yet noticed by a rescan): the other track has the cover.
    fs::remove_file(&source).unwrap();
    assert_eq!(library.art_image(&key, None).unwrap().0, cover);
    assert!(library.art(&key, Some(40)).unwrap().is_file());
    let _ = (fs::remove_dir_all(data), fs::remove_dir_all(music));
}

#[test]
fn opus_tracks_are_hashed_and_old_codec_flags_cleared() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    let opus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/tests/fixtures/sine.opus");
    fs::copy(opus, music.join("sine.opus")).unwrap();
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 1 && !s.scanning && s.hashing == 0);
    let db = rusqlite::Connection::open(data.join("library.db")).unwrap();
    let problem = |library: &Library| {
        let query = LibraryQuery::Tracks {
            search: None,
            sort: vec![],
            offset: 0,
            limit: 1,
        };
        let QueryResult::Tracks { tracks, .. } = library.query(query).unwrap() else {
            panic!()
        };
        tracks[0].problem.clone()
    };
    let hash = || -> String {
        db.query_row("SELECT audio_hash FROM tracks", [], |r| {
            r.get::<_, Option<String>>(0)
        })
        .unwrap()
        .unwrap_or_default()
    };
    assert!(hash().starts_with("pcm-b3:"), "{}", hash());
    assert_eq!(problem(&library), None);

    // Engines from before flagged every Opus file: an upgrade checks them again.
    drop(library);
    db.execute(
        "UPDATE tracks SET audio_hash = '', problem = 'unsupported feature: core (codec): unsupported audio codec'",
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 10).unwrap();
    let library = Library::open(&data).unwrap();
    wait_until(&library, |_| !hash().is_empty());
    assert_eq!(problem(&library), None);
    assert!(hash().starts_with("pcm-b3:"), "{}", hash());
    let _ = (fs::remove_dir_all(data), fs::remove_dir_all(music));
}

fn playlists(library: &Library) -> Vec<protocol::Playlist> {
    let QueryResult::Playlists { playlists } = library.query(LibraryQuery::Playlists).unwrap()
    else {
        panic!()
    };
    playlists
}

/// A playlist's tracks by title, with their item ids.
fn entries(library: &Library, id: &str) -> Vec<(String, String)> {
    let QueryResult::PlaylistTracks { entries } = library
        .query(LibraryQuery::PlaylistTracks { id: id.into() })
        .unwrap()
    else {
        panic!()
    };
    entries
        .into_iter()
        .map(|e| (e.item, e.track.track.title.unwrap()))
        .collect()
}

fn titles_of(entries: &[(String, String)]) -> Vec<&str> {
    entries.iter().map(|(_, t)| t.as_str()).collect()
}

#[test]
fn playlists_are_made_edited_played_and_survive_moves_and_repeats() {
    let (data, music) = (temp_dir("data"), temp_dir("music"));
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        &music,
    );
    let library = Library::open(&data).unwrap();
    library
        .query(LibraryQuery::AddRoot {
            path: music.to_str().unwrap().into(),
        })
        .unwrap();
    wait_until(&library, |s| s.tracks == 4 && !s.scanning && s.hashing == 0);
    let path = |title: &str| track(&library, title).track.id;
    let revision = library.status().playlists;

    // Every command is sent twice, as a retrying remote client would.
    let twice = |query: LibraryQuery| {
        library.query(query.clone()).unwrap();
        library.query(query).unwrap();
    };
    twice(LibraryQuery::CreatePlaylist {
        id: "p1".into(),
        name: "  Road trip ".into(),
        paths: vec![path("Opening"), path("Closing")],
    });
    twice(LibraryQuery::AddToPlaylist {
        id: "p1".into(),
        op: "a1".into(),
        paths: vec![path("Ending")],
    });
    assert_eq!(
        titles_of(&entries(&library, "p1")),
        ["Opening", "Closing", "Ending"]
    );
    assert!(library.status().playlists > revision, "clients hear of it");

    let items = entries(&library, "p1");
    twice(LibraryQuery::MovePlaylistItem {
        id: "p1".into(),
        item: items[2].0.clone(),
        before: Some(items[0].0.clone()),
    });
    assert_eq!(
        titles_of(&entries(&library, "p1")),
        ["Ending", "Opening", "Closing"]
    );
    twice(LibraryQuery::MovePlaylistItem {
        id: "p1".into(),
        item: items[2].0.clone(),
        before: None,
    });
    assert_eq!(
        titles_of(&entries(&library, "p1")),
        ["Opening", "Closing", "Ending"]
    );

    twice(LibraryQuery::RemoveFromPlaylist {
        id: "p1".into(),
        items: vec![items[1].0.clone()],
    });
    twice(LibraryQuery::RenamePlaylist {
        id: "p1".into(),
        name: "Drive".into(),
    });
    let list = playlists(&library);
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].id.as_str(), list[0].name.as_str(), list[0].tracks),
        ("p1", "Drive", 2)
    );
    assert!(
        library
            .query(LibraryQuery::RenamePlaylist {
                id: "p1".into(),
                name: " ".into(),
            })
            .is_err()
    );
    assert!(
        library
            .query(LibraryQuery::AddToPlaylist {
                id: "nope".into(),
                op: "a2".into(),
                paths: vec![path("Ending")],
            })
            .is_err()
    );

    let Command::SetContext {
        tracks,
        start,
        kind,
        label,
    } = library
        .selection(&LibraryQuery::PlayPlaylist {
            id: "p1".into(),
            start: 1,
        })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        tracks
            .iter()
            .map(|t| t.title.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["Opening", "Ending"]
    );
    assert_eq!(
        (start, kind, label.as_deref()),
        (Some(1), ContextKind::List, Some("Drive"))
    );

    // A moved file is found again by its audio hash.
    let opening = PathBuf::from(path("Opening"));
    let moved = opening.with_file_name("01 Opening (moved).flac");
    fs::rename(&opening, &moved).unwrap();
    library.query(LibraryQuery::Rescan).unwrap();
    wait_until(&library, |s| {
        s.tracks == 5 && s.missing == 1 && !s.scanning && s.hashing == 0
    });
    let QueryResult::PlaylistTracks {
        entries: moved_entries,
    } = library
        .query(LibraryQuery::PlaylistTracks { id: "p1".into() })
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(moved_entries[0].track.track.id, moved.to_str().unwrap());
    assert!(!moved_entries[0].track.missing);

    twice(LibraryQuery::DeletePlaylist { id: "p1".into() });
    assert!(playlists(&library).is_empty());
    let _ = fs::remove_dir_all(data);
    let _ = fs::remove_dir_all(music);
}
