# Theming plan

Goal: make yutu-mux themable. A handful of built-in themes, user
themes imported from plain files (no in-app editor), and, as a
stretch, classic Winamp `.wsz` skins. One theme file works the same
on Linux, macOS and Windows: iced draws every pixel itself, so a theme
is pure data and only its folder location differs per OS.

Each phase below is one PR (`feat/theme-*`), lands green on the full
verification matrix (AGENTS.md section 7) and carries its own
`CHANGELOG.md` line when users can see it. Phases are ordered so every
merge is useful on its own and later phases never rework earlier ones.

## Prior art (what we borrow)

| App | Format | Borrowed idea |
|---|---|---|
| Spotifast (Rust/egui) | JSON per theme: `base` + ~17 semantic color keys, missing keys inherit | Small colors-only format, base inheritance, bundled editable palettes, follow-system light/dark pair, accent from art, file limits, invalid files skipped, `.wsz` support |
| Zed, Helix, Alacritty | TOML/JSON palettes | Semantic keys ("panel", "text"), not per-widget ones |
| VS Code | JSON + published schema | Schema gives authors editor completion |
| Base16 / tinted-theming | 16-color YAML | Optional importer later: a big ready library |
| Spicetify, Obsidian | CSS | Not possible in iced: colors + radii is the honest scope |

## Design decisions

- **Name**: "themes" for users; code module `theme` (UI tokens) and
  `yutu_core::theme` (file model). Winamp files stay "skins".
- **Tokens, not widgets**: a theme sets ~20 semantic colors plus a
  few radii. Views never name a raw color again.
- **Token access**: a process-wide `tokens()` handle (copy of a small
  `Copy` struct behind a `RwLock`), swapped when the theme changes.
  All our style fns are `fn(&iced::Theme)` and `iced::Theme`'s palette
  has too few slots for our surfaces. Rejected: a custom `Theme` type
  implementing every widget `Catalog` (large churn, no user benefit).
  Stock widgets still follow the theme through
  `iced::Theme::custom(name, palette)` built from the tokens.
- **File format**: TOML (`*.toml`), comments allowed, every key
  optional except `name`; `base = "dark" | "light"` fills the rest.
  Spotifast-style `*.json` with the same key names is accepted too.
  Colors are `#RRGGBB` or `#RRGGBBAA`.
- **Fonts stay out of themes (v1)**: the font pick already lives in
  Appearance and needs a restart; a theme switch must be instant.
- **Readability is enforced, not trusted**: `on_accent` is checked
  against `accent` (WCAG contrast); unreadable pairs fall back to the
  better of black/white. The AGENTS.md rule "glyphs on red are always
  white" becomes "glyphs on the accent are always readable".
- **Default look is unchanged**: the built-in Dark theme is today's
  palette byte for byte (`#FF0033` accent etc.).

## Token set (Phase 1 names, Dark values)

| Token | Dark value | Used for |
|---|---|---|
| `window` | `#0A0A0A` | app background, page veil end |
| `panel` | `#121212` | sidebars, drawers, player bar |
| `surface` | `#181818` | cards, Now Playing |
| `surface_hover` | `#282828` | card/row hover |
| `surface_active` | `#2A2A2A` | active nav row, focused field |
| `field` | `#222222` | inputs, menus |
| `outline` | `#3A3A3A` | borders, slider rails, seek track |
| `outline_strong` | `#6E6E6E` | focused borders |
| `text` | `#FFFFFF` | primary text, icons |
| `text_muted` | `#B3B3B3` | secondary text (today `MUTED`) |
| `placeholder` | `#DDDDDD` | input placeholders |
| `accent` | `#FF0033` | active controls, highlights |
| `accent_hover` | `#FF3366` | accent hover |
| `on_accent` | `#FFFFFF` | glyphs/text on the accent |
| `scrim` | `#0000008C` | modal dim, drawer scrim |
| `shadow` | `#00000099` | floating panel shadow |
| `mini_surface` | `#1A1417` | mini player capsule |

Translucent white overlays (`from_rgba(1,1,1,a)`) become
`text` at that alpha (`tk.text.scale_alpha(a)`), so they flip to dark
tints on light themes. Exact list is finalized in Phase 1.

## Phase 1: tokens refactor (no visible change)

1. `theme.rs`: add `Tokens` (`Copy`), `Tokens::DARK`, `tokens()` and
   `set_tokens()`; keep radii (`card` 8, `panel` 12) in it.
2. Replace the 8 color consts and the `FIELD_*` consts with token
   reads; remove the consts.
3. Replace every literal color outside `theme.rs` (`widgets.rs`,
   `input_menu.rs`, `seek_bar.rs`, `eq_graph.rs`, `mini_player.rs`,
   `views_main.rs`, `romaji.rs`) with tokens.
4. Classify the ~118 `Color::WHITE` uses: `text` (on surfaces) vs
   `on_accent` (on red) vs art overlays (stay white, they sit on cover
   art and video, never on a theme surface).
5. `App::theme()` builds `iced::Theme::custom` from the tokens.
6. Guard test: a unit test scans `src/*.rs` (via `include_str!`) and
   fails on `from_rgb8` / `from_rgba` / `Color::WHITE` outside the
   allow-listed theme and art-overlay sites.
7. Proof of no change: layout snapshots before and after are
   byte-identical (hash compare in the PR description).

## Phase 2: theme model in `yutu-core`

1. `yutu_core::theme`: `ThemeSpec { name, base, colors, shape,
   accent_from_art }`, all colors `Option<Rgba>`; hex parser; resolve
   against the base; derived defaults (`accent_hover` lightened from
   `accent`, `surface_hover` from `surface`, `mini_surface` from
   `surface` + accent tint) so a 3-line theme looks finished.
2. Contrast helpers (relative luminance, ratio) and the `on_accent`
   fallback; muted text kept >= 4.5:1 against `surface` when the
   author did not set it.
3. TOML + Spotifast-compatible JSON parsing (`serde`, `toml`), size
   limit 64 KiB, unknown keys warn (not fail).
4. Tests: inheritance, hex forms, derived colors, contrast fallback,
   Spotifast JSON round trip, malformed input errors.

## Phase 3: built-in themes + picker

1. Bundled `crates/yutu-ui/assets/themes/*.toml` (`include_str!`):
   Dark (default), Light, YouTube Music, Catppuccin Mocha/Latte, Nord,
   Rose Pine, Tokyo Night, Gruvbox. Same format users write.
2. Pref `theme` (id), plus `theme_light` / `theme_dark` for the
   "Follow system" choice (`iced::system::theme()` +
   `theme_changes()` subscription, both in iced 0.14).
3. Settings > Appearance: theme dropdown with a swatch strip per row
   (window, surface, accent, text). Instant switch, no restart.
4. `ctl theme list|set <id>|reload` (`docs/CONTROL.md`).
5. Light-theme audit: scrims, shadows, art veils, favorites cover
   glow, seek/EQ rails, mini player, focus borders.
6. Snapshots per theme (`YUTU_SNAPSHOT_THEME=light`).
7. i18n strings in every locale, CHANGELOG, AGENTS.md theme section
   updated (red is the default accent, not a lock).

## Phase 4: user themes

1. Folder `<data>/themes/` (existing data-dir helper, per OS). Scan
   at boot and on reload: max 128 files, 64 KiB each, no symlinks or
   subfolders; bad files listed in Settings with a human reason.
2. Settings: Import theme... (file picker copies the file in), Open
   themes folder, Reload. File picker: make `rfd` available outside
   `innertube` (new default feature) so offline builds stay green.
3. Reload also runs on main-window focus (cheap mtime check), so
   editing a theme and alt-tabbing back shows it. No file watcher dep.
4. `docs/THEMES.md`: format, every key with its Dark/Light value,
   screenshots of where each token shows, examples. JSON Schema at
   `docs/theme.schema.json` (taplo/VS Code completion via
   `#:schema` comment).

## Phase 5: accent from album art

1. Per-theme `accent_from_art` (plus a global Appearance toggle).
2. Pick the most vivid color from the cover thumbnail we already
   downscale for the Now Playing backdrop; reject near-grays; enforce
   contrast against `window` and `on_accent`.
3. Ease between accents on song change through the existing
   `Animate` subscription; fall back to the theme accent when art is
   missing or too gray.

## Phase 6: Winamp skins, palette import

1. `yutu_core::winamp`: read `.wsz` (zip, case-insensitive names,
   unpacked-size and entry-count caps, in memory only).
2. `pledit.txt` (`Normal`, `Current`, `NormalBG`, `SelectedBG`),
   `viscolor.txt` (24 colors) and `main.bmp` sampling map to a
   `ThemeSpec`. `image` gains the `bmp` feature.
3. Import writes a generated `.toml` into the themes folder (users can
   tweak it) and keeps the `.wsz` beside it for Phase 7.
4. Tests on a tiny synthetic skin built in the test (no third-party
   skins bundled).

## Phase 7: Winamp classic mini player (stretch)

1. A themed skin switches the mini player to a Classic layout: the
   275x116 main window drawn from `main.bmp`, `titlebar.bmp`,
   `cbuttons.bmp`, `posbar.bmp`, `volume.bmp`, `balance.bmp`,
   `playpaus.bmp`, `shufrep.bmp`, `monoster.bmp`, `numbers.bmp` /
   `nums_ex.bmp`, and the `text.bmp` bitmap font for the scrolling
   title. Sprite coordinates follow Webamp (MIT) as reference.
2. Sprites are cropped once into image handles at import; 2x scale
   option for HiDPI.
3. Seek, volume and buttons dispatch the existing `Message`s (same
   contracts as the pill: no side doors).
4. `region.txt` shapes on transparent platforms (Linux, macOS);
   rectangular on Windows (opaque DX12 surface), gated on
   `MiniPlatform` like every other mini workaround.
5. Missing sprites fall back to our own drawing (the original Winamp
   base skin is Nullsoft's and cannot ship). No visualizer in v1.

## Out of scope

In-app theme editor, arbitrary CSS, per-widget overrides, modern
Winamp `.wal` XML skins, fonts inside themes (v1).
