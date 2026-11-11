//! yutu-ui fonts: split from lib.rs (no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use pointing_slider::pointing_slider;
#[allow(unused_imports)]
use row_button::row_button;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use yutu_core::{
    forgotten_favorites, history_section, should_scrobble, weekly_mix, Album, Artist, HistoryEntry,
    HistorySection, LibraryStore, LyricsProvider, LyricsResult, Playlist, PlaylistHit,
    QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

/// Display font (Settings Appearance, persisted pref `font_family`):
/// bundled Adwaita Sans by default so Linux and macOS render the same
/// outlines; System keeps the old platform-sans behavior; Custom is
/// any installed family picked from the system list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Adwaita,
    System,
    /// An installed family by name (interned, see [`intern_font_name`]).
    Custom(&'static str),
}

impl FontFamily {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Adwaita => "Adwaita Sans (bundled)",
            Self::System => "System default",
            Self::Custom(name) => name,
        }
    }

    pub(crate) fn stored(self) -> String {
        match self {
            Self::Adwaita => "adwaita".to_string(),
            Self::System => "system".to_string(),
            Self::Custom(name) => format!("name:{name}"),
        }
    }

    pub(crate) fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("system") {
            Self::System
        } else if raw.len() >= 5 && raw[..5].eq_ignore_ascii_case("name:") {
            // The prefix is ASCII, so byte index 5 is a boundary.
            if raw[5..].trim().is_empty() {
                Self::Adwaita
            } else {
                Self::Custom(intern_font_name(&raw[5..]))
            }
        } else {
            // "adwaita", legacy "inter", and junk all default to Adwaita.
            Self::Adwaita
        }
    }

    /// Map a picker row back (the two heads are special; every other
    /// row is a raw installed family name).
    pub(crate) fn from_label(label: &str) -> Self {
        if label == Self::Adwaita.label() {
            Self::Adwaita
        } else if label == Self::System.label() {
            Self::System
        } else if label.trim().is_empty() {
            Self::Adwaita
        } else {
            Self::Custom(intern_font_name(label))
        }
    }

    pub(crate) fn to_iced_font(self) -> iced::Font {
        match self {
            Self::Adwaita => iced::Font::with_name("Adwaita Sans"),
            Self::System => iced::Font::default(),
            Self::Custom(name) => iced::Font::with_name(name),
        }
    }
}

/// Interned font names: `Family::Name` needs `&'static str`, so a
/// picked name leaks exactly once (a handful per session at most,
/// never per frame). Lock-poisoning falls back to the inner map,
/// same as the icon cache.
pub(crate) static INTERNED_FONTS: std::sync::LazyLock<
    std::sync::Mutex<HashMap<String, &'static str>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub(crate) fn intern_font_name(raw: &str) -> &'static str {
    // Absurd lengths never name a real family: cap the leak (the
    // floor keeps the cut on a char boundary).
    let trimmed = raw.trim();
    let mut end = trimmed.len().min(256);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let short = &trimmed[..end];
    let mut cache = INTERNED_FONTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = cache.get(short) {
        return hit;
    }
    let leaked: &'static str = Box::leak(short.to_owned().into_boxed_str());
    cache.insert(short.to_owned(), leaked);
    leaked
}

/// Family names from a font database: first family per face,
/// deduplicated and sorted (pure helper, unit-tested with a
/// memory-loaded database).
pub(crate) fn family_names_of(db: &fontdb::Database) -> Vec<String> {
    let mut names: Vec<String> = db
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !name.trim().is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Installed system families, scanned once at startup on a
/// blocking thread (fontdb reads the OS font directories: the same
/// source iced resolves names through, so every row renders). The
/// window never waits for it: Settings shows the two pinned heads
/// until the list lands.
pub(crate) fn system_font_task() -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(|| {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                family_names_of(&db)
            })
            .await
            .unwrap_or_default()
        },
        Message::SystemFontsLoaded,
    )
}

/// Picker rows: the two pinned heads plus installed families (an
/// installed "Adwaita Sans", if any, is covered by the bundled head).
pub(crate) fn font_option_rows(system: &[String]) -> Vec<String> {
    let mut options = vec![
        FontFamily::Adwaita.label().to_string(),
        FontFamily::System.label().to_string(),
    ];
    options.extend(
        system
            .iter()
            .filter(|name| {
                name.as_str() != FontFamily::Adwaita.label()
                    && name.as_str() != FontFamily::System.label()
                    && name.as_str() != "Adwaita Sans"
            })
            .cloned(),
    );
    options
}

/// True when the picked custom family is gone from the installed
/// list (bundled Adwaita Sans always counts as present; an empty list means
/// the startup scan has not landed yet, not that fonts are missing).
pub(crate) fn is_font_missing(family: FontFamily, system: &[String]) -> bool {
    match family {
        FontFamily::Custom(name) => {
            name != "Adwaita Sans" && !system.is_empty() && !system.iter().any(|n| n == name)
        }
        FontFamily::Adwaita | FontFamily::System => false,
    }
}

/// Startup-effective display family. Iced fixes the default font at
/// launch, so the Settings pick writes the pref and takes effect on
/// relaunch; button and bold fonts read this (not live state) so a
/// pending pick never mixes two families in one session.
pub(crate) static EFFECTIVE_FONT: std::sync::OnceLock<FontFamily> = std::sync::OnceLock::new();

pub(crate) fn effective_font() -> FontFamily {
    *EFFECTIVE_FONT.get_or_init(|| FontFamily::Adwaita)
}

/// True when the pick differs from the startup-effective family
/// (the renderer font is fixed at launch, so only a relaunch can
/// apply it). Single rule behind both the status note and the
/// relaunch button below.
pub(crate) fn font_pending(picked: FontFamily, effective: FontFamily) -> bool {
    picked != effective
}

/// Status line for a font pick: `None` when the pick already runs
/// (the picker selection confirms it, so the status keeps showing any
/// live failure instead of being erased by a no-op); otherwise the
/// relaunch note. Pure in both args so tests never touch the
/// process-wide [`EFFECTIVE_FONT`].
pub(crate) fn font_pick_status(picked: FontFamily, effective: FontFamily) -> Option<String> {
    if font_pending(picked, effective) {
        Some(format!(
            "Display font: {} saved (relaunch to apply).",
            picked.label()
        ))
    } else {
        None
    }
}

/// UI scale bounds (Settings Appearance slider): the application
/// scale factor, so text and controls grow together and rows never
/// overflow. Persisted pref `ui_scale`, applies instantly.
pub const UI_SCALE_MIN: f32 = 0.8;
pub const UI_SCALE_MAX: f32 = 1.5;
pub const UI_SCALE_DEFAULT: f32 = 1.0;

/// Clamp a stored scale into range (non-finite falls back to 1.0:
/// a NaN must never reach the renderer).
pub(crate) fn clamp_scale(raw: f32) -> f32 {
    if raw.is_finite() {
        raw.clamp(UI_SCALE_MIN, UI_SCALE_MAX)
    } else {
        UI_SCALE_DEFAULT
    }
}

/// Button-weight font (one step above body): shared by button
/// labels and button rows so the weight cannot drift per site.
/// Family follows the startup-effective pick (see [`EFFECTIVE_FONT`]).
pub(crate) fn medium_font() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Medium,
        ..effective_font().to_iced_font()
    }
}

/// Bold-weight font: same family rule as [`medium_font`] (play and
/// shuffle pills plus the live lyric line used to pin SansSerif).
pub(crate) fn bold_font() -> iced::Font {
    iced::Font {
        weight: iced::font::Weight::Bold,
        ..effective_font().to_iced_font()
    }
}
