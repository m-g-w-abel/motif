# Changelog

What changed in Motif, newest first. Motif has no releases yet, so changes are
grouped by phase of the plan.

## Unreleased: phase 5, Android

- **Animated screen changes**: opening and leaving a view slides it, and an
  album's or playlist's cover and title fly from its tile to its page and
  back (the Library's categories too). Swiping back moves the screen with
  your finger. Now playing grows out of the bar with the artist, album, play
  button and seek bar each moving to their place, while the bar's buttons
  drop away. The queue, lyrics, menus and sheets rise from the bottom. A
  swipe on a song shows a green + (add to the queue) or a red × (remove),
  and the songs below a removed one slide up. Swiping now playing down moves
  it with your finger, and on Android the system back swipe moves the screen
  too. Dialogs, the changelog and the selection bar animate in and out, the
  player bar's song changes smoothly, and so do search results and the
  Recently Played switch. Swiping back shows the screen you return to
  sliding in underneath, with the album's cover and title travelling as you
  swipe; the queue's steps can be dragged the same way, with the cover,
  title and controls moving between them. Album covers no longer flicker
  after opening. The Albums view keeps its place when you come back to it,
  and the cover flies back to its tile however far down it is. Letters like
  g are no longer cut off in now playing. A new screen now slides in over
  the one you leave, and going back is the same move in reverse, by tap or
  by swipe; Settings pages follow your finger too, without the player bar
  showing through. Titles keep their shape and change colour and weight
  smoothly as they fly, and Search no longer shakes when you come back to it.
  Recently Played and Added remember whether they showed songs or albums,
  song lists remember how far down you were, and phones lose the thin bar at
  the top of each view (the back button sits above the title instead).
  Swiping back works on phones again, titles land in place with a larger
  system font size, a swipe taken all the way no longer plays again, and
  covers no longer blink empty when you change pages. A swipe back starts
  exactly under your finger and you can tap again as soon as it lands; on
  Android, the system font size now scales the whole app evenly, so titles
  and their icons land in place, without a last hop.
- **Search**: one field for the whole library, with filters for songs,
  albums, artists, album artists, folders and playlists. All shows the
  best-matching artist and their albums first, then other albums, then songs.
  Tap a folder to play it, or an artist to see their albums and songs.
- **Swipes** on touch screens: swipe right to go back, swipe a song left to
  add it to the queue (or, in the queue, to remove it), and swipe down anywhere
  on now playing to close it. Dragging the artwork up or down now does this
  too instead of setting the volume; Settings → Gestures brings that back.
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
