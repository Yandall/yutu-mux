//! Theme catalog and switching: built-in themes (bundled TOML, the
//! same format users write), user themes from `<data>/themes/`, the
//! Follow system pair, and accent from album art. Parsing and color
//! math live in `yutu_core::theme`; this module only picks the active
//! theme and pushes it into the live tokens (`set_tokens`).

use super::*;
use dropdown::dropdown;
use yutu_core::theme::{self as core_theme, Base, Format, Rgba, Theme};
use yutu_core::LibraryStore;

/// Pref value for "follow the system light/dark setting".
pub(crate) const THEME_SYSTEM: &str = "system";
/// Default theme ids.
pub(crate) const THEME_DEFAULT: &str = "dark";
pub(crate) const THEME_DEFAULT_LIGHT: &str = "light";

/// Built-in themes in picker order: `(id, file)`.
const BUILTIN: &[(&str, &str)] = &[
    ("dark", include_str!("../assets/themes/dark.toml")),
    ("light", include_str!("../assets/themes/light.toml")),
    (
        "youtube-music",
        include_str!("../assets/themes/youtube-music.toml"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../assets/themes/catppuccin-mocha.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../assets/themes/catppuccin-latte.toml"),
    ),
    ("nord", include_str!("../assets/themes/nord.toml")),
    ("rose-pine", include_str!("../assets/themes/rose-pine.toml")),
    (
        "tokyo-night",
        include_str!("../assets/themes/tokyo-night.toml"),
    ),
    (
        "gruvbox-dark",
        include_str!("../assets/themes/gruvbox-dark.toml"),
    ),
];

/// Parsed built-ins (they are tested to parse cleanly).
pub(crate) fn builtin_themes() -> Vec<Theme> {
    BUILTIN
        .iter()
        .filter_map(|(id, text)| core_theme::parse(id, text, Format::Toml).ok())
        .collect()
}

/// Whether `id` names a built-in theme.
pub(crate) fn is_builtin(id: &str) -> bool {
    BUILTIN.iter().any(|(b, _)| *b == id)
}

/// User themes folder: `themes/` beside the library database (so a
/// test's temp library never reads the real folder).
pub(crate) fn themes_dir(library_path: &str) -> std::path::PathBuf {
    std::path::Path::new(library_path)
        .parent()
        .map(|p| p.join("themes"))
        .unwrap_or_else(|| std::path::PathBuf::from("themes"))
}

/// Built-ins followed by user themes; a user file with a built-in's id
/// replaces it in place (edit a copy of `nord.toml` to tweak Nord).
pub(crate) fn merge_themes(user: Vec<Theme>) -> Vec<Theme> {
    let mut all = builtin_themes();
    for theme in user {
        match all.iter_mut().find(|t| t.id == theme.id) {
            Some(slot) => *slot = theme,
            None => all.push(theme),
        }
    }
    all
}

/// Which theme id a pick resolves to (`system` follows the OS mode,
/// unknown modes count as dark).
pub(crate) fn resolve_pick<'a>(
    pick: &'a str,
    light: &'a str,
    dark: &'a str,
    system_light: bool,
) -> &'a str {
    if pick == THEME_SYSTEM {
        if system_light {
            light
        } else {
            dark
        }
    } else {
        pick
    }
}

/// Tokens for a theme, with the art accent swapped in when given (its
/// hover and glyph colors re-derived so they still read).
pub(crate) fn theme_tokens(theme: &Theme, art_accent: Option<Rgba>) -> Tokens {
    let mut palette = theme.palette;
    if let Some(accent) = art_accent {
        palette.accent = accent;
        palette.accent_hover = match theme.base {
            Base::Dark => accent.mix(Rgba::WHITE, 0.2),
            Base::Light => accent.mix(Rgba::BLACK, 0.15),
        };
        palette.on_accent = accent.readable_glyph();
    }
    Tokens::from_palette(&palette, theme.base == Base::Light)
}

impl AppState {
    /// The theme the picks resolve to (falls back to the first
    /// built-in, Dark, when a saved id is gone).
    pub(crate) fn active_theme(&self) -> &Theme {
        let id = resolve_pick(
            &self.theme_pick,
            &self.theme_light,
            &self.theme_dark,
            self.system_light,
        );
        self.themes
            .iter()
            .find(|t| t.id == id)
            .or_else(|| self.themes.first())
            .expect("built-in themes are never empty")
    }

    /// Accent from album art: the explicit pref, else the theme's own
    /// `accent_from_art` default.
    pub(crate) fn accent_from_art_on(&self) -> bool {
        self.accent_from_art
            .unwrap_or_else(|| self.active_theme().accent_from_art)
    }

    /// Push the active theme (plus the art accent when on) into the
    /// live tokens. Every theme-affecting change calls this.
    pub(crate) fn apply_theme(&mut self) {
        let art = self.glide_accent().or(if self.accent_from_art_on() {
            self.art_accent
        } else {
            None
        });
        set_tokens(theme_tokens(self.active_theme(), art));
    }

    /// The accent the theme settles on (art accent when on and found,
    /// else the theme's own).
    pub(crate) fn target_accent(&self) -> Rgba {
        match self.art_accent {
            Some(art) if self.accent_from_art_on() => art,
            _ => self.active_theme().palette.accent,
        }
    }

    /// The in-between accent while a glide runs (eased), `None` once
    /// it is done or when there is none.
    pub(crate) fn glide_accent(&self) -> Option<Rgba> {
        let (from, start) = self.accent_glide?;
        let t = start.elapsed().as_secs_f32() * 1000.0 / ACCENT_GLIDE_MS;
        (t < 1.0).then(|| {
            let eased = 1.0 - (1.0 - t).powi(3);
            from.mix(self.target_accent(), eased)
        })
    }

    /// One `Animate` step of the accent glide: re-apply the in-between
    /// color, and end the glide (settled tokens) once it arrives.
    pub(crate) fn step_accent_glide(&mut self) {
        if !self.animations_enabled || self.glide_accent().is_none() {
            self.accent_glide = None;
        }
        self.apply_theme();
    }

    /// Re-read the user themes folder (built-ins always present) and
    /// re-apply; failures are kept for the Settings list.
    pub(crate) fn reload_themes(&mut self) {
        let (user, failures) = core_theme::load_dir(&themes_dir(&self.library_path));
        for f in &failures {
            yutu_core::app_log::warn("theme", format_args!("{}: {}", f.file, f.reason));
        }
        for t in &user {
            for w in &t.warnings {
                yutu_core::app_log::warn("theme", format_args!("{}: {w}", t.id));
            }
        }
        self.themes = merge_themes(user);
        self.theme_failures = failures;
        self.apply_theme();
    }

    /// Load the saved picks (garbage keeps the defaults) and the theme
    /// catalog, then apply. Called once at startup.
    pub(crate) fn load_theme_prefs(&mut self) {
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            if let Some(raw) = store.get_pref("theme").unwrap_or(None) {
                self.theme_pick = raw;
            }
            if let Some(raw) = store.get_pref("theme_light").unwrap_or(None) {
                self.theme_light = raw;
            }
            if let Some(raw) = store.get_pref("theme_dark").unwrap_or(None) {
                self.theme_dark = raw;
            }
            self.accent_from_art = match store.get_pref("accent_from_art").unwrap_or(None) {
                Some(v) if v == "1" => Some(true),
                Some(v) if v == "0" => Some(false),
                _ => None,
            };
        }
        self.reload_themes();
    }

    pub(crate) fn save_theme_pref(&self, key: &str, value: &str) {
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref(key, value);
        }
    }

    /// Theme import: copy a picked `.toml` / `.json` into the themes
    /// folder under a safe id (validated first, so a broken file is
    /// refused instead of landing in the folder). Returns the new id.
    pub(crate) fn import_theme_file(&mut self, src: &std::path::Path) -> Result<String, String> {
        let format = Format::from_path(src)
            .ok_or_else(|| core_theme::ThemeError::BadExtension.to_string())?;
        let theme = core_theme::load_file(src).map_err(|e| e.to_string())?;
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&theme.name);
        let id = core_theme::sanitize_id(stem);
        let dir = themes_dir(&self.library_path);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ext = match format {
            Format::Toml => "toml",
            Format::Json => "json",
        };
        // One file per id: drop a sibling of the other format so the
        // folder never holds two themes fighting over one id.
        for other in ["toml", "json"] {
            if other != ext {
                let _ = std::fs::remove_file(dir.join(format!("{id}.{other}")));
            }
        }
        std::fs::copy(src, dir.join(format!("{id}.{ext}"))).map_err(|e| e.to_string())?;
        self.reload_themes();
        Ok(id)
    }
}

/// Accent glide length when the art accent changes (ms).
pub(crate) const ACCENT_GLIDE_MS: f32 = 600.0;

/// Cover pixels read for the art accent (a little more than the 8x8
/// backdrop so small vivid details still count).
const ART_ACCENT_PX: u32 = 24;

impl AppState {
    /// Accent from album art: once the live song's cover is on disk,
    /// read its dominant vivid color on a blocking thread. Keyed by
    /// cover URL and window color, so a theme switch re-reads it for
    /// the new background. Polled from Tick; a song with no cover, or
    /// the feature off, drops back to the theme accent.
    pub(crate) fn art_accent_task(&mut self) -> iced::Task<Message> {
        let url = self
            .accent_from_art_on()
            .then(|| self.queue.current().cloned())
            .flatten()
            .and_then(|t| self.blur_source_url(&t).or_else(|| self.hero_art_for(&t)));
        let Some(url) = url else {
            if self.art_accent.is_some() || self.art_accent_url.is_some() {
                self.art_accent = None;
                self.art_accent_url = None;
                self.apply_theme();
            }
            return iced::Task::none();
        };
        let window = self.active_theme().palette.window;
        let key = format!("{url}|{}", window.to_hex());
        if self.art_accent_url.as_deref() == Some(key.as_str()) {
            return iced::Task::none();
        }
        let Some(path) = artwork_path_for(&url).filter(|p| std::path::Path::new(p).exists()) else {
            return iced::Task::none();
        };
        self.art_accent_url = Some(key.clone());
        iced::Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let img = image::open(&path).ok()?;
                    let small = img.resize_exact(
                        ART_ACCENT_PX,
                        ART_ACCENT_PX,
                        image::imageops::FilterType::Triangle,
                    );
                    core_theme::accent_from_pixels(&small.to_rgba8().into_raw(), window)
                })
                .await
                .ok()
                .flatten()
            },
            move |accent| Message::ArtAccentReady {
                url: key.clone(),
                accent,
            },
        )
    }

    /// System file picker for a theme file (`.toml` / `.json`).
    #[cfg(feature = "file-picker")]
    pub(crate) fn theme_browse_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_title("Import theme")
                    .add_filter("Theme", &["toml", "json"])
                    .pick_file()
                    .await
                    .map(|h| h.path().to_path_buf())
            },
            Message::ThemeImportPicked,
        )
    }
}

/// The color swatch strip a theme row shows (window, surface, accent,
/// text).
pub(crate) fn theme_swatches(theme: &Theme) -> [iced::Color; 4] {
    let p = &theme.palette;
    [p.window, p.surface, p.accent, p.text].map(to_color)
}

/// Four touching color chips in one rounded, outlined pill: a theme's
/// look at a glance in its picker row.
pub(crate) fn swatch_strip(colors: [iced::Color; 4]) -> iced::Element<'static, Message> {
    use iced::widget::{container, row, Space};
    let chips = colors.map(|c| {
        container(Space::new())
            .width(iced::Length::Fixed(9.0))
            .height(iced::Length::Fixed(16.0))
            .style(move |_: &iced::Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(c)),
                ..Default::default()
            })
            .into()
    });
    container(row(chips))
        .padding(1)
        .style(|_: &iced::Theme| iced::widget::container::Style {
            border: iced::Border {
                color: tk().outline,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .clip(true)
        .into()
}

impl AppState {
    /// Themes for one half of the Follow system pair (light-base
    /// themes for light mode, dark-base for dark).
    pub(crate) fn themes_for_mode(&self, light: bool) -> impl Iterator<Item = &Theme> {
        self.themes
            .iter()
            .filter(move |t| (t.base == Base::Light) == light)
    }
}

impl AppState {
    /// Settings > Appearance theme block: the picker (swatch on its
    /// face), the Follow system pair while that pick is on, the folder
    /// actions, any unreadable files, and the art accent toggle.
    pub(crate) fn theme_settings(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text, toggler};
        use iced::Fill;
        let picker = |kind: SelectMenu, label: String, swatch: Option<[iced::Color; 4]>| {
            let mut face_row = row![].spacing(8).align_y(iced::alignment::Vertical::Center);
            if let Some(colors) = swatch {
                face_row = face_row.push(swatch_strip(colors));
            }
            let face: iced::Element<'_, Message> = button(
                face_row
                    .push(btn_text(label, 13).width(Fill))
                    .push(btn_icon("chevron-down", 13.0, tk().text_muted))
                    .width(Fill),
            )
            .style(select_face)
            .on_press(Message::OpenSelectMenu(kind.clone()))
            .padding(8)
            .width(Fill)
            .into();
            let menu = (self.sort_menu.as_ref() == Some(&kind))
                .then(|| self.select_menu_card(&kind, 280.0));
            settings_input(dropdown(face, menu).into())
        };
        let following = self.theme_pick == THEME_SYSTEM;
        let main_label = if following {
            self.language.theme_system()
        } else {
            self.active_theme().name.clone()
        };
        let mut block = column![
            text(self.language.theme_label()).size(13),
            picker(
                SelectMenu::Theme,
                main_label,
                (!following).then(|| theme_swatches(self.active_theme())),
            ),
        ]
        .spacing(6);
        if following {
            for (kind, label, id) in [
                (
                    SelectMenu::ThemeLight,
                    self.language.theme_light_pick(),
                    &self.theme_light,
                ),
                (
                    SelectMenu::ThemeDark,
                    self.language.theme_dark_pick(),
                    &self.theme_dark,
                ),
            ] {
                let theme = self.themes.iter().find(|t| &t.id == id);
                block = block
                    .push(text(label).size(12).color(tk().text_muted))
                    .push(picker(
                        kind,
                        theme.map_or_else(|| id.clone(), |t| t.name.clone()),
                        theme.map(theme_swatches),
                    ));
            }
        }
        let mut actions = row![].spacing(8);
        #[cfg(feature = "file-picker")]
        {
            actions = actions.push(icon_pill_button(
                "file-up",
                self.language.theme_import(),
                Message::ImportThemeBrowse,
            ));
        }
        actions = actions
            .push(icon_pill_button(
                "folder",
                self.language.theme_open_folder(),
                Message::OpenThemesFolder,
            ))
            .push(
                button(
                    iced::widget::container(btn_text(self.language.theme_reload(), ICON_PILL_TEXT))
                        .height(Fill)
                        .align_y(iced::alignment::Vertical::Center),
                )
                .style(ghost_button)
                .on_press(Message::ReloadThemes)
                .padding([0.0, ICON_PILL_PAD_X])
                .height(iced::Length::Fixed(PILL_H)),
            );
        block = block
            .push(iced::widget::Space::new().height(2))
            .push(actions.wrap().vertical_spacing(8))
            .push(
                text(self.language.theme_hint())
                    .size(12)
                    .color(tk().text_muted),
            );
        if !self.theme_failures.is_empty() {
            let mut lines = column![text(self.language.theme_failures())
                .size(12)
                .color(tk().text_muted)]
            .spacing(2);
            for f in &self.theme_failures {
                lines = lines.push(
                    text(format!("{} · {}", f.file, f.reason))
                        .size(12)
                        .color(tk().text_muted),
                );
            }
            block = block.push(lines);
        }
        block
            .push(
                toggler(self.accent_from_art_on())
                    .label(self.language.accent_from_art())
                    .on_toggle(Message::AccentFromArtToggled)
                    .style(red_toggle),
            )
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_cleanly_and_ids_are_unique() {
        for (id, text) in BUILTIN {
            let t = core_theme::parse(id, text, Format::Toml).unwrap();
            assert!(t.warnings.is_empty(), "{id}: {:?}", t.warnings);
            assert!(
                t.palette.text.contrast(t.palette.surface) >= 4.5,
                "{id} text contrast"
            );
            assert!(
                t.palette.text_muted.contrast(t.palette.surface) >= 3.0,
                "{id} muted contrast"
            );
            assert!(
                t.palette.on_accent.contrast(t.palette.accent) >= 3.0,
                "{id} on_accent contrast"
            );
        }
        let themes = builtin_themes();
        assert_eq!(themes.len(), BUILTIN.len());
        let mut ids: Vec<_> = themes.iter().map(|t| t.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), BUILTIN.len());
    }

    #[test]
    fn builtin_dark_is_the_default_look() {
        let dark = &builtin_themes()[0];
        assert_eq!(dark.id, THEME_DEFAULT);
        assert_eq!(theme_tokens(dark, None), Tokens::DARK);
        assert_eq!(builtin_themes()[1].id, THEME_DEFAULT_LIGHT);
    }

    #[test]
    fn user_theme_replaces_builtin_with_same_id() {
        let mine = core_theme::parse("nord", "name = \"My Nord\"", Format::Toml).unwrap();
        let extra = core_theme::parse("zz", "", Format::Toml).unwrap();
        let all = merge_themes(vec![mine, extra]);
        assert_eq!(all.len(), BUILTIN.len() + 1);
        assert_eq!(all.iter().find(|t| t.id == "nord").unwrap().name, "My Nord");
        assert_eq!(all.last().unwrap().id, "zz");
    }

    #[test]
    fn system_pick_follows_the_os_mode() {
        assert_eq!(resolve_pick("system", "light", "nord", true), "light");
        assert_eq!(resolve_pick("system", "light", "nord", false), "nord");
        assert_eq!(resolve_pick("nord", "light", "dark", true), "nord");
    }

    #[test]
    fn art_accent_rederives_hover_and_glyph() {
        let dark = &builtin_themes()[0];
        let pale = Rgba::rgb(0xFF, 0xE0, 0x66);
        let t = theme_tokens(dark, Some(pale));
        assert_eq!(t.accent, to_color(pale));
        assert_eq!(t.on_accent, to_color(Rgba::BLACK));
        assert_ne!(t.accent_hover, Tokens::DARK.accent_hover);
    }
}
