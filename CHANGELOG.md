# Changelog

What changed in Motif, newest first. Motif has no releases yet, so changes are
grouped by phase of the plan.

## Unreleased: phase 5, Android

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
