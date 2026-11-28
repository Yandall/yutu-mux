//! yutu-ui romaji: romanized lines under Japanese lyrics.
//!
//! Rules:
//! - One switch (`lyrics_romaji` pref): Settings toggle, the lyrics
//!   stepper's quick toggle and `ctl lyrics romaji` all dispatch
//!   `Message::SetLyricsRomaji`.
//! - The first enable downloads + builds the dictionary once (see
//!   `yutu_core::romaji`); later launches only load it from disk.
//! - Romaji is computed when lyrics land (or once the dictionary is
//!   ready), never in `view`: views read `romaji_line` lookups only.
//! - Only songs whose lyrics contain kana are romanized; lines with no
//!   Japanese characters get no second line.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use std::collections::HashMap;
#[allow(unused_imports)]
use yutu_core::{LibraryStore, LyricsProvider, LyricsResult};

/// Romanized lines kept in memory (line text keyed, shared by every
/// view). Past the cap the map rebuilds from the cached lyrics.
#[cfg(feature = "romaji")]
pub(crate) const ROMAJI_LINES_CAP: usize = 4000;

/// Dictionary setup progress (the Settings row reads it).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum RomajiSetup {
    #[default]
    Idle,
    /// Downloading/building (first enable) or loading (later launches).
    #[cfg_attr(not(feature = "romaji"), allow(dead_code))]
    Preparing,
    /// Setup failed: the toggle stays on, Settings offers a retry.
    Failed,
}

/// Where the built dictionary lives.
#[cfg(feature = "romaji")]
pub(crate) fn romaji_data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(storage::data_dir()).join("romaji")
}

/// Shared analyzer handle carried by `Message::RomajiReady`.
#[cfg(feature = "romaji")]
pub(crate) type RomanizerHandle = std::sync::Arc<yutu_core::romaji::Romanizer>;

/// Romaji for every Japanese line of `lyrics` missing from `lines`.
#[cfg(feature = "romaji")]
pub(crate) fn romanize_into(
    romanizer: &yutu_core::romaji::Romanizer,
    lyrics: &LyricsResult,
    lines: &mut HashMap<String, Option<String>>,
) {
    if lyrics.instrumental || !lyrics_are_japanese(lyrics) {
        return;
    }
    let synced = lyrics.synced.iter().map(|l| l.text.as_str());
    for line in synced.chain(lyrics.plain.lines()) {
        let line = line.trim();
        if line.is_empty() || lines.contains_key(line) {
            continue;
        }
        lines.insert(line.to_string(), romanizer.romanize_line(line));
    }
}

/// True when the lyrics contain kana anywhere (see
/// `yutu_core::romaji::is_japanese`).
pub(crate) fn lyrics_are_japanese(lyrics: &LyricsResult) -> bool {
    yutu_core::romaji::is_japanese(&lyrics.plain)
        || lyrics
            .synced
            .iter()
            .any(|l| yutu_core::romaji::is_japanese(&l.text))
}

impl AppState {
    /// Romaji to show under `line`, when the option is on and the line
    /// has one.
    pub(crate) fn romaji_line(&self, line: &str) -> Option<&str> {
        if !self.lyrics_romaji {
            return None;
        }
        self.romaji_lines.get(line.trim())?.as_deref()
    }

    /// True when the lyrics shown for `id` are Japanese (the stepper
    /// shows its romaji toggle only then).
    pub(crate) fn lyrics_shown_japanese(&self, id: &str) -> bool {
        self.lyrics_display_entry(id)
            .is_some_and(|(_, l)| !l.instrumental && lyrics_are_japanese(l))
    }

    /// Romanize one freshly cached entry (no-op without the analyzer).
    #[allow(unused_variables)]
    pub(crate) fn romanize_cached(&mut self, id: &str, provider: LyricsProvider) {
        #[cfg(feature = "romaji")]
        {
            let Some(romanizer) = self.romanizer.clone() else {
                return;
            };
            if self.romaji_lines.len() > ROMAJI_LINES_CAP {
                self.romanize_all();
                return;
            }
            if let Some(lyrics) = self.lyrics_cache.get(&(id.to_string(), provider)) {
                romanize_into(&romanizer, lyrics, &mut self.romaji_lines);
            }
        }
    }

    /// Rebuild the romaji map from every cached lyrics entry.
    #[cfg(feature = "romaji")]
    pub(crate) fn romanize_all(&mut self) {
        self.romaji_lines.clear();
        if let Some(romanizer) = self.romanizer.clone() {
            for lyrics in self.lyrics_cache.values() {
                romanize_into(&romanizer, lyrics, &mut self.romaji_lines);
            }
        }
    }

    /// Persist the switch.
    pub(crate) fn persist_lyrics_romaji(&self) {
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref("lyrics_romaji", if self.lyrics_romaji { "1" } else { "0" });
        }
    }

    /// Start dictionary setup when the option is on and no analyzer is
    /// loaded or in flight. Builds without the feature never set up.
    pub(crate) fn romaji_setup_task(&mut self) -> iced::Task<Message> {
        #[cfg(feature = "romaji")]
        {
            if !self.lyrics_romaji
                || self.romanizer.is_some()
                || self.romaji_setup == RomajiSetup::Preparing
            {
                return iced::Task::none();
            }
            self.romaji_setup = RomajiSetup::Preparing;
            let dir = romaji_data_dir();
            iced::Task::perform(
                async move {
                    yutu_core::romaji::ensure_dictionary(dir.clone()).await?;
                    tokio::task::spawn_blocking(move || {
                        yutu_core::romaji::Romanizer::load(&dir).map(std::sync::Arc::new)
                    })
                    .await
                    .map_err(|e| format!("romaji load failed: {e}"))?
                },
                Message::RomajiReady,
            )
        }
        #[cfg(not(feature = "romaji"))]
        iced::Task::none()
    }

    /// `Message::SetLyricsRomaji`: flip the switch, persist, set up the
    /// dictionary on first use.
    pub(crate) fn set_lyrics_romaji(&mut self, on: bool) -> iced::Task<Message> {
        if self.lyrics_romaji != on {
            self.lyrics_romaji = on;
            self.persist_lyrics_romaji();
            self.publish_mpris();
        }
        if on && self.romaji_setup == RomajiSetup::Failed {
            // Turning it on again is the retry.
            self.romaji_setup = RomajiSetup::Idle;
        }
        self.romaji_setup_task()
    }

    /// `Message::RomajiReady`: keep the analyzer and romanize whatever
    /// lyrics are already cached; failures log and show in Settings.
    #[cfg(feature = "romaji")]
    pub(crate) fn romaji_ready(&mut self, result: Result<RomanizerHandle, String>) {
        match result {
            Ok(romanizer) => {
                self.romanizer = Some(romanizer);
                self.romaji_setup = RomajiSetup::Idle;
                self.romanize_all();
            }
            Err(e) => {
                yutu_core::app_log::warn("romaji", &e);
                self.romaji_setup = RomajiSetup::Failed;
                #[cfg(feature = "debug-panel")]
                {
                    self.status = e;
                }
            }
        }
    }
}

/// Width of [`AppState::romaji_quick_toggle`] (the stepper mirrors it
/// with a spacer so the provider label stays centered).
pub(crate) const ROMAJI_TOGGLE_W: f32 = 26.0;

/// Romaji line size for a verse of `size` (smaller, never tiny).
pub(crate) fn romaji_size(size: f32) -> f32 {
    (size * 0.62).max(11.0)
}

impl AppState {
    /// Settings row: the switch plus its hint or setup status.
    pub(crate) fn romaji_settings(&self) -> iced::Element<'_, Message> {
        #[cfg(feature = "romaji")]
        {
            use iced::widget::{column, container, row, text, toggler, tooltip};
            let note = match self.romaji_setup {
                RomajiSetup::Preparing => self.language.lyrics_romaji_preparing(),
                RomajiSetup::Failed => self.language.lyrics_romaji_failed(),
                RomajiSetup::Idle => self.language.lyrics_romaji_hint(),
            };
            let note_color = if self.romaji_setup == RomajiSetup::Failed {
                tk().accent_hover
            } else {
                tk().text_muted
            };
            // Romaji is machine-made: the info tooltip says so, so a
            // wrong reading is no surprise.
            let info = tooltip(
                btn_icon("info", 14.0, tk().text_muted),
                container(
                    text(self.language.lyrics_romaji_tooltip())
                        .size(12)
                        .color(tk().text),
                )
                .style(floating_panel_bg)
                .padding([8, 10])
                .max_width(260),
                tooltip::Position::Top,
            )
            .gap(6);
            column![
                row![
                    toggler(self.lyrics_romaji)
                        .label(self.language.lyrics_romaji())
                        .on_toggle(Message::SetLyricsRomaji)
                        .style(red_toggle),
                    info,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
                text(note).size(12).color(note_color),
            ]
            .spacing(4)
            .into()
        }
        #[cfg(not(feature = "romaji"))]
        iced::widget::Space::new().into()
    }

    /// Lyrics stepper quick toggle (Japanese lyrics only): filled red
    /// while on, quiet outline while off.
    pub(crate) fn romaji_quick_toggle(&self) -> iced::Element<'_, Message> {
        use iced::widget::button;
        let on = self.lyrics_romaji;
        let glyph = btn_icon(
            "languages",
            14.0,
            if on { tk().text } else { tk().text_muted },
        );
        let b = button(ctrl_box(glyph))
            .on_press(Message::SetLyricsRomaji(!on))
            .padding(4);
        if on {
            b.style(accent_button).into()
        } else {
            b.style(icon_hover).into()
        }
    }

    /// One lyric verse with its romaji line beneath (when it has one).
    /// `main` is the styled original; the romaji follows its alpha.
    pub(crate) fn verse_with_romaji<'a>(
        &self,
        main: iced::widget::Text<'a>,
        raw: &str,
        size: f32,
        alpha: f32,
    ) -> iced::Element<'a, Message> {
        self.verse_with_romaji_in(main, raw, size, alpha, None)
    }

    /// [`Self::verse_with_romaji`] for a synced verse in a lyrics
    /// column `col_w` wide: the romaji is pre-broken like the verse
    /// (`AppState::lyric_romaji_lines`), so it never reflows mid-glide.
    pub(crate) fn verse_with_romaji_broken<'a>(
        &self,
        main: iced::widget::Text<'a>,
        raw: &str,
        size: f32,
        alpha: f32,
        col_w: f32,
    ) -> iced::Element<'a, Message> {
        self.verse_with_romaji_in(main, raw, size, alpha, Some(col_w))
    }

    fn verse_with_romaji_in<'a>(
        &self,
        main: iced::widget::Text<'a>,
        raw: &str,
        size: f32,
        alpha: f32,
        col_w: Option<f32>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, text};
        match self.romaji_line(raw) {
            Some(roma) => column![
                main,
                text(match col_w {
                    Some(w) => Self::lyric_romaji_lines(roma, w),
                    None => roma.to_string(),
                })
                .size(romaji_size(size))
                .color(tk().text.scale_alpha(alpha * 0.75))
                .align_x(iced::alignment::Horizontal::Center)
                .width(iced::Fill),
            ]
            .spacing(2)
            .width(iced::Fill)
            .into(),
            None => main.into(),
        }
    }
}
