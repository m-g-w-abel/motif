# Layouts

A layout arranges the **music area**: the part of the window that shows what
you're browsing (the library now, saved playlists later). The app has one set
of widgets (track list, album gallery, artwork, track info, controls, queue),
and a layout only chooses which of them appear there, where, and how each is
configured. The now-playing screen is built from the same widgets.

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
| Tracks + art | Track list with artwork and track details beside it |
| Details | Columns, artwork and a full metadata table |
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

### `split`

`split row { ... }` places its children side by side; `split column { ... }`
stacks them. Splits nest. On phones, `row` splits stack vertically.

### `tracklist`

The tracks of whatever is being browsed.

| Property | Values |
|---|---|
| `style` | `table`: one row per track with column headers (default). `grouped`: tracks under album headers, numbered per album |
| `max-width` | pixels; centres the list at this width on wide screens |

Child `columns` lists the columns in order: `index`, `title`, `artist`,
`album`, `format`, `duration`. Defaults: `columns index title artist album
duration` for `table`, `columns index artist title duration` for `grouped`.

Title takes the remaining width. Drag the edge between two column headers to
move it (double-click to reset). Widths are remembered per layout on each
device. Double-click a track to play the list from it.

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

The playing track's artwork, as large as the space allows. Click it to expand
the player.

### `track-info`

Details of the playing track.

| Property | Values |
|---|---|
| `style` | `list`: title, artist and details stacked (default). `table`: a field/value table. `hero`: large type, as on the now-playing screen |

Codec details (bitrate, sample rate, bit depth) are added once real playback
lands in phase 1.

### `controls`

Seek bar with times, and the transport centred between repeat and volume, as
on the now-playing screen.

### `queue`

The play queue (its main home is under the now-playing screen). Handy beside a
library list.

| Property | Values |
|---|---|
| `collapsible` | `#true` adds a header that folds the list away (default `#false`) |

## Planned

These will be added without breaking existing files:
- more widgets: lyrics, visualiser, spectrogram, waveform, stats
- per-device layouts (phone vs desktop)
- column sorting, saved per track list
- drag-to-rearrange, writing the change back into the file
