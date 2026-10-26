# Theme files

A theme is a small KDL file that sets the app's colours and fonts. Nothing
else: no CSS, no sizes. Install one by dropping the `.kdl` file onto the
window or with Settings → Interface → Install theme…, and remove it from the
same place (select it, then Remove). Themes are stored per device. Installing
a theme with the same `name` replaces the old one. The accent colour is a
separate setting and works with every theme.

```kdl
theme version=1 name="Dusk" scheme="dark" font-sans="Georgia, serif" font-mono="Fira Code, monospace"
bg "#1b1626"
surface "#241d33"
surface-2 "#2e2640"
hover "#ffffff12"
border "#372e4d"
divider "#54476f"
text "#ece8f5"
text-muted "#9c93b3"
shadow "0 8px 32px rgb(0 0 0 / 0.5)"
```

## The `theme` line

| Property | |
|---|---|
| `version` | Required, `1`. |
| `scheme` | Required, `"light"` or `"dark"`. Sets native controls and picks the built-in theme that fills any token you leave out. |
| `name` | Shown in Settings. Defaults to the file name. |
| `font-sans`, `font-mono` | Optional comma-separated font names (letters, digits, spaces, commas, quotes, hyphens). Only installed fonts work; end with a generic family such as `sans-serif`. |

## Tokens

Each token is a node with one text value. All are optional.

| Token | Value |
|---|---|
| `bg`, `surface`, `surface-2`, `border`, `divider`, `text`, `text-muted` | Hex colour: `#rgb` or `#rrggbb`. |
| `hover` | Hex colour, may have alpha: `#rrggbbaa`. |
| `shadow` | A box-shadow such as `0 8px 32px rgb(0 0 0 / 0.5)`. |

## Validation

Errors (wrong version, unknown token, bad colour, a second value for a token…)
are all reported with their line, and the file isn't installed. Poor contrast
doesn't block installing but shows a warning: `text` should reach 4.5:1 and
`text-muted` 3:1 against `bg`, `surface` and `surface-2`.

The built-in themes are still defined in `apps/ui/src/lib/themes.ts`, not as
files.
