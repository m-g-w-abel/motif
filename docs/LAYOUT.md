# Layouts

A layout arranges the **music area**: the part of the window that shows what
you're browsing (the library now, saved playlists later). The app has one set
of widgets (track list, album gallery, artwork, track info, controls, queue,
visualiser, lyrics), and a layout only chooses which of them appear there, where, and
how each is configured. The now-playing screen is built from the same widgets.

Layouts never change the rest of the app. The left panel and the player bar
stay put, and appearance (theme, accent colour, density, bar or floating
player) lives in **Settings**, the gear at the bottom of the left panel. On
phones the left panel is a drawer: tap the menu button or swipe in from the
left edge.

Selection and album focus are shared by all layouts, so switching layouts
keeps your place.

## Switching and installing

The buttons at the right of the toolbar above the music area switch layouts.
Each source remembers its own layout (once playlists exist, a playlist can
use a plain list while the library shows a gallery).

| Built-in | |
|---|---|
| Tracks | Album-grouped track list |
| Tracks + art | Track list with artwork, track details and lyrics beside it |
| Details | Columns, artwork, a visualiser and a full metadata table |
| Tracks + gallery | Track list beside a cover gallery |
| Gallery | Album covers only |

Each built-in is a file in `apps/ui/src/lib/layout/presets/`. To make your
own, copy one, change it, and **drop the `.kdl` file onto the window** (or use
**⋯ → Add layout file…**). It appears as another button. A file whose layout
has the same name as an installed one replaces it, and **⋯ → Remove** removes
the current one. Layouts belong to each device; the engine doesn't store
them.

**⋯ → Edit layout file…** opens the current layout's source. Editing a
built-in saves your version as a new "(custom)" layout; the built-in itself
never changes. Errors are listed with line numbers, and nothing is applied
until the file is valid.

## Arranging in place

Drag the line between two widgets to resize them (double-click it to go back
to the file's size). **⋯ → Arrange widgets** shows a handle on each widget:
drag it onto another widget in the same split to move it there, or focus it and
use the arrow keys; **Done** ends it. Mouse and touch both work.

These edits never change the layout file, built-ins included. Each device
keeps them per layout, and separately for the phone variant (below), keyed by
where each widget sits in the file, e.g. `split-row/1.split-column/0.artwork`.
When the file changes so that a path no longer matches, that edit is dropped.
**⋯ → Reset layout** discards them, and **⋯ → Save as…** writes the layout
with them as a new layout file. Sizes and order are all they cover;
adding or removing widgets is done in the file.

## Format

Layouts are written in [KDL](https://kdl.dev). A node is a name followed by
arguments and `key=value` properties, with optional children in `{ }`. Simple
words don't need quotes (`style=grouped`); text with spaces or symbols does
(`name="Tracks + art"`). `//` starts a comment, and `/-` comments out the next
node.

```kdl
layout version=1 name="Tracks + art" description="List with artwork beside it" icon=info

split row {
    tracklist style=grouped
    split column size=300 {
        artwork
        track-info
    }
}
```

The file starts with a `layout` node, followed by the widgets. Several
top-level widgets are stacked top to bottom.

### `layout`

| Property | Values |
|---|---|
| `version` | `1` (required) |
| `name` | shown on the layout's button; defaults to the file name |
| `description` | shown in the button's tooltip |
| `icon` | `list`, `grid`, `split`, `info`, `columns` (default `list`) |

### Sizes

Every widget and split accepts `size`: a number of pixels (`size=300`) or a
percentage of its parent (`size="35%"`). Widgets without a size share the
remaining space equally.

### Phone variant

`when phone { ... }` holds a separate arrangement for screens 700 px wide or
less; the widgets outside it are used everywhere else. Without one, phones use
the main arrangement with row splits stacked.

```kdl
layout version=1 name="Art + list"

split row {
    tracklist
    artwork size=300
}

when phone {
    artwork size="40%"
    queue
}
```

### `split`

`split row { ... }` places its children side by side; `split column { ... }`
stacks them. Splits nest. On phones, `row` splits stack vertically.

### `tracklist`

The tracks of whatever is being browsed.

| Property | Values |
|---|---|
| `album-breaks` | `#true`: tracks under album header rows, numbered per album. Default `#false` |
| `headers` | `#true`: show column titles, which carry resizing and one-off sorting. Default `#true` |
| `style` | shorthand kept for older files: `table` = `headers=#true`, `grouped` = `album-breaks=#true headers=#false`. `album-breaks` and `headers` override it |
| `max-width` | pixels; centres the list at this width on wide screens |

Child `columns` lists the columns in order: `index`, `title`, `artist`,
`album`, `format`, `duration`. Defaults: `columns index title artist album
duration` without album breaks, `columns index artist title duration` with them.

Title takes the remaining width. Drag the edge between two column headers to
move it (double-click to reset). Widths are remembered per layout on each
device and still apply when `headers=#false`, but can't be changed then. Double-click a track to play the list from it.

### `album-grid`

Album covers from whatever is being browsed. Click an album to scroll the
track lists in the same layout to it; double-click to play it. The playing
album is outlined in the accent colour.

| Property | Values |
|---|---|
| `card-size` | minimum card width in pixels, 80–600 (default 180). The number of columns follows the width |
| `columns` | fixed number of columns, 1–24, instead of the automatic count. Capped so cards stay at least 80 px wide |

Ctrl+scroll over the grid changes the column count (scroll down for more).
The choice is remembered per layout on each device.

### `artwork`

The artwork of the track selected in a track list (blank until something is
selected), as large as the space allows. Click it to expand
the player.

### `track-info`

Details of the track selected in a track list (click or arrow to it); a hint
shows until something is selected. The `artwork` widget follows the same selection.

| Property | Values |
|---|---|
| `style` | `list`: title, artist and details stacked (default). `table`: a field/value table. `hero`: large type, as on the now-playing screen |

### `controls`

Seek bar with times, and the transport centred between repeat and volume, as
on the now-playing screen.

### `queue`

What plays next (its main home is under the now-playing screen), in two
parts: **Next in queue**, the tracks added with Queue next or Add to queue
(drag or Alt+↑/↓ to reorder, × or Delete to remove), then **Next from …**, the
rest of the album or list that is playing (an album in full, other lists ten
tracks at a time). Handy beside a library list.

| Property | Values |
|---|---|
| `collapsible` | `#true` adds a header that folds the list away (default `#false`) |

### `visualiser`

Spectrum bars of what is playing (32 bands, 40 Hz to 16 kHz, in the accent
colour), as large as the space allows. It moves only while playing, on screen
and in a visible window; otherwise it is blank and the engine does no analysis.
It shows the music as it is played, whatever the volume. E.g. a strip under
the artwork: `visualiser size=120`.

### `lyrics`

The playing track's lyrics: from a sidecar `.lrc` or `.txt` beside the file,
its tags, or LRCLIB (when it is on in **Settings → Integrations**). Synced
lyrics win over plain ones wherever they come from, then sidecars win over
tags. Synced lyrics highlight the line being sung and keep it in the middle;
click a line to play from there. Plain lyrics just scroll. The footer names
the source and, for synced lyrics, has an offset (− and + move the lines 0.1 s
earlier or later, kept per track on this device; click the value to reset) and
**Save to sidecar**, which writes the lyrics with the offset baked in to a
`.lrc` beside the track. Audio files are never written to. The now-playing
screen has the same view behind its lyrics button. No properties.

## Planned

These will be added without breaking existing files:
- more widgets: spectrogram, waveform, stats
