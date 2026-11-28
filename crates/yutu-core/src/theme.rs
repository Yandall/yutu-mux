//! Theme files: the data model behind yutu-mux themes.
//!
//! A theme is plain data (TOML, or Spotifast-style JSON): a `base`
//! (`dark` or `light`) plus any subset of the semantic color keys in
//! [`Palette`]. Keys an author leaves out come from the base, or are
//! derived from the keys they did set (a theme that only sets
//! `window`, `text` and `accent` still gets matching hovers, borders
//! and muted text). The UI turns the resolved [`Palette`] into its
//! token struct; nothing here knows about iced, so a theme renders the
//! same on every OS.
//!
//! Rules:
//! - Unknown keys warn, never fail (forward compatible files).
//! - Glyphs on the accent stay readable: an `on_accent` below 3:1
//!   contrast is replaced by black or white, with a warning.
//! - Files are capped at [`MAX_THEME_BYTES`]; a folder scan at
//!   [`MAX_THEME_FILES`] themes (no symlinks, no subfolders).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Largest theme file accepted (bytes).
pub const MAX_THEME_BYTES: usize = 64 * 1024;
/// Most theme files read from one folder.
pub const MAX_THEME_FILES: usize = 128;
/// Minimum contrast for glyphs on the accent (WCAG large-text/UI).
pub const MIN_ON_ACCENT_CONTRAST: f32 = 3.0;

/// An sRGB color with alpha, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const WHITE: Rgba = Rgba::rgb(0xFF, 0xFF, 0xFF);
    pub const BLACK: Rgba = Rgba::rgb(0x00, 0x00, 0x00);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xFF }
    }

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional).
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        match hex.len() {
            3 => {
                let nib = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
                Some(Self::rgb(nib(0)?, nib(1)?, nib(2)?))
            }
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }

    /// `#RRGGBB`, or `#RRGGBBAA` when not opaque.
    pub fn to_hex(self) -> String {
        if self.a == 0xFF {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    /// Linear blend toward `other` by `t` (0 = self, 1 = other); keeps
    /// this color's alpha.
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let ch = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba::new(
            ch(self.r, other.r),
            ch(self.g, other.g),
            ch(self.b, other.b),
            self.a,
        )
    }

    /// WCAG relative luminance (alpha ignored).
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// WCAG contrast ratio between two colors (1.0 ..= 21.0).
    pub fn contrast(self, other: Rgba) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// White or black, whichever reads on this color. White wins ties
    /// down to [`MIN_ON_ACCENT_CONTRAST`] (it matches the app's look).
    pub fn readable_glyph(self) -> Rgba {
        if Rgba::WHITE.contrast(self) >= MIN_ON_ACCENT_CONTRAST
            || Rgba::WHITE.contrast(self) >= Rgba::BLACK.contrast(self)
        {
            Rgba::WHITE
        } else {
            Rgba::BLACK
        }
    }

    /// This color nudged toward white or black (whichever direction
    /// the backgrounds leave more room in) until it reaches `min`
    /// contrast against every background, keeping its hue as long as
    /// possible. Already-readable colors come back unchanged.
    pub fn ensure_contrast(self, backgrounds: &[Rgba], min: f32) -> Rgba {
        let worst = |c: Rgba| {
            backgrounds
                .iter()
                .map(|b| c.contrast(*b))
                .fold(f32::INFINITY, f32::min)
        };
        if backgrounds.is_empty() || worst(self) >= min {
            return self;
        }
        // Toward the extreme that can reach the target: light text on
        // dark backgrounds, dark text on light ones.
        let darkest = backgrounds
            .iter()
            .map(|b| b.luminance())
            .fold(f32::INFINITY, f32::min);
        let brightest = backgrounds
            .iter()
            .map(|b| b.luminance())
            .fold(0.0, f32::max);
        let room_up = (1.05) / (brightest + 0.05);
        let room_down = (darkest + 0.05) / 0.05;
        let target = if room_up >= room_down {
            Rgba::WHITE
        } else {
            Rgba::BLACK
        };
        // Smallest blend that reaches `min` (binary search on 0..1).
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..16 {
            let mid = (lo + hi) / 2.0;
            if worst(self.mix(target, mid)) >= min {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        self.mix(target, hi)
    }

    /// HSV saturation 0..1 and value 0..1 (for art accent picking).
    pub fn saturation_value(self) -> (f32, f32) {
        let max = self.r.max(self.g).max(self.b) as f32 / 255.0;
        let min = self.r.min(self.g).min(self.b) as f32 / 255.0;
        let s = if max <= 0.0 { 0.0 } else { (max - min) / max };
        (s, max)
    }
}

/// Defines [`Palette`] once so field names and file keys never drift.
macro_rules! palette {
    ($($(#[doc = $doc:literal])* $field:ident),* $(,)?) => {
        /// Every semantic color a theme can set (file keys are the
        /// field names).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Palette {
            $($(#[doc = $doc])* pub $field: Rgba,)*
        }

        impl Palette {
            /// File keys, in declaration order.
            pub const KEYS: &'static [&'static str] = &[$(stringify!($field)),*];

            /// Color for a file key.
            pub fn get(&self, key: &str) -> Option<Rgba> {
                match key {
                    $(stringify!($field) => Some(self.$field),)*
                    _ => None,
                }
            }

            fn slot(&mut self, key: &str) -> Option<&mut Rgba> {
                match key {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

palette! {
    /// App background, page veil end.
    window,
    /// Sidebars, drawers, player bar.
    panel,
    /// Cards, Now Playing sheet.
    surface,
    /// Card and row hover.
    surface_hover,
    /// Active nav row, focused field.
    surface_active,
    /// Recessed shell (segmented pickers).
    inset,
    /// Text inputs, secondary buttons at rest.
    field,
    /// Secondary buttons hovered.
    field_hover,
    /// Popup menus.
    menu,
    /// Borders, slider rails, seek and EQ tracks.
    outline,
    /// Focused or hovered borders.
    outline_strong,
    /// Primary text and icons.
    text,
    /// Text with no explicit color (labels, body copy).
    text_body,
    /// Secondary text and idle icons.
    text_muted,
    /// Disabled entries.
    text_disabled,
    /// Input placeholders.
    placeholder,
    /// Active controls, highlights, the live song.
    accent,
    /// Accent under the pointer.
    accent_hover,
    /// Glyphs and text drawn on the accent.
    on_accent,
    /// Slider, toggle and seek knobs.
    handle,
    /// Knob outline (transparent when the knob already stands out).
    handle_edge,
    /// Modal and drawer dim.
    scrim,
    /// Drop shadows (alpha set per use).
    shadow,
}

impl Palette {
    /// The default look: dark surfaces, vivid red accent.
    pub const DARK: Palette = Palette {
        window: Rgba::rgb(0x0A, 0x0A, 0x0A),
        panel: Rgba::rgb(0x12, 0x12, 0x12),
        surface: Rgba::rgb(0x18, 0x18, 0x18),
        surface_hover: Rgba::rgb(0x28, 0x28, 0x28),
        surface_active: Rgba::rgb(0x2A, 0x2A, 0x2A),
        inset: Rgba::rgb(0x16, 0x16, 0x16),
        field: Rgba::rgb(0x22, 0x22, 0x22),
        field_hover: Rgba::rgb(0x33, 0x33, 0x33),
        menu: Rgba::rgb(0x24, 0x24, 0x24),
        outline: Rgba::rgb(0x3A, 0x3A, 0x3A),
        outline_strong: Rgba::rgb(0x6E, 0x6E, 0x6E),
        text: Rgba::WHITE,
        text_body: Rgba::rgb(0xE6, 0xE6, 0xE6),
        text_muted: Rgba::rgb(0xB3, 0xB3, 0xB3),
        text_disabled: Rgba::rgb(0x6E, 0x6E, 0x6E),
        placeholder: Rgba::rgb(0xDD, 0xDD, 0xDD),
        accent: Rgba::rgb(0xFF, 0x00, 0x33),
        accent_hover: Rgba::rgb(0xFF, 0x33, 0x66),
        on_accent: Rgba::WHITE,
        handle: Rgba::WHITE,
        handle_edge: Rgba::new(0x00, 0x00, 0x00, 0x00),
        scrim: Rgba::new(0x00, 0x00, 0x00, 0x8C),
        shadow: Rgba::BLACK,
    };

    /// Light base: white cards on a soft gray window, dark text, a
    /// slightly deeper red so the accent keeps contrast on white.
    pub const LIGHT: Palette = Palette {
        window: Rgba::rgb(0xF0, 0xF0, 0xF2),
        panel: Rgba::rgb(0xFA, 0xFA, 0xFB),
        surface: Rgba::rgb(0xFF, 0xFF, 0xFF),
        surface_hover: Rgba::rgb(0xEC, 0xEC, 0xEE),
        surface_active: Rgba::rgb(0xE4, 0xE4, 0xE7),
        inset: Rgba::rgb(0xF3, 0xF3, 0xF5),
        field: Rgba::rgb(0xF2, 0xF2, 0xF4),
        field_hover: Rgba::rgb(0xE6, 0xE6, 0xE9),
        menu: Rgba::rgb(0xFF, 0xFF, 0xFF),
        outline: Rgba::rgb(0xD2, 0xD2, 0xD6),
        outline_strong: Rgba::rgb(0x9A, 0x9A, 0xA0),
        text: Rgba::rgb(0x12, 0x12, 0x14),
        text_body: Rgba::rgb(0x1E, 0x1E, 0x22),
        text_muted: Rgba::rgb(0x5C, 0x5C, 0x63),
        text_disabled: Rgba::rgb(0xA8, 0xA8, 0xAE),
        placeholder: Rgba::rgb(0x6E, 0x6E, 0x75),
        accent: Rgba::rgb(0xE0, 0x00, 0x2D),
        accent_hover: Rgba::rgb(0xFF, 0x00, 0x33),
        on_accent: Rgba::WHITE,
        handle: Rgba::WHITE,
        handle_edge: Rgba::rgb(0x9A, 0x9A, 0xA0),
        scrim: Rgba::new(0x00, 0x00, 0x00, 0x59),
        shadow: Rgba::new(0x00, 0x00, 0x00, 0x80),
    };
}

/// Which built-in palette a theme starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Base {
    #[default]
    Dark,
    Light,
}

impl Base {
    pub fn palette(self) -> Palette {
        match self {
            Base::Dark => Palette::DARK,
            Base::Light => Palette::LIGHT,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Base::Dark => "dark",
            Base::Light => "light",
        }
    }
}

/// Theme file syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Toml,
    /// Spotifast-compatible JSON (`base` + `colors`).
    Json,
}

impl Format {
    /// Format from a file extension (`.toml` / `.json`).
    pub fn from_path(path: &Path) -> Option<Format> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "toml" => Some(Format::Toml),
            "json" => Some(Format::Json),
            _ => None,
        }
    }
}

/// Why a theme file was rejected (shown to users, so plain language).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("file is larger than 64 KiB")]
    TooLarge,
    #[error("not valid {0}: {1}")]
    Syntax(&'static str, String),
    #[error("base must be \"dark\" or \"light\", not \"{0}\"")]
    BadBase(String),
    #[error("color \"{key}\" is not a hex color: \"{value}\"")]
    BadColor { key: String, value: String },
    #[error("unsupported file type (use .toml or .json)")]
    BadExtension,
    #[error("could not read the file: {0}")]
    Io(String),
}

/// A parsed, fully resolved theme.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Stable id (file stem for user themes).
    pub id: String,
    /// Display name (`name` key, else the id).
    pub name: String,
    pub base: Base,
    pub palette: Palette,
    /// Tint the accent from the playing song's cover.
    pub accent_from_art: bool,
    /// Non-fatal problems (unknown keys, replaced colors).
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ThemeFile {
    name: Option<String>,
    base: Option<String>,
    accent_from_art: Option<bool>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// Spotifast key names mapped onto ours.
fn alias(key: &str) -> &str {
    match key {
        "secondary" => "text_muted",
        "dim" => "text_disabled",
        "overlay" => "scrim",
        other => other,
    }
}

/// Keys other apps' theme files carry that we accept and ignore.
const IGNORED_KEYS: &[&str] = &["danger", "warning", "tint"];

/// Derivation rules: `(key, sources, derive)`, in dependency order. A
/// key the author did not set is re-derived when any of its sources
/// changed from the base (set, or derived itself), so a short theme
/// gets coherent hovers and borders, and an unchanged base stays
/// byte-identical to its built-in palette.
type Rule = (
    &'static str,
    &'static [&'static str],
    fn(&Palette, Base) -> Rgba,
);

const RULES: &[Rule] = &[
    ("surface", &["window", "text"], |p, _| {
        p.window.mix(p.text, 0.04)
    }),
    ("panel", &["window", "surface"], |p, _| {
        p.window.mix(p.surface, 0.5)
    }),
    ("inset", &["window", "surface"], |p, _| {
        p.surface.mix(p.window, 0.25)
    }),
    ("surface_hover", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.065)
    }),
    ("surface_active", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.075)
    }),
    ("field", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.04)
    }),
    ("field_hover", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.11)
    }),
    ("menu", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.05)
    }),
    ("outline", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.15)
    }),
    ("outline_strong", &["surface", "text"], |p, _| {
        p.surface.mix(p.text, 0.38)
    }),
    ("text_body", &["text", "surface"], |p, _| {
        p.text.mix(p.surface, 0.1)
    }),
    ("text_muted", &["text", "surface"], |p, _| {
        p.text.mix(p.surface, 0.3)
    }),
    ("text_disabled", &["text", "surface"], |p, _| {
        p.text.mix(p.surface, 0.6)
    }),
    ("placeholder", &["text", "surface"], |p, _| {
        p.text.mix(p.surface, 0.15)
    }),
    ("accent_hover", &["accent"], |p, base| match base {
        Base::Dark => p.accent.mix(Rgba::WHITE, 0.2),
        Base::Light => p.accent.mix(Rgba::BLACK, 0.15),
    }),
    ("on_accent", &["accent"], |p, _| p.accent.readable_glyph()),
    (
        "handle_edge",
        &["handle", "panel", "outline_strong"],
        |p, _| knob_edge(p.handle, p.panel, p.outline_strong),
    ),
];

/// Knob outline: none when the knob reads on the panel by itself
/// (3:1, the WCAG bar for controls), else the strong outline.
fn knob_edge(handle: Rgba, panel: Rgba, outline_strong: Rgba) -> Rgba {
    if handle.contrast(panel) >= 3.0 {
        Rgba::new(0, 0, 0, 0)
    } else {
        outline_strong
    }
}

/// Minimum contrast per text role: `(key, ratio, backgrounds)`, a key
/// may have several rows. WCAG levels: 7:1 (AAA) for primary text on
/// the reading surfaces (cards, panels), 4.5:1 (AA) for it elsewhere
/// and for secondary text and placeholders, 3:1 for the accent
/// (controls, large text), a floor for disabled text (meant to look
/// faint, not vanish).
const CONTRAST_RULES: &[(&str, f32, &[&str])] = &[
    ("text", 7.0, &["panel", "surface"]),
    ("text", 4.5, &["window", "surface_hover", "field"]),
    ("text_body", 7.0, &["panel", "surface"]),
    ("text_body", 4.5, &["window", "surface_hover", "field"]),
    ("text_muted", 4.5, &["window", "panel", "surface"]),
    ("placeholder", 4.5, &["field"]),
    ("text_disabled", 2.0, &["menu"]),
    ("accent", 3.0, &["window", "panel", "surface"]),
];

/// Parse and resolve a theme file's text.
pub fn parse(id: &str, text: &str, format: Format) -> Result<Theme, ThemeError> {
    if text.len() > MAX_THEME_BYTES {
        return Err(ThemeError::TooLarge);
    }
    let file: ThemeFile = match format {
        Format::Toml => {
            toml::from_str(text).map_err(|e| ThemeError::Syntax("TOML", e.to_string()))?
        }
        Format::Json => {
            serde_json::from_str(text).map_err(|e| ThemeError::Syntax("JSON", e.to_string()))?
        }
    };
    let base = match file.base.as_deref().map(str::trim) {
        None => Base::Dark,
        Some(b) if b.eq_ignore_ascii_case("dark") => Base::Dark,
        Some(b) if b.eq_ignore_ascii_case("light") => Base::Light,
        Some(other) => return Err(ThemeError::BadBase(other.to_string())),
    };
    let mut palette = base.palette();
    let mut warnings = Vec::new();
    let mut set: Vec<&'static str> = Vec::new();
    for (raw_key, value) in &file.colors {
        let key = alias(raw_key);
        if IGNORED_KEYS.contains(&key) {
            continue;
        }
        let Some(&canonical) = Palette::KEYS.iter().find(|k| **k == key) else {
            warnings.push(format!("unknown color \"{raw_key}\" ignored"));
            continue;
        };
        let color = Rgba::parse(value).ok_or_else(|| ThemeError::BadColor {
            key: raw_key.clone(),
            value: value.clone(),
        })?;
        if let Some(slot) = palette.slot(canonical) {
            *slot = color;
        }
        set.push(canonical);
    }
    let mut changed = set.clone();
    for (key, sources, derive) in RULES {
        if set.contains(key) || !sources.iter().any(|s| changed.contains(s)) {
            continue;
        }
        let value = derive(&palette, base);
        if let Some(slot) = palette.slot(key) {
            *slot = value;
        }
        changed.push(key);
    }
    // Readability floor: text roles move toward white or black until
    // they read on every surface they sit on (a hand-set color that
    // needed it gets a warning, derived ones are fixed quietly).
    for (key, min, against) in CONTRAST_RULES {
        let backgrounds: Vec<Rgba> = against.iter().filter_map(|b| palette.get(b)).collect();
        let Some(color) = palette.get(key) else {
            continue;
        };
        let fixed = color.ensure_contrast(&backgrounds, *min);
        if fixed != color {
            if set.contains(key) {
                warnings.push(format!(
                    "{key} {} is hard to read, using {}",
                    color.to_hex(),
                    fixed.to_hex()
                ));
            }
            if let Some(slot) = palette.slot(key) {
                *slot = fixed;
            }
        }
    }
    // The accent may have moved: hover follows it unless hand-set.
    if !set.contains(&"accent_hover") && palette.accent != base.palette().accent {
        palette.accent_hover = match base {
            Base::Dark => palette.accent.mix(Rgba::WHITE, 0.2),
            Base::Light => palette.accent.mix(Rgba::BLACK, 0.15),
        };
    }
    if !set.contains(&"handle_edge") {
        palette.handle_edge = knob_edge(palette.handle, palette.panel, palette.outline_strong);
    }
    if palette.on_accent.contrast(palette.accent) < MIN_ON_ACCENT_CONTRAST {
        let fixed = palette.accent.readable_glyph();
        if fixed != palette.on_accent {
            if set.contains(&"on_accent") {
                warnings.push(format!(
                    "on_accent {} is hard to read on accent {}, using {}",
                    palette.on_accent.to_hex(),
                    palette.accent.to_hex(),
                    fixed.to_hex()
                ));
            }
            palette.on_accent = fixed;
        }
    }
    let name = file
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| id.to_string());
    Ok(Theme {
        id: id.to_string(),
        name,
        base,
        palette,
        accent_from_art: file.accent_from_art.unwrap_or(false),
        warnings,
    })
}

/// Read and parse one theme file (id = file stem).
pub fn load_file(path: &Path) -> Result<Theme, ThemeError> {
    let format = Format::from_path(path).ok_or(ThemeError::BadExtension)?;
    let meta = std::fs::metadata(path).map_err(|e| ThemeError::Io(e.to_string()))?;
    if meta.len() > MAX_THEME_BYTES as u64 {
        return Err(ThemeError::TooLarge);
    }
    let text = std::fs::read_to_string(path).map_err(|e| ThemeError::Io(e.to_string()))?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("theme")
        .to_string();
    parse(&id, &text, format)
}

/// A theme file that could not be used, for the Settings list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadFailure {
    pub file: String,
    pub reason: String,
}

/// Scan a themes folder: every `.toml` / `.json` regular file directly
/// inside (no symlinks, no subfolders), at most [`MAX_THEME_FILES`],
/// sorted by file name. A missing folder is simply empty.
pub fn load_dir(dir: &Path) -> (Vec<Theme>, Vec<LoadFailure>) {
    let mut themes = Vec::new();
    let mut failures = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (themes, failures);
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| Format::from_path(p).is_some())
        .collect();
    paths.sort();
    if paths.len() > MAX_THEME_FILES {
        failures.push(LoadFailure {
            file: format!("{} more files", paths.len() - MAX_THEME_FILES),
            reason: format!("only the first {MAX_THEME_FILES} themes are read"),
        });
        paths.truncate(MAX_THEME_FILES);
    }
    for path in paths {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match load_file(&path) {
            Ok(theme) => themes.push(theme),
            Err(e) => failures.push(LoadFailure {
                file,
                reason: e.to_string(),
            }),
        }
    }
    (themes, failures)
}

/// A theme id safe as a file stem: lowercase ascii letters, digits and
/// dashes (used when importing a file under a new name).
pub fn sanitize_id(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "theme".to_string()
    } else {
        out
    }
}

/// The light and dark tones of blurred cover art (RGBA pixels): the
/// 80th and 20th luminance percentiles, i.e. the brightest and darkest
/// areas text may land on, ignoring a few outliers. `None` when empty.
pub fn art_tones(rgba: &[u8]) -> Option<(Rgba, Rgba)> {
    let mut px: Vec<Rgba> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| Rgba::rgb(p[0], p[1], p[2]))
        .collect();
    if px.is_empty() {
        return None;
    }
    px.sort_by(|a, b| a.luminance().total_cmp(&b.luminance()));
    let at = |q: f32| px[((px.len() - 1) as f32 * q).round() as usize];
    Some((at(0.8), at(0.2)))
}

/// Weakest veil (0..1: how much of `surface` is painted over the art)
/// that keeps `text` at `min` contrast over art of color `tone` drawn
/// at `art_opacity` on `surface`. 0 when the art never threatens it.
pub fn veil_floor(text: Rgba, surface: Rgba, tone: Rgba, art_opacity: f32, min: f32) -> f32 {
    let bg = |veil: f32| surface.mix(tone, art_opacity * (1.0 - veil));
    if text.contrast(bg(0.0)) >= min {
        return 0.0;
    }
    if text.contrast(bg(1.0)) < min {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        if text.contrast(bg(mid)) >= min {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// Pick an accent from cover art pixels (RGBA, any size): the most
/// saturated, reasonably bright color, weighted by how common its hue
/// is. `None` for gray or empty art. The result is nudged until it has
/// at least 3:1 contrast against `window`, so it always reads.
pub fn accent_from_pixels(rgba: &[u8], window: Rgba) -> Option<Rgba> {
    const BINS: usize = 24;
    let mut weight = [0f32; BINS];
    let mut sum = [[0f32; 3]; BINS];
    for px in rgba.as_chunks::<4>().0 {
        if px[3] < 128 {
            continue;
        }
        let c = Rgba::rgb(px[0], px[1], px[2]);
        let (s, v) = c.saturation_value();
        if s < 0.25 || v < 0.2 {
            continue;
        }
        let hue = hue_degrees(c);
        let bin = ((hue / 360.0) * BINS as f32) as usize % BINS;
        let w = s * s * v;
        weight[bin] += w;
        sum[bin][0] += px[0] as f32 * w;
        sum[bin][1] += px[1] as f32 * w;
        sum[bin][2] += px[2] as f32 * w;
    }
    let (best, &w) = weight
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if w <= 0.0 {
        return None;
    }
    let mut c = Rgba::rgb(
        (sum[best][0] / w).round() as u8,
        (sum[best][1] / w).round() as u8,
        (sum[best][2] / w).round() as u8,
    );
    // Push toward full brightness/saturation so a muddy average still
    // reads as an accent, then away from the window until it contrasts.
    let (s, v) = c.saturation_value();
    if v < 0.75 || s < 0.5 {
        c = vivid(c);
    }
    let toward = if window.luminance() < 0.4 {
        Rgba::WHITE
    } else {
        Rgba::BLACK
    };
    for _ in 0..10 {
        if c.contrast(window) >= 3.0 {
            break;
        }
        c = c.mix(toward, 0.12);
    }
    Some(c)
}

fn hue_degrees(c: Rgba) -> f32 {
    let (r, g, b) = (c.r as f32, c.g as f32, c.b as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    if d <= 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h * 60.0
}

/// Same hue at saturation >= 0.65 and value >= 0.85.
fn vivid(c: Rgba) -> Rgba {
    let (s, v) = c.saturation_value();
    let (s, v) = (s.max(0.65), v.max(0.85));
    let h = hue_degrees(c) / 60.0;
    let chroma = v * s;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = v - chroma;
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba::rgb(to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_forms_parse_and_round_trip() {
        assert_eq!(Rgba::parse("#FF0033"), Some(Rgba::rgb(0xFF, 0, 0x33)));
        assert_eq!(Rgba::parse("ff0033"), Some(Rgba::rgb(0xFF, 0, 0x33)));
        assert_eq!(Rgba::parse("#f03"), Some(Rgba::rgb(0xFF, 0, 0x33)));
        assert_eq!(Rgba::parse("#0000008C"), Some(Rgba::new(0, 0, 0, 0x8C)));
        assert_eq!(Rgba::parse("#12345"), None);
        assert_eq!(Rgba::parse("#GG0000"), None);
        assert_eq!(Rgba::parse("red"), None);
        assert_eq!(Rgba::new(0, 0, 0, 0x8C).to_hex(), "#0000008C");
        assert_eq!(Rgba::rgb(0xFF, 0, 0x33).to_hex(), "#FF0033");
    }

    #[test]
    fn empty_theme_is_its_base_exactly() {
        let dark = parse("x", "name = \"X\"", Format::Toml).unwrap();
        assert_eq!(dark.palette, Palette::DARK);
        assert!(dark.warnings.is_empty(), "{:?}", dark.warnings);
        let light = parse("x", "base = \"light\"", Format::Toml).unwrap();
        assert_eq!(light.palette, Palette::LIGHT);
        assert_eq!(light.name, "x");
    }

    #[test]
    fn base_palettes_keep_readable_pairs() {
        for p in [Palette::DARK, Palette::LIGHT] {
            assert!(p.on_accent.contrast(p.accent) >= MIN_ON_ACCENT_CONTRAST);
            assert!(p.text.contrast(p.surface) >= 7.0);
            assert!(p.text_muted.contrast(p.surface) >= 4.5);
        }
    }

    #[test]
    fn short_theme_derives_matching_colors() {
        let t = parse(
            "nord",
            "[colors]\nwindow = \"#2E3440\"\ntext = \"#ECEFF4\"\naccent = \"#88C0D0\"",
            Format::Toml,
        )
        .unwrap();
        let p = t.palette;
        assert_eq!(p.window, Rgba::rgb(0x2E, 0x34, 0x40));
        // Surfaces follow the new window, not the base dark grays.
        assert_ne!(p.surface, Palette::DARK.surface);
        assert_ne!(p.panel, Palette::DARK.panel);
        assert_ne!(p.surface_hover, Palette::DARK.surface_hover);
        assert_ne!(p.accent_hover, Palette::DARK.accent_hover);
        // A pale accent gets dark glyphs.
        assert_eq!(p.on_accent, Rgba::BLACK);
        // Untouched roots stay at the base.
        assert_eq!(p.scrim, Palette::DARK.scrim);
        assert_eq!(p.handle, Palette::DARK.handle);
    }

    #[test]
    fn explicit_keys_win_over_derivation() {
        let t = parse(
            "t",
            "[colors]\nsurface = \"#202020\"\nsurface_hover = \"#ABCDEF\"",
            Format::Toml,
        )
        .unwrap();
        assert_eq!(t.palette.surface_hover, Rgba::rgb(0xAB, 0xCD, 0xEF));
        assert_ne!(t.palette.outline, Palette::DARK.outline);
    }

    #[test]
    fn unreadable_on_accent_is_replaced_with_a_warning() {
        let t = parse(
            "t",
            "[colors]\naccent = \"#FFE066\"\non_accent = \"#FFFFFF\"",
            Format::Toml,
        )
        .unwrap();
        assert_eq!(t.palette.on_accent, Rgba::BLACK);
        assert!(t.warnings.iter().any(|w| w.contains("on_accent")));
    }

    #[test]
    fn unknown_keys_warn_and_bad_values_fail() {
        let t = parse("t", "[colors]\nsparkle = \"#FFFFFF\"", Format::Toml).unwrap();
        assert_eq!(t.warnings.len(), 1);
        assert!(matches!(
            parse("t", "[colors]\naccent = \"blue\"", Format::Toml),
            Err(ThemeError::BadColor { .. })
        ));
        assert!(matches!(
            parse("t", "base = \"sepia\"", Format::Toml),
            Err(ThemeError::BadBase(_))
        ));
        assert!(matches!(
            parse("t", "colors = [", Format::Toml),
            Err(ThemeError::Syntax("TOML", _))
        ));
        let big = format!("name = \"{}\"", "x".repeat(MAX_THEME_BYTES));
        assert_eq!(parse("t", &big, Format::Toml), Err(ThemeError::TooLarge));
    }

    #[test]
    fn spotifast_json_maps_its_keys() {
        let json = r##"{"base":"light","colors":{"accent":"#1DB954","secondary":"#555555",
            "dim":"#999999","overlay":"#00000066","danger":"#FF0000","tint":"#11223344"}}"##;
        let t = parse("green", json, Format::Json).unwrap();
        assert_eq!(t.base, Base::Light);
        // Spotify green is too light for a light window: darkened to
        // 3:1, same hue family, with a warning.
        let green = Rgba::rgb(0x1D, 0xB9, 0x54);
        assert_ne!(t.palette.accent, green);
        assert!(t.palette.accent.g > t.palette.accent.r);
        assert!(t.palette.accent.contrast(t.palette.window) >= 3.0);
        assert_eq!(t.palette.text_muted, Rgba::rgb(0x55, 0x55, 0x55));
        assert_eq!(t.palette.text_disabled, Rgba::rgb(0x99, 0x99, 0x99));
        assert_eq!(t.palette.scrim, Rgba::new(0, 0, 0, 0x66));
        assert_eq!(t.warnings.len(), 1, "{:?}", t.warnings);
        assert!(t.warnings[0].starts_with("accent"));
        assert_eq!(t.name, "green");
    }

    #[test]
    fn accent_from_art_flag_parses() {
        let t = parse("t", "accent_from_art = true", Format::Toml).unwrap();
        assert!(t.accent_from_art);
    }

    #[test]
    fn load_dir_reads_valid_files_and_lists_failures() {
        let dir = std::env::temp_dir().join(format!("yutu-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.toml"), "name = \"Bee\"").unwrap();
        std::fs::write(dir.join("a.json"), r#"{"base":"light"}"#).unwrap();
        std::fs::write(dir.join("bad.toml"), "base = 3").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        std::fs::write(dir.join("sub").join("c.toml"), "").unwrap();
        let (themes, failures) = load_dir(&dir);
        let ids: Vec<_> = themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(themes[1].name, "Bee");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].file, "bad.toml");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_dir(&dir).0.is_empty());
    }

    #[test]
    fn sanitize_id_makes_safe_stems() {
        assert_eq!(sanitize_id("My Cool Theme!"), "my-cool-theme");
        assert_eq!(sanitize_id("../../etc"), "etc");
        assert_eq!(sanitize_id("***"), "theme");
    }

    #[test]
    fn art_accent_picks_the_dominant_vivid_hue() {
        // Mostly gray art with a strong blue patch.
        let mut px = Vec::new();
        for i in 0..64 {
            if i < 20 {
                px.extend_from_slice(&[0x20, 0x40, 0xE0, 0xFF]);
            } else {
                px.extend_from_slice(&[0x80, 0x80, 0x80, 0xFF]);
            }
        }
        let c = accent_from_pixels(&px, Palette::DARK.window).unwrap();
        assert!(c.b > c.r && c.b > c.g, "{c:?}");
        assert!(c.contrast(Palette::DARK.window) >= 3.0);
        // All gray: no accent.
        let gray: Vec<u8> = [0x77u8, 0x77, 0x77, 0xFF].repeat(64);
        assert_eq!(accent_from_pixels(&gray, Palette::DARK.window), None);
        assert_eq!(accent_from_pixels(&[], Palette::DARK.window), None);
    }

    #[test]
    fn ensure_contrast_moves_only_what_needs_it() {
        let dark = [Rgba::rgb(0x1E, 0x1E, 0x2E)];
        // Already readable: untouched.
        assert_eq!(Rgba::WHITE.ensure_contrast(&dark, 7.0), Rgba::WHITE);
        // Too dim: lifted just enough, toward white.
        let dim = Rgba::rgb(0x58, 0x5B, 0x70);
        let fixed = dim.ensure_contrast(&dark, 4.5);
        assert!(fixed.contrast(dark[0]) >= 4.5);
        assert!(fixed.contrast(dark[0]) < 5.0, "minimal move");
        assert!(fixed.luminance() > dim.luminance());
        // On light backgrounds it darkens instead.
        let light = [Rgba::WHITE, Rgba::rgb(0xEC, 0xEC, 0xEE)];
        let pale = Rgba::rgb(0x99, 0x99, 0x99);
        let fixed = pale.ensure_contrast(&light, 4.5);
        assert!(light.iter().all(|b| fixed.contrast(*b) >= 4.5));
        assert!(fixed.luminance() < pale.luminance());
    }

    #[test]
    fn low_contrast_text_is_fixed_and_warned() {
        let t = parse(
            "t",
            "[colors]\nsurface = \"#1E1E2E\"\ntext_muted = \"#45475A\"",
            Format::Toml,
        )
        .unwrap();
        assert!(t.palette.text_muted.contrast(t.palette.surface) >= 4.5);
        assert!(t.warnings.iter().any(|w| w.contains("text_muted")));
        // Derived roles are fixed without a warning.
        let t = parse("t", "[colors]\ntext = \"#9090A0\"", Format::Toml).unwrap();
        for key in ["text", "text_body"] {
            let c = t.palette.get(key).unwrap();
            for bg in ["panel", "surface"] {
                assert!(
                    c.contrast(t.palette.get(bg).unwrap()) >= 7.0,
                    "{key} on {bg}"
                );
            }
            for bg in ["window", "surface_hover", "field"] {
                assert!(
                    c.contrast(t.palette.get(bg).unwrap()) >= 4.5,
                    "{key} on {bg}"
                );
            }
        }
    }

    #[test]
    fn knob_gets_an_edge_only_when_it_blends_in() {
        assert_eq!(Palette::DARK.handle_edge.a, 0);
        assert_eq!(Palette::LIGHT.handle_edge, Palette::LIGHT.outline_strong);
        let t = parse("t", "[colors]\nhandle = \"#111111\"", Format::Toml).unwrap();
        assert_eq!(t.palette.handle_edge, Palette::DARK.outline_strong);
    }

    #[test]
    fn veil_floor_rises_with_bright_art() {
        let (text, surface) = (Rgba::WHITE, Palette::DARK.surface);
        let dark_art = Rgba::rgb(0x20, 0x10, 0x30);
        assert_eq!(veil_floor(text, surface, dark_art, 0.7, 7.0), 0.0);
        let bright_art = Rgba::rgb(0xE8, 0xE8, 0xE8);
        let v = veil_floor(text, surface, bright_art, 0.7, 7.0);
        assert!(v > 0.3 && v < 1.0, "{v}");
        let bg = surface.mix(bright_art, 0.7 * (1.0 - v));
        assert!(text.contrast(bg) >= 7.0);
        let tones = art_tones(&[[0u8, 0, 0, 255], [255, 255, 255, 255]].concat()).unwrap();
        assert_eq!(tones, (Rgba::WHITE, Rgba::BLACK));
        assert_eq!(art_tones(&[]), None);
    }

    #[test]
    fn art_accent_reads_on_light_windows() {
        let px: Vec<u8> = [0xFF, 0xF0, 0x40, 0xFF].repeat(16);
        let c = accent_from_pixels(&px, Palette::LIGHT.window).unwrap();
        assert!(c.contrast(Palette::LIGHT.window) >= 3.0, "{c:?}");
    }
}
