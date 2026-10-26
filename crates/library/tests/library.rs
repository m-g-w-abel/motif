//! Scans the fixture folder (two small albums) and checks what the library
//! answers.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use library::Library;
use protocol::{LibraryQuery, QueryResult, SortKey, SortRule, TrackRef};

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
    assert!(library.art(&art, Some(32)).unwrap().is_file());
    assert_eq!(albums[0].art, None);

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

    let (refs, _) = library
        .selection(&LibraryQuery::Play {
            search: None,
            sort: vec![],
            start: 0,
        })
        .unwrap();
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
    let (refs, _) = library
        .selection(&LibraryQuery::Play {
            search: None,
            sort: by_title.clone(),
            start: 0,
        })
        .unwrap();
    assert_eq!(refs[0].title.as_deref(), Some("Beginning"));
    let (refs, _) = library
        .selection(&LibraryQuery::PlayAlbum {
            album: albums[0].key.clone(),
            sort: vec![SortRule {
                key: SortKey::Track,
                descending: true,
            }],
        })
        .unwrap();
    let names: Vec<_> = refs.iter().map(|r| r.title.clone().unwrap()).collect();
    assert_eq!(names, ["Closing", "Opening"]);
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
