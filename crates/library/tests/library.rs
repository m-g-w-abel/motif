//! Scans the fixture folder (two small albums) and checks what the library
//! answers.

use std::{
    fs,
    path::{Path, PathBuf},
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
        })
        .unwrap()
    else {
        panic!()
    };
    let names: Vec<_> = refs.iter().map(|r| r.title.clone().unwrap()).collect();
    assert_eq!(names, ["Closing", "Opening"]);
    assert_eq!(kind, ContextKind::Album);
    assert_eq!(label, refs[0].album);
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
