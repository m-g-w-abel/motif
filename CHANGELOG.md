# Changelog

New features and notable changes in Motif, newest first (bug fixes are left
out). Motif has no releases yet, so changes are grouped by phase of the plan.

## Phase 6: Sync

- **Downloads for offline listening**: each device has a Downloaded group.
  Playlists (their edits follow), albums, artists (their later albums too)
  and songs added to it download from your other devices to Music/Motif, as
  Opus 256 kbps by default, and play from there; taking them out deletes the
  copies nothing else in the group needs. Only the engine side is in so far;
  the screens come next.
- **Streams ride out a dropped connection**: a song streamed from a linked
  device, and the next one, are fetched whole to a cache on disk, so a short
  network drop no longer stops playback and seeking within what has arrived
  is instant. Cached songs are deleted once they're neither playing nor next.
- **Control your desktop from anywhere**: a phone (or another desktop) whose
  engine is linked keeps controlling it away from home, over the same
  encrypted link, with the same permissions as at home. The connection shows
  whether it goes over the LAN, the internet or a relay.
- **Covers and seek bar waveforms of your other devices' songs**: a song that is
  only on a linked device shows its real cover and waveform, fetched from that
  device the first time and kept here.
- **Songs on your other devices show up in your library**: a linked device's
  songs mix in with yours (the same song on both shows once), marked with a
  cloud and dimmed while that device can't be reached. The sort menu's "Only
  songs on this device" narrows songs, albums, artists and search to what's here.
- **Play songs from your other devices**: a song that is only on a linked
  device plays here, streamed from it in its original quality, with seeking
  and gapless albums. If that device can't be reached, playback stops on
  the song and says so, with Retry in place of Play; or, with Settings →
  Playback → Skip songs that can't be reached, skips past it and says how
  many were skipped.
- **Smaller streams away from home**: off your home network, lossless and
  high-bitrate songs from a linked device come as Opus 256 kbps (128 on a
  metered phone network) instead of the original; lossy songs at or below
  that come as they are. Settings → Devices sets both, and the quality for
  downloads, Original included. Now Playing shows which device a song
  streams from and what arrives, and away from home whether it comes over
  the internet or a relay.
- **Scrobbles from every device**: a device with a scrobbling account also
  sends the plays synced from your devices without one (up to 14 days old),
  each play once.
- **Playlists, history and ratings sync**: linked devices share playlists
  (names, songs, order), what you played (so Recently Played and play counts
  cover every device) and favourites and ratings. Changes made apart merge
  when they meet, a deleted playlist stays deleted, and a device in between
  passes changes on.
- **Swipe left to go forward**: on a phone, swiping left on the Library home
  reopens the view you just went back from, as many steps as you went back.
- **Linked devices reach each other anywhere**: a desktop and phone that sync
  connect over an end-to-end encrypted link, directly or through a relay, at
  home or away, with no account. Settings → Devices shows how each is reached,
  can use your own relay instead of the slow public ones, and can keep them to
  the same network.
- **Stars or hearts**: rate songs with stars (a rating column in the table, the
  now-playing sheet) or love them with a heart (a song's menu), as you choose
  in Settings → Interface; the other is hidden and nothing is lost by
  switching. Lists sort by it, hearts have a Favourites view in the Library,
  and both follow a file when it moves.

## Phase 5: Android

- **Long press selects**: on phones, a long press on a song, album or playlist
  starts select mode with it picked; taps then pick more. Albums and playlists
  act on all their songs in order. A long press no longer queues (swipe left
  still does), and on phones it never opens a menu.
- **Select bar in the player bar's place**: on phones, selecting fades the
  player bar into a select bar of the same size, with Play, Add to queue, Add
  to playlist and ⋮ where the Library, Search and menu buttons were.
- **Simpler Library home on phones**: Add folder and Rescan moved from the
  Library home's ⋮ menu to Settings → Library.
- **Cover colours pick background and accent separately**: the background
  comes from the colour covering most of the cover, the accent from its
  strongest other colour.
- **No back button at the top of phone screens**: go back with the system back
  button or by swiping right.
- **Animated screen changes**: views slide in and out, an album's or
  playlist's cover and title fly from its tile to its page and back, and
  swiping back moves the screen with your finger. Now playing grows out of the
  bar and steps between the queue and lyrics as one move you can drag and
  catch halfway; menus, dialogs and sheets rise from the bottom.
- **Search**: one field for the whole library, with filters for songs,
  albums, artists, album artists, folders and playlists. All shows the
  best-matching artist and their albums first, then other albums, then songs.
  Tap a folder to play it, or an artist to see their albums and songs.
- **Swipes** on touch screens: swipe right to go back, swipe a song left to
  add it to the queue (or, in the queue, to remove it), and swipe down anywhere
  on now playing to close it. Settings → Gestures remaps them.
- **Changelog and Help**: this changelog, in the menu on phones and the left
  panel on desktop, and Help, which opens the project's documentation online.
- The phone's now-playing sheet steps, by drag or its handle, between now
  playing, a half queue under the cover, a full reorderable queue, and lyrics.
- A shuffle button next to repeat, in the player and the bar.
- The same engine runs inside the Android app, with background playback that
  holds up through deep Doze, a notification and lock-screen controls.
- The phone's volume keys and Motif's volume are one control; while the phone
  controls another engine, the keys set that engine's volume.
- Calls, alarms, other players, unplugged headphones and navigation prompts
  pause or duck playback the way a good Android audio app should.
- Pair with another engine by scanning its QR code with the camera, or by
  picking it from the LAN and typing its code.
- System back closes the topmost menu, dialog, screen or sheet, then goes back
  through the views.
- A Library home (All Songs, Albums, Playlists, Recently Played, Recently
  Added), with album and playlist pages, per-view searching, and a Select mode
  to play, queue or add several songs at once.
- Albums, playlists and songs show as a grid, a list or a list with covers,
  remembered per device for each kind of view.
- Saved playlists live in the engine, so every paired device sees the same
  ones.
- Shuffle is engine state, and what plays keeps up with its source as tracks
  are added or removed.
- Phones get a bottom bar with a mini player and a menu, a full-screen
  now-playing sheet, and remappable touch gestures.
- Any view can be pinned to the phone's home screen as a shortcut.
- 20,000 tracks scan in under 7 minutes on a phone.

## Phase 4: remote control

- Pair devices by QR code, with device keys and scoped, revocable tokens (Full,
  Control playback, View only).
- Find engines on the LAN over mDNS, and switch between them.
- An MPD server, so existing MPD clients can drive Motif.

## Phase 3: cheap wins

- Synced, karaoke-style lyrics from embedded tags, `.lrc` sidecars or LRCLIB.
- Scrobbling to Last.fm, ListenBrainz and Libre.fm, with an offline queue.
- Discord Rich Presence.
- ReplayGain, with loudness measured on demand for untagged files.

## Phase 2: looks great

- Theme files, and colour palettes taken from the album art.
- Layouts are editable by dragging, and new layouts and themes install by
  dropping a file on the window.
- A waveform seek bar and a spectrum visualiser.
- Large webview performance fixes: the album grid went from 12 to 62 fps on CPU
  rendering.

## Phase 1: desktop player

- Local files play through decoder plugins (Symphonia, libopus, and FFmpeg for
  everything else) with sample-exact gapless playback.
- A library scanned from your folders into SQLite, with search, folder
  watching and artwork.
- A play queue that survives a restart, coming back paused at the same track
  and position.
- OS media controls (MPRIS, SMTC, macOS Now Playing).

## Phase 0: foundations

- A headless engine, a WebSocket protocol with generated TypeScript types, and
  a Svelte interface arranged by layout files.
