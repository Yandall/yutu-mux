# Themes

yutu-mux ships a few built-in themes and reads your own from plain
text files. There is no in-app editor: a theme is a small TOML (or
JSON) file you write in any editor. The same file works on Linux,
macOS and Windows.

Pick a theme in **Settings > Appearance > Theme**. **Follow system**
switches between a light and a dark theme of your choice when your
OS changes mode.

## Built-in themes

Dark (default), Light, YouTube Music, Catppuccin Mocha, Catppuccin
Latte, Nord, Rose Pine, Tokyo Night, Gruvbox Dark. Their files live in
[`crates/yutu-ui/assets/themes/`](../crates/yutu-ui/assets/themes/)
and are good starting points.

## Where theme files go

| OS | Folder |
|---|---|
| Linux | `$XDG_DATA_HOME/yutu-mux/themes/` (usually `~/.local/share/yutu-mux/themes/`) |
| macOS | `~/.local/share/yutu-mux/themes/` (or `$XDG_DATA_HOME/yutu-mux/themes/`) |
| Windows | `%APPDATA%\yutu-mux\themes\` |

**Open themes folder** in Settings creates and opens it. **Import
theme** copies a file you pick into it (after checking that it
parses) and selects it. Every `.toml` and `.json` file directly in the
folder is a theme; the file name (without extension) is its id.

The folder is re-read when you press **Reload themes**, run
`yutu-mux ctl theme reload`, or switch back to the app window, so you
can keep the file open in an editor and alt-tab to see changes.

A user file with the same id as a built-in (for example `nord.toml`)
replaces that built-in.

Limits: 64 KiB per file, 128 files. Files that cannot be used are
listed under the picker with the reason; the rest still load.

## Format

```toml
#:schema ./theme.schema.json
name = "Midnight"      # shown in the picker (default: the file name)
base = "dark"          # "dark" or "light": fills every key you omit
accent_from_art = false

[colors]
window = "#0B0B12"
surface = "#161624"
text = "#E8E8F0"
accent = "#7C5CFF"
```

Every key is optional. Colors are `#RGB`, `#RRGGBB` or `#RRGGBBAA`.
Unknown keys are ignored with a warning, so a theme written for a newer
version still loads.

You rarely need many keys. Keys you leave out are **derived** from the
ones you set: set `window`, `text` and `accent` and you get matching
surfaces, hovers, borders and muted text. A key you set always wins
over derivation.

`on_accent` (glyphs drawn on the accent) is always kept readable: when
it has less than 3:1 contrast against `accent`, black or white is used
instead and the file gets a warning.

### Color keys

| Key | Used for | Dark | Light |
|---|---|---|---|
| `window` | app background | `#0A0A0A` | `#F0F0F2` |
| `panel` | sidebars, drawers, player bar | `#121212` | `#FAFAFB` |
| `surface` | cards, Now Playing sheet | `#181818` | `#FFFFFF` |
| `surface_hover` | card and row hover | `#282828` | `#ECECEE` |
| `surface_active` | active nav row, focused field | `#2A2A2A` | `#E4E4E7` |
| `inset` | segmented pickers (EQ presets) | `#161616` | `#F3F3F5` |
| `field` | text inputs, secondary buttons | `#222222` | `#F2F2F4` |
| `field_hover` | secondary buttons hovered | `#333333` | `#E6E6E9` |
| `menu` | text field context menu | `#242424` | `#FFFFFF` |
| `outline` | borders, slider rails, seek track | `#3A3A3A` | `#D2D2D6` |
| `outline_strong` | focused borders | `#6E6E6E` | `#9A9AA0` |
| `text` | titles, icons | `#FFFFFF` | `#121214` |
| `text_body` | plain labels | `#E6E6E6` | `#1E1E22` |
| `text_muted` | secondary text, idle icons | `#B3B3B3` | `#5C5C63` |
| `text_disabled` | disabled entries | `#6E6E6E` | `#A8A8AE` |
| `placeholder` | input placeholders | `#DDDDDD` | `#6E6E75` |
| `accent` | active controls, the live song | `#FF0033` | `#E0002D` |
| `accent_hover` | accent under the pointer | `#FF3366` | `#FF0033` |
| `on_accent` | glyphs on the accent | `#FFFFFF` | `#FFFFFF` |
| `handle` | slider, toggle and seek knobs | `#FFFFFF` | `#FFFFFF` |
| `handle_edge` | knob outline | transparent | `#9A9AA0` |
| `scrim` | dim behind dialogs and drawers | `#0000008C` | `#00000059` |
| `shadow` | drop shadows | `#000000` | `#00000080` |

Derived when omitted (from the keys in brackets): `surface` (window,
text), `panel` and `inset` (window, surface), `surface_hover`,
`surface_active`, `field`, `field_hover`, `menu`, `outline`,
`outline_strong` (surface, text), `text_body`, `text_muted`,
`text_disabled`, `placeholder` (text, surface), `accent_hover` and
`on_accent` (accent), `handle_edge` (an outline only when the knob
has less than 3:1 contrast with `panel`).

### Readability

Every theme is checked for contrast (WCAG ratios), and colors that
fall short are moved toward white or black just enough to pass, so a
theme can never make text unreadable:

| Role | Minimum |
|---|---|
| `text`, `text_body` | 7:1 on `panel` and `surface`, 4.5:1 on `window`, `surface_hover`, `field` |
| `text_muted` | 4.5:1 on `window`, `panel`, `surface` |
| `placeholder` | 4.5:1 on `field` |
| `text_disabled` | 2:1 on `menu` (faint on purpose) |
| `accent` | 3:1 on `window`, `panel`, `surface` |
| `on_accent` | 3:1 on `accent` (black or white) |

A color you set yourself that had to move gets a warning in the log
(and `ctl theme`), so you can pick a better one. Over cover art (Now
Playing, album and playlist headers) the backdrop veil also thickens
for bright or dark covers, keeping text at 7:1 on the art.

Content drawn over cover art (the hover play icon, the mini player's
glow) stays white in every theme: it sits on the art, not on a theme
surface.

### Accent from album art

`accent_from_art = true` makes the theme take its accent from the
playing song's cover (the most vivid common color, adjusted so it
always reads on `window`). The **Accent color from album art** switch
in Settings, or `yutu-mux ctl theme art on|off`, overrides the theme's
choice.

## Spotifast themes

JSON files in the [Spotifast](https://spotifast.rocks) format load as
they are: `base` plus `colors` with the same key names. Its
`secondary`, `dim` and `overlay` keys map to `text_muted`,
`text_disabled` and `scrim`; `danger`, `warning` and `tint` are
accepted and ignored.

```json
{ "base": "dark", "colors": { "accent": "#1DB954", "secondary": "#A0A0A0" } }
```

## Editor completion

[`theme.schema.json`](theme.schema.json) describes the format. Editors
with TOML schema support (Taplo, VS Code's Even Better TOML) pick it up
from a `#:schema ./theme.schema.json` first line when the schema sits
next to your theme (copy it into the themes folder), or from its URL.

## Control socket

```sh
yutu-mux ctl theme                 # themes, picks, failures (JSON)
yutu-mux ctl theme set nord        # or: theme set system
yutu-mux ctl theme reload
yutu-mux ctl theme art on
```
