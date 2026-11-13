//! yutu-ui dialogs_menus: dialogs, sync UI, menus and drag state (split from lib.rs, no behavior change).

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

impl AppState {
    /// Hover-reveal remove slot with a width-stable box: the idle
    /// branch renders the same button assembly with a blank glyph
    /// (no press handler) instead of a spacer, so the text column
    /// keeps the exact same width hovered or not and boundary titles
    /// never flip word wrap on hover. The x stays invisible until the
    /// row hovers, then appears white. `None` arms nothing (idle).
    pub(crate) fn hover_remove_slot(armed: Option<Message>) -> iced::Element<'static, Message> {
        use iced::widget::button;
        let glyph = if armed.is_some() {
            btn_icon("x", 11.0, iced::Color::WHITE)
        } else {
            Self::empty_slot_icon(11.0)
        };
        let mut btn = button(glyph).style(hover_row).padding(3);
        if let Some(msg) = armed {
            btn = btn.on_press(msg);
        }
        btn.into()
    }

    /// Cookie import panel: normal browser login, no OAuth client needed.
    /// A saved session stays signed in across restarts (re-import refreshes
    /// without another export); likes, dislikes, and playlist edits mirror
    /// to YouTube Music while signed in. Syncing (not a separate import)
    /// pulls web changes in: the Sync section below holds the action.
    #[cfg(feature = "innertube")]
    pub(crate) fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, text, text::Wrapping, Column, Row};
        use iced::Fill;
        if self.cookie_logged_in {
            return column![
                text(self.language.signed_in_cookies())
                    .size(13)
                    .color(ACCENT),
                // No separate import action: Sync now (below) is the one
                // refresh, so the two buttons never coexist.
                button(btn_text(self.language.sign_out(), 12))
                    .style(ghost_button)
                    .on_press(Message::RequestSignOut)
                    .padding(8),
            ]
            .spacing(4)
            .into();
        }
        // Room check from live geometry: window minus the animated
        // sidebars, app padding, and row spacing (conservative). Roomy
        // windows get the fixed field aligned with the other inputs;
        // narrow ones get a fluid field that shrinks, so Import is always
        // reachable. The label budget follows the actual field width.
        let win_w = self.window.map(|s| s.width).unwrap_or(1280.0);
        let center_w = win_w - self.sidebar_w - self.right_w - 32.0;
        let roomy = cookie_row_roomy(center_w);
        let field_w = if roomy {
            SETTINGS_INPUT_W
        } else {
            (center_w - COOKIE_IMPORT_ROOM).max(0.0)
        };
        let (pick_label, pick_color) = if self.cookie_path.trim().is_empty() {
            (self.language.choose_cookies_file().to_string(), MUTED)
        } else {
            (
                trim_cookie_path(self.cookie_path.trim(), cookie_path_budget(field_w)),
                iced::Color::WHITE,
            )
        };
        // Fixed field width on roomy windows, aligned with the other
        // capped inputs (a Row hands Fill children tight limits that no
        // max_width can shrink, so the cap here is the width itself).
        // Import sits to its right in leftover space. The trimmed label
        // can never outgrow the field, so nothing paints past the border.
        let mut row_items: Vec<iced::Element<'_, Message>> = vec![button(
            iced::widget::row![
                btn_icon("folder", 14.0, MUTED),
                text(pick_label)
                    .size(12)
                    .color(pick_color)
                    .width(Fill)
                    .wrapping(Wrapping::None),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .width(Fill),
        )
        .style(file_pick)
        .on_press(Message::CookieBrowse)
        .padding(10)
        .width(if roomy {
            iced::Length::Fixed(SETTINGS_INPUT_W)
        } else {
            Fill
        })
        .into()];
        // Import only once a txt file is picked: anything else cannot
        // import, so the button stays out of the way until it can work.
        if cookie_pick_valid(&self.cookie_path) {
            row_items.push(
                button(self.busy_content(self.language.import_button(), 12.0))
                    .style(accent_button)
                    .on_press(Message::CookieImport)
                    .padding(8)
                    .into(),
            );
        }
        let content: Vec<iced::Element<'_, Message>> = vec![
            text(self.language.cookie_explainer())
                .size(11)
                .color(MUTED)
                .into(),
            Row::with_children(row_items)
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .width(Fill)
                .into(),
        ];
        Column::with_children(content).spacing(4).into()
    }

    /// Shared modal frame: dimmed dismiss scrim plus a card that rises
    /// from below on the shared entrance (snapped when animations are
    /// off, so the dialog never strands off-center). `dismiss` fires on
    /// scrim click (left or right).
    pub(crate) fn modal_frame<'a>(
        &self,
        card: iced::Element<'a, Message>,
        dismiss: Message,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, container, mouse_area, text, Space, Stack};
        use iced::Fill;
        /// Card travel below center at entrance start, in px.
        const MODAL_RISE_PX: f32 = 48.0;
        let scrim: iced::Element<'_, Message> = mouse_area(
            container(text("").size(1))
                .style(modal_dim)
                .width(Fill)
                .height(Fill),
        )
        .on_press(dismiss.clone())
        .on_right_press(dismiss)
        .on_enter(Message::ClearHovers)
        .on_exit(Message::ClearHovers)
        // Arrow over the whole backdrop: the vote shadows every
        // widget beneath, so background buttons never show the hand.
        .interaction(iced::mouse::Interaction::Idle)
        .into();
        // Entrance: the card starts below center and settles as the
        // ticker eases `modal_anim` to 1.
        let rise = (1.0 - self.modal_anim).clamp(0.0, 1.0) * MODAL_RISE_PX;
        let placed: iced::Element<'_, Message> = container(
            column![Space::new().height(iced::Length::Fixed(rise)), card,]
                .align_x(iced::alignment::Horizontal::Center),
        )
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into();
        Stack::with_children(vec![scrim, placed]).into()
    }

    /// Post-import cleanup prompt: the session is already in the OS
    /// keychain, so the plaintext export can go. Delete is the CTA;
    /// scrim/Esc keeps the file (safe default). Dimmed backdrop plus a
    /// card that rises from below, same layer pattern as the menus.
    #[cfg(feature = "innertube")]
    pub(crate) fn cookie_cleanup_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, text, text::Wrapping};
        use iced::Fill;
        let src = self.pending_cookie_cleanup.clone()?;
        let card: iced::Element<'_, Message> = container(
            column![
                text(self.language.cleanup_title()).size(16),
                text(self.language.cleanup_body()).size(12).color(MUTED),
                // Glyph wrapping: a long path wraps mid-name and grows
                // the card instead of painting past its border.
                text(src)
                    .size(11)
                    .color(MUTED)
                    .width(Fill)
                    .wrapping(Wrapping::WordOrGlyph),
                iced::widget::row![
                    button(btn_text(self.language.delete_export(), 12))
                        .style(accent_button)
                        .on_press(Message::CookieCleanupDelete)
                        .padding(8),
                    button(btn_text(self.language.keep_file(), 12))
                        .style(ghost_button)
                        .on_press(Message::CookieCleanupKeep)
                        .padding(8),
                ]
                .spacing(8),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(400)
        .into();
        Some(self.modal_frame(card, Message::CookieCleanupKeep))
    }

    /// Sign-out confirm: clearing the session is one tap in Settings,
    /// so the destructive step gets an explicit dialog. Delete is the
    /// CTA; scrim/Esc stays signed in (safe default).
    #[cfg(feature = "innertube")]
    pub(crate) fn signout_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, text};
        if !self.pending_signout {
            return None;
        }
        let card: iced::Element<'_, Message> = container(
            column![
                text(self.language.signout_title()).size(16),
                text(self.language.signout_body()).size(12).color(MUTED),
                iced::widget::row![
                    button(btn_text(self.language.sign_out(), 12))
                        .style(accent_button)
                        .on_press(Message::CookieLogout)
                        .padding(8),
                    button(btn_text(self.language.stay_signed_in(), 12))
                        .style(ghost_button)
                        .on_press(Message::CancelSignOut)
                        .padding(8),
                ]
                .spacing(8),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(400)
        .into();
        Some(self.modal_frame(card, Message::CancelSignOut))
    }

    /// Playlist-delete confirm: the menu row is one tap from wiping a
    /// whole list (mirrored to YouTube Music while signed in), so the
    /// destructive step gets an explicit dialog. Delete is the CTA;
    /// scrim/Esc keeps the playlist (safe default).
    pub(crate) fn playlist_delete_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, text};
        let id = self.pending_playlist_delete.clone()?;
        let (title, n) = self
            .find_playlist(&id)
            .map(|p| (p.title.clone(), p.tracks.len()))
            .unwrap_or((id.clone(), 0));
        let card: iced::Element<'_, Message> = container(
            column![
                text(self.language.delete_playlist_title()).size(16),
                text(self.language.delete_playlist_body(&title, n))
                    .size(12)
                    .color(MUTED),
                iced::widget::row![
                    button(btn_text(self.language.delete_playlist(), 12))
                        .style(accent_button)
                        .on_press(Message::DeletePlaylist(id))
                        .padding(8),
                    button(btn_text(self.language.keep_playlist(), 12))
                        .style(ghost_button)
                        .on_press(Message::CancelDeletePlaylist)
                        .padding(8),
                ]
                .spacing(8),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(400)
        .into();
        Some(self.modal_frame(card, Message::CancelDeletePlaylist))
    }

    /// Playlist-rename dialog: the menu row opens it with the current
    /// title prefilled (mirrored to YouTube Music while signed in).
    /// Save is the CTA; scrim/Esc keeps the old name (safe default).
    pub(crate) fn playlist_rename_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, text, text_input};
        use iced::Fill;
        let id = self.pending_playlist_rename.clone()?;
        let name = self.rename_playlist_name.clone();
        let card: iced::Element<'_, Message> = container(
            column![
                text(self.language.rename_playlist_title()).size(16),
                text_input(
                    self.language.rename_playlist_placeholder(),
                    &self.rename_playlist_name
                )
                .style(rounded_input)
                .on_input(Message::RenamePlaylistNameChanged)
                .width(Fill)
                .padding(10),
                iced::widget::row![
                    button(btn_text(self.language.rename_playlist_save(), 12))
                        .style(accent_button)
                        .on_press(Message::RenamePlaylist {
                            playlist_id: id,
                            title: name,
                        })
                        .padding(8),
                    button(btn_text(self.language.rename_playlist_cancel(), 12))
                        .style(ghost_button)
                        .on_press(Message::CancelRenamePlaylist)
                        .padding(8),
                ]
                .spacing(8),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(400)
        .into();
        Some(self.modal_frame(card, Message::CancelRenamePlaylist))
    }

    /// Self-update row (About section): a manual check button, swapped
    /// for "Update and restart" plus a changelog link once a release
    /// is available (the modal covers the notes; the row stays one
    /// action).
    #[cfg(feature = "update")]
    pub(crate) fn update_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, row, text};
        if let Some(info) = &self.update_available {
            row![
                button(btn_text(self.language.update_and_restart(), 12))
                    .style(accent_button)
                    .on_press(Message::UpdateAndRestart)
                    .padding(8),
                link_row(
                    self.language.update_available(&info.latest_tag),
                    12.0,
                    info.html_url.clone(),
                ),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        } else {
            let mut items: Vec<iced::Element<'_, Message>> =
                vec![button(btn_text(self.language.check_for_updates(), 12))
                    .style(ghost_button)
                    .on_press(Message::CheckForUpdates)
                    .padding(8)
                    .into()];
            if self.update_checking {
                items.push(Self::spinner_frame(self.spin_frame, 12.0, MUTED));
                items.push(
                    text(self.language.checking_for_updates())
                        .size(12)
                        .color(MUTED)
                        .into(),
                );
            } else if self.update_checked {
                items.push(
                    text(self.language.up_to_date(crate::APP_VERSION))
                        .size(12)
                        .color(MUTED)
                        .into(),
                );
            }
            iced::widget::Row::with_children(items)
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .into()
        }
    }

    /// Offline builds: no HTTP client, so the check explains itself.
    #[cfg(not(feature = "update"))]
    pub(crate) fn update_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text(self.language.needs_update_check())
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Self-update modal: a newer release exists. Shows the tag, an
    /// excerpt of its notes, and a changelog link, with three actions:
    /// update and restart (downloads, verifies, installs), omit this
    /// version (never prompts again), or later (dismisses for this
    /// session). While downloading/installing the actions give way to
    /// progress; scrim/Esc cancels like "later".
    pub(crate) fn update_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, row, scrollable, text};
        use iced::Fill;
        let info = self.pending_update.clone()?;
        let busy = self.update_phase != UpdatePhase::Idle;
        let notes_h = ((info.notes.lines().count() as f32) * 15.0 + 12.0).clamp(48.0, 220.0);
        let mut body: Vec<iced::Element<'_, Message>> = vec![
            text(self.language.update_title()).size(16).into(),
            link_row(
                self.language.update_available(&info.latest_tag),
                13.0,
                info.html_url.clone(),
            ),
        ];
        if !info.notes.trim().is_empty() {
            body.push(
                container(
                    scrollable(text(info.notes.clone()).size(12).color(MUTED).width(Fill))
                        .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                        .style(move |t, s| {
                            slim_scroll_alpha(t, s, self.scroll_opacity("update-notes"))
                        })
                        .on_scroll(|v| Message::Scrolled("update-notes", v.absolute_offset().y))
                        .height(iced::Length::Fixed(notes_h))
                        .width(Fill),
                )
                .width(Fill)
                .into(),
            );
        }
        if busy {
            let progress: iced::Element<'_, Message> = match self.update_phase {
                UpdatePhase::Downloading => match self.update_progress {
                    Some((done, Some(total))) if total > 0 => {
                        let pct = (done.min(total) as f32 / total as f32 * 100.0).clamp(0.0, 100.0);
                        column![
                            iced::widget::progress_bar(0.0..=100.0, pct)
                                .girth(8)
                                .style(import_bar),
                            text(self.language.update_progress(pct.round() as u64))
                                .size(12)
                                .color(MUTED),
                        ]
                        .spacing(4)
                        .into()
                    }
                    _ => row![
                        Self::spinner_frame(self.spin_frame, 12.0, ACCENT),
                        text(self.language.update_progress_unknown())
                            .size(12)
                            .color(MUTED),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .into(),
                },
                _ => row![
                    Self::spinner_frame(self.spin_frame, 12.0, ACCENT),
                    text(self.language.update_installing())
                        .size(12)
                        .color(MUTED),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            };
            body.push(progress);
        } else {
            body.push(
                row![
                    button(btn_text(self.language.update_and_restart(), 12))
                        .style(accent_button)
                        .on_press(Message::UpdateAndRestart)
                        .padding(8),
                    button(btn_text(self.language.omit_version(), 12))
                        .style(ghost_button)
                        .on_press(Message::UpdateOmitVersion)
                        .padding(8),
                    button(btn_text(self.language.update_later(), 12))
                        .style(ghost_button)
                        .on_press(Message::UpdateLater)
                        .padding(8),
                ]
                .spacing(8)
                .into(),
            );
        }
        let card: iced::Element<'_, Message> = container(column(body).spacing(8))
            .style(card_bg)
            .padding(16)
            .width(440)
            .into();
        // Non-dismissing scrim (see modal_frame): clicks outside are
        // eaten with NoOp, Esc is a no-op for this modal; leaving goes
        // through Update and restart, Omit, or Later.
        Some(self.modal_frame(card, Message::NoOp))
    }

    /// Persist scrobble secrets to the OS keychain, one write per field
    /// (never per keystroke). Empty fields delete their entries, so
    /// cleared tokens leave no residue. Every field is attempted even
    /// when one fails; the first error surfaces. In-memory state always
    /// applies, so a locked/missing keychain only costs a re-entry next
    /// launch; only failures surface (success stays quiet to never
    /// clobber a progress status behind the dialog).
    pub(crate) fn persist_scrobble_secrets(&mut self) {
        let saves = [
            (yutu_core::SCROBBLE_LB_TOKEN, self.scrobble_lb_token.clone()),
            (yutu_core::SCROBBLE_FM_KEY, self.scrobble_fm_key.clone()),
            (
                yutu_core::SCROBBLE_FM_SECRET,
                self.scrobble_fm_secret.clone(),
            ),
            (yutu_core::SCROBBLE_FM_SK, self.scrobble_fm_sk.clone()),
        ];
        let mut first_err: Option<String> = None;
        for (account, value) in saves {
            if let Err(e) = save_scrobble_secret(account, &value) {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
        if let Some(e) = first_err {
            yutu_core::app_log::warn("yutu-scrobble", format!("keychain save failed: {e}"));
            self.status = Self::failure_status(self.language.fail_scrobble_save(), e);
        }
    }

    /// Scrobbling credentials dialog: the enable toggle plus the
    /// ListenBrainz/Last.fm fields, same controls as the old inline
    /// section. Scrim/Esc closes it, edits are kept.
    pub(crate) fn scrobble_dialog(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, checkbox, column, container, text, text_input};
        if !self.scrobble_modal_open {
            return None;
        }
        let card: iced::Element<'_, Message> = container(
            column![
                text(self.language.scrobbling_section()).size(16),
                checkbox(self.scrobble_enabled)
                    .label(self.language.enable_scrobbling())
                    .on_toggle(Message::ScrobbleToggled),
                text_input(
                    self.language.scrobble_user_placeholder(),
                    &self.scrobble_user
                )
                .style(rounded_input)
                .on_input(Message::ScrobbleUserChanged)
                .padding(10),
                text_input(
                    self.language.scrobble_lb_placeholder(),
                    &self.scrobble_lb_token
                )
                .style(rounded_input)
                .on_input(Message::ScrobbleLbTokenChanged)
                .secure(true)
                .padding(10),
                text_input(
                    self.language.scrobble_fm_key_placeholder(),
                    &self.scrobble_fm_key
                )
                .style(rounded_input)
                .on_input(Message::ScrobbleFmKeyChanged)
                .padding(10),
                text_input(
                    self.language.scrobble_fm_secret_placeholder(),
                    &self.scrobble_fm_secret
                )
                .style(rounded_input)
                .on_input(Message::ScrobbleFmSecretChanged)
                .secure(true)
                .padding(10),
                text_input(
                    self.language.scrobble_fm_sk_placeholder(),
                    &self.scrobble_fm_sk
                )
                .style(rounded_input)
                .on_input(Message::ScrobbleFmSkChanged)
                .secure(true)
                .padding(10),
                button(btn_text(self.language.done_button(), 12))
                    .style(accent_button)
                    .on_press(Message::CloseScrobbleModal)
                    .padding(8),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(440)
        .into();
        Some(self.modal_frame(card, Message::CloseScrobbleModal))
    }

    /// Sync panel: manual refresh plus the scheduled pull that keeps
    /// web edits (likes, playlist songs/titles, deletions) flowing into
    /// the app. Pending local pushes replay first, so unpushed intent is
    /// never dropped by the pull.
    #[cfg(feature = "innertube")]
    pub(crate) fn sync_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, checkbox, column, row, text};
        if !self.logged_in && !self.cookie_logged_in {
            return text("").size(1).into();
        }
        column![
            text(
                self.language
                    .last_sync(&self.language.sync_ago(self.last_sync_unix, now_unix())),
            )
            .size(12)
            .color(MUTED),
            row![
                button(self.busy_content(self.language.sync_now(), 12.0))
                    .style(accent_button)
                    .on_press(Message::SyncNow)
                    .padding(8),
                checkbox(self.auto_sync)
                    .label(self.language.auto_sync_label())
                    .on_toggle(Message::AutoSyncToggled),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center),
        ]
        .spacing(4)
        .into()
    }

    /// Offline builds: sync needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn sync_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text(self.language.needs_innertube_sync())
            .size(12)
            .color(MUTED)
            .into()
    }

    /// True while a background sync pull runs (shared pipeline, quiet
    /// UI): the top banner stays down, the sidebar title spins instead.
    #[cfg(feature = "innertube")]
    pub(crate) fn syncing(&self) -> bool {
        self.import_active && self.sync_active
    }

    /// One spinning-loader frame: the verbatim loader glyph rotated
    /// `frame` steps (same precedent as the heart fill-flip: rotation
    /// applied at runtime, no custom drawing, no new asset files).
    /// Shared by the sidebar spinner, busy buttons, and the artist
    /// Show-more loader. The tint is the caller's text color: glyphs
    /// must contrast their background (white on red buttons, red on
    /// dark surfaces).
    pub(crate) fn spinner_frame(
        frame: u64,
        size: f32,
        color: iced::Color,
    ) -> iced::Element<'static, Message> {
        use iced::widget::svg;
        const LOADER: &str = include_str!("../assets/icons/loader-circle.svg");
        // Parsed once: every tick shares the rotated handles.
        static SPINNER_FRAMES: std::sync::LazyLock<Vec<svg::Handle>> =
            std::sync::LazyLock::new(|| {
                (0..SPINNER_STEPS)
                    .map(|i| {
                        let angle = i * 360 / SPINNER_STEPS;
                        let rotated = LOADER.replace(
                            "<path",
                            &format!("<path transform=\"rotate({angle} 12 12)\""),
                        );
                        svg::Handle::from_memory(rotated.into_bytes())
                    })
                    .collect()
            });
        let frame = (frame % SPINNER_STEPS as u64) as usize;
        svg(SPINNER_FRAMES[frame].clone())
            .width(size)
            .height(size)
            .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
            .into()
    }

    /// Sidebar sync spinner: the rotating loader while a sync runs,
    /// a same-width empty box otherwise (the pin button never hops when
    /// a sync starts or stops).
    #[cfg(feature = "innertube")]
    pub(crate) fn sync_spinner(&self) -> iced::Element<'_, Message> {
        if self.syncing() {
            Self::spinner_frame(self.spin_frame, 14.0, ACCENT)
        } else {
            iced::widget::Space::new()
                .width(iced::Length::Fixed(14.0))
                .height(iced::Length::Shrink)
                .into()
        }
    }

    /// Button content with a loading state: the plain label when idle
    /// (nothing reserved, so the text centers exactly), the label plus
    /// a spinner on its right while the pipeline runs.
    #[cfg(feature = "innertube")]
    pub(crate) fn busy_content(
        &self,
        label: impl Into<String>,
        size: f32,
    ) -> iced::Element<'static, Message> {
        use iced::widget::row;
        let label: String = label.into();
        if self.pipeline_busy() {
            // White like the label: these buttons sit on red
            // (`accent_button`), where the accent glyph would vanish.
            row![
                btn_text(label, size),
                Self::spinner_frame(self.spin_frame, 12.0, iced::Color::WHITE),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        } else {
            btn_text(label, size).into()
        }
    }

    /// True while any pipeline run is in flight (import or sync):
    /// busy buttons spin and the `Animate` ticker runs for them.
    #[cfg(feature = "innertube")]
    pub(crate) fn pipeline_busy(&self) -> bool {
        self.login_busy || self.import_active
    }

    /// Offline builds run no pipeline: never busy, no ticker for it.
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn pipeline_busy(&self) -> bool {
        false
    }

    /// Offline builds never sync: no spinner, no layout shift.
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn sync_spinner(&self) -> iced::Element<'_, Message> {
        iced::widget::Space::new()
            .width(iced::Length::Shrink)
            .height(iced::Length::Shrink)
            .into()
    }

    /// Top import banner (in-flow, pushes content down): live progress
    /// while an import runs (songs/liked counts, playlists done/total,
    /// red bar with %). Imports only: sync pulls stay quiet (sidebar
    /// spinner) so background refreshes never shove the layout.
    /// `None` when idle or syncing, so layout is untouched then.
    #[cfg(feature = "innertube")]
    pub(crate) fn import_banner_view(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{column, container, progress_bar, row, text, Space};
        use iced::Fill;
        // Same predicate both sides (see `syncing`): the banner is up
        // exactly when an import runs and no sync does.
        if !self.import_active || self.syncing() {
            return None;
        }
        // Displayed count eases toward the truth (see
        // `ease_import_shown`); the bar needs a denominator, falling
        // back to playlists done/total while no total is known.
        let shown = self.import_shown;
        let expected = self.import_expected;
        let song_pct = (expected > 0)
            .then(|| (shown.min(expected) as f32 / expected as f32 * 100.0).clamp(0.0, 100.0));
        let list_pct = (self.import_total > 0).then(|| {
            (self.import_done as f32 / self.import_total as f32 * 100.0).clamp(0.0, 100.0)
        });
        let pct = song_pct.or(list_pct).unwrap_or(0.0);
        let pct_label = if song_pct.or(list_pct).is_some() {
            format!("{}%", pct.round() as u32)
        } else {
            "…".to_string()
        };
        let counts = if self.import_total > 0 {
            // Totals show as soon as details lands, so the target is
            // visible long before the first page arrives.
            let songs = if expected > 0 {
                self.language.import_progress_songs(shown, expected)
            } else {
                self.language.import_progress_songs_simple(shown)
            };
            self.language.import_progress_detail(
                &songs,
                self.import_likes,
                self.import_done,
                self.import_total,
            )
        } else {
            self.language
                .import_progress_reading(shown, self.import_likes)
        };
        Some(
            container(
                column![
                    row![
                        text(self.language.importing_title())
                            .size(13)
                            .color(iced::Color::WHITE),
                        Space::new().width(Fill),
                        text(pct_label).size(13).color(ACCENT),
                    ]
                    .spacing(8)
                    .width(Fill)
                    .align_y(iced::alignment::Vertical::Center),
                    progress_bar(0.0..=100.0, pct).girth(8).style(import_bar),
                    text(counts).size(11).color(MUTED),
                ]
                .spacing(4),
            )
            .style(card_bg)
            .padding(10)
            .width(Fill)
            .into(),
        )
    }

    /// In-app error log: the same lines that go to stderr (capped at 30).
    /// Visible only with the `debug-panel` feature (without it, builds
    /// keep stderr, not the panel).
    pub(crate) fn error_section(&self) -> iced::Element<'_, Message> {
        #[cfg(not(feature = "debug-panel"))]
        {
            iced::widget::text("").size(1).into()
        }
        #[cfg(feature = "debug-panel")]
        {
            use iced::widget::{column, text};
            let body: iced::Element<'_, Message> = if self.error_log.is_empty() {
                text(self.language.no_errors()).size(12).color(MUTED).into()
            } else {
                column(
                    self.error_log
                        .iter()
                        .rev()
                        .take(10)
                        .map(|e| text(e.clone()).size(11).color(MUTED).into())
                        .collect::<Vec<_>>(),
                )
                .spacing(2)
                .into()
            };
            column![text(self.language.error_section_title()).size(17), body,]
                .spacing(4)
                .into()
        }
    }

    /// Cache census, visible only with the `debug-panel` feature:
    /// every map here is capped and the
    /// renderer trims its own textures per frame, so a long session
    /// plateaus instead of growing. Builds without the flag hide it
    /// with the error panel above.
    pub(crate) fn cache_section(&self) -> iced::Element<'_, Message> {
        #[cfg(not(feature = "debug-panel"))]
        {
            iced::widget::text("").size(1).into()
        }
        #[cfg(feature = "debug-panel")]
        {
            use iced::widget::{column, text};
            column![
                text(self.language.cache_section_title()).size(17),
                text(self.language.cache_census(
                    self.artwork.len(),
                    self.catalog.len(),
                    self.lyrics_cache.len(),
                    self.url_cache.len(),
                    self.history.len(),
                    self.search_cache.len(),
                ))
                .size(12)
                .color(MUTED),
            ]
            .spacing(4)
            .into()
        }
    }

    /// Offline builds: cookie import needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    pub(crate) fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text(self.language.needs_innertube_import())
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Open the context menu, freezing the cursor so the menu stays put.
    pub(crate) fn open_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.context_menu = Some(id);
        self.close_playlist_submenu();
        self.pl_menu = None;
        self.sort_menu = None;
        self.coll_menu = None;
    }

    /// Sidebar visibility with hover hysteresis: unpinned, the panel expands
    /// when the cursor enters the rail + grace zone, then stays open while
    /// the cursor is anywhere inside the expanded panel + grace. It only
    /// collapses once the cursor leaves that wider zone. The state is
    /// latched in `CursorMoved` (never derived from the transient animated
    /// width, which would flip the target mid-glide).
    pub(crate) fn sidebar_open(&self) -> bool {
        self.sidebar_pinned || self.sidebar_latched
    }

    /// Right panel visibility, same hysteresis contract as the sidebar.
    pub(crate) fn right_open(&self) -> bool {
        self.right_pinned || self.right_latched
    }

    /// Animated sidebar target widths from the open states above.
    pub(crate) fn sidebar_target(&self) -> f32 {
        if self.sidebar_open() {
            235.0
        } else {
            56.0
        }
    }

    /// Animated right-panel target width (collapsed keeps a rail with
    /// queue/autoplay thumbnails; the rail stays visible even when empty).
    pub(crate) fn right_target(&self) -> f32 {
        if self.right_open() {
            285.0
        } else {
            RIGHT_RAIL_W
        }
    }

    /// Collapsed rail thumb capacity: how many 32px thumbs fill the
    /// available height (window minus player reserve and chrome). Bounded
    /// below by the old fixed 6 so tiny windows never collapse to nothing.
    pub(crate) fn collapsed_rail_capacity(&self) -> usize {
        let wh = self.window.map(|s| s.height).unwrap_or(800.0);
        let avail = (wh - self.player_zone_h() - RAIL_CHROME_PX).max(0.0);
        let fit = ((avail + 6.0) / RAIL_ITEM_PITCH).floor() as usize;
        fit.clamp(RAIL_MIN_CAP, RAIL_MAX_CAP)
    }

    /// True while any transition is still converging (drives the ticker).
    /// A running pipeline counts: spinners advance one frame per tick.
    /// So does a lyrics follow glide (it settles on its own), and so
    /// does the artist full-shelf fetch (its Show-more spinner turns).
    /// An update probe or download/install keeps the About and modal
    /// spinners turning the same way.
    pub(crate) fn animating(&self) -> bool {
        self.sidebar_w != self.sidebar_target()
            || self.right_w != self.right_target()
            || self.drawer_anim != if self.drawer_open { 1.0 } else { 0.0 }
            || self.drag_gap != self.drag_gap_target()
            || self.scroll_fading()
            || self.pipeline_busy()
            || self.artist_full_loading.is_some()
            || self.lyrics_gliding()
            || self.modal_animating()
            || self.update_checking
            || self.update_phase != UpdatePhase::Idle
    }

    /// Scrollbar opacity for a scrollable key (0 when never scrolled).
    pub(crate) fn scroll_opacity(&self, key: &'static str) -> f32 {
        self.scroll_hint.get(key).map(|(_, o)| *o).unwrap_or(0.0)
    }

    /// Record scroll motion: the bar shows at full strength, then the
    /// ticker fades it once idle (see `Message::Animate`).
    pub(crate) fn lit_scroll(&mut self, key: &'static str) {
        self.scroll_hint
            .insert(key, (std::time::Instant::now(), 1.0));
    }

    /// True while any scrollbar fade still needs ticker frames: lit but
    /// idle past the delay (fresh motion needs no frames, it sits at 1).
    pub(crate) fn scroll_fading(&self) -> bool {
        self.scroll_hint.values().any(|(at, o)| {
            *o > 0.0 && at.elapsed() > std::time::Duration::from_millis(SCROLL_FADE_DELAY_MS)
        })
    }

    /// Eased drop-slot gap target: open while an armed slot commits to a
    /// real move, else shut. Own-spot hovers never open the gap. Covers
    /// both the queue drag and the manual playlist drag (the latter
    /// resolves its length from the drag's own playlist).
    pub(crate) fn drag_gap_target(&self) -> f32 {
        if self.effective_drag_slot().is_some() {
            return 1.0;
        }
        if let (Some(drag), Some(pid)) = (self.pl_drag, self.pl_drag_id.as_deref()) {
            if drag.live {
                let len = self.find_playlist(pid).map(|p| p.tracks.len()).unwrap_or(0);
                if drag
                    .slot
                    .is_some_and(|s| slot_commit(len, drag.from, s).is_some())
                {
                    return 1.0;
                }
            }
        }
        0.0
    }

    /// True once the press has moved past click slop and the slot lanes
    /// are visible. A merely armed press (plain click) never counts.
    pub(crate) fn dragging(&self) -> bool {
        self.drag.is_some_and(|d| d.live)
    }

    /// Latched slot filtered to real moves (own-spot hovers are None).
    /// Armed-but-unmoved presses never report a slot, so the gap stays
    /// shut and no bar renders before the lanes exist.
    pub(crate) fn effective_drag_slot(&self) -> Option<usize> {
        self.drag.and_then(|d| {
            if !d.live {
                return None;
            }
            d.slot
                .filter(|s| slot_commit(self.queue.len(), d.from, *s).is_some())
        })
    }

    /// Cancel any in-progress drag plus its press snapshot (one place,
    /// so no stale snap survives a queue mutation).
    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
        self.press_snap = None;
    }

    /// Drop every row hover (one place): arming any modal calls this
    /// so a frozen hover highlight never sits behind the dialog (the
    /// cursor never exits the row it was on when the modal appeared).
    /// Shared with the scrim hover-freeze (same frozen state).
    pub(crate) fn clear_hovers(&mut self) {
        self.hovered_track = None;
        self.hovered_recent = None;
        self.hovered_album = None;
        self.hovered_artist = None;
        self.hovered_search_playlist = None;
        self.hovered_playlist = None;
        self.hovered_queue = None;
        self.hovered_history = None;
        self.hovered_auto = None;
        self.hover_remove = None;
        self.pl_hover = None;
        self.pl_action_hover = None;
    }

    /// Close the context menu and drop its anchor.
    pub(crate) fn close_menu(&mut self) {
        self.context_menu = None;
        self.menu_at = None;
        self.close_playlist_submenu();
        self.pl_menu = None;
        self.sort_menu = None;
        self.coll_menu = None;
    }

    /// Dismiss the update modal and cancel in-flight download/install
    /// work (the generation bump quarantines stale progress and
    /// arrivals, like the import generation). "Later", scrim, and Esc
    /// land here without persisting anything; the settings row keeps
    /// offering the update.
    pub(crate) fn dismiss_update_modal(&mut self) {
        self.pending_update = None;
        self.update_phase = UpdatePhase::Idle;
        self.update_progress = None;
        self.update_gen += 1;
    }

    /// Close the playlist flyout and drop its hover state (one place,
    /// so a stale hot flag can never pin a dead panel open).
    pub(crate) fn close_playlist_submenu(&mut self) {
        self.submenu_playlists_for = None;
        self.submenu_trigger_hot = false;
        self.submenu_fly_hot = false;
        self.submenu_hot_at = None;
    }

    /// Open the playlist menu, freezing the cursor like the track menu.
    pub(crate) fn open_pl_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.pl_menu = Some(id);
        self.sort_menu = None;
        self.context_menu = None;
        self.close_playlist_submenu();
        self.coll_menu = None;
        self.show_suggest = false;
    }

    /// Open a selector menu, freezing the cursor like the other menus
    /// (only one menu kind is ever open). The open card floats from the
    /// button itself, never from the cursor.
    pub(crate) fn open_select_menu(&mut self, menu: SelectMenu) {
        self.menu_at = self.cursor;
        self.sort_menu = Some(menu);
        self.context_menu = None;
        self.close_playlist_submenu();
        self.pl_menu = None;
        self.coll_menu = None;
        self.show_suggest = false;
    }

    /// Open menu card for a selector kind: one row per pick with the
    /// active pick in accent. Rows send their message directly (pick
    /// handlers close the menu); the caller floats this from its button
    /// via the dropdown overlay and keeps the top-level scrim while open.
    pub(crate) fn select_menu_card(
        &self,
        kind: &SelectMenu,
        width: f32,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, row};
        use iced::Fill;
        let rows: Vec<(String, bool, Message)> = match kind {
            SelectMenu::Sort(pid) => {
                let current = self.playlist_sort_cached(pid);
                PlaylistSort::ALL
                    .iter()
                    .map(|opt| {
                        (
                            opt.label().to_string(),
                            *opt == current,
                            Message::PlaylistSortSelected {
                                playlist_id: pid.clone(),
                                sort: *opt,
                            },
                        )
                    })
                    .collect()
            }
            SelectMenu::Language => Language::ALL_LABELS
                .iter()
                .map(|name| {
                    (
                        name.to_string(),
                        self.language.label() == *name,
                        Message::LanguageChanged(Language::parse(name)),
                    )
                })
                .collect(),
            SelectMenu::LyricsProvider => LyricsProvider::all()
                .iter()
                .map(|p| {
                    (
                        p.label().to_string(),
                        *p == self.lyrics_provider,
                        Message::SetLyricsProvider(*p),
                    )
                })
                .collect(),
            SelectMenu::FontFamily => self
                .font_rows
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        name == self.font_family.label(),
                        Message::FontFamilyChanged(FontFamily::from_label(name)),
                    )
                })
                .collect(),
        };
        let mut menu_rows: Vec<iced::Element<'_, Message>> = Vec::with_capacity(rows.len());
        for (label, active, msg) in rows {
            let text = if active {
                btn_text(label, 14).color(ACCENT)
            } else {
                btn_text(label, 14).color(iced::Color::WHITE)
            };
            menu_rows.push(
                row_button(
                    row![text]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                )
                .style(hover_row)
                .on_press(msg)
                .padding(8)
                .width(Fill)
                .into(),
            );
        }
        // Capped height with scroll: long pickers (system fonts) stop
        // at SELECT_MENU_MAX_H and scroll instead of stretching the
        // overlay past the viewport. Natural height below the cap, so
        // short pickers show no empty gap and no scrollbar.
        container(
            iced::widget::scrollable(column(menu_rows).spacing(0).width(Fill))
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("select")))
                .on_scroll(|v| Message::Scrolled("select", v.absolute_offset().y))
                .width(Fill),
        )
        .style(card_bg)
        .padding(14)
        .width(width)
        .max_height(SELECT_MENU_MAX_H)
        .into()
    }

    /// Open the search collection menu, same frozen anchor (only one
    /// menu kind is ever open).
    pub(crate) fn open_coll_menu(&mut self, menu: CollectionMenu) {
        self.menu_at = self.cursor;
        self.coll_menu = Some(menu);
        self.context_menu = None;
        self.close_playlist_submenu();
        self.pl_menu = None;
        self.sort_menu = None;
        self.show_suggest = false;
    }

    /// Collection hover with the same modal freeze as track rows: rows
    /// behind scrims never highlight.
    pub(crate) fn coll_hover(&self, h: Option<String>) -> Option<String> {
        if self.modal_freeze() {
            None
        } else {
            h
        }
    }

    /// The collection menu's hit is still present (search hits live
    /// in results, home album groups live in the catalog: stale rows
    /// never open a scrim-less modal, and fresh results prune a dead
    /// search menu).
    pub(crate) fn coll_hit_present(&self, menu: &CollectionMenu) -> bool {
        match menu {
            CollectionMenu::Album(id) => self.results.albums.iter().any(|a| &a.id == id),
            CollectionMenu::Artist(id) => self.results.artists.iter().any(|a| &a.id == id),
            CollectionMenu::Playlist(id) => self.results.playlists.iter().any(|p| &p.id == id),
            CollectionMenu::HomeAlbum { album, artist } => self
                .catalog
                .values()
                .any(|t| t.album.as_deref() == Some(album.as_str()) && &t.artist == artist),
        }
    }

    /// Open a collection menu only when its hit is still present.
    pub(crate) fn open_coll_for(&mut self, menu: CollectionMenu) {
        if self.coll_hit_present(&menu) {
            self.show_suggest = false;
            self.open_coll_menu(menu);
        } else if matches!(menu, CollectionMenu::HomeAlbum { .. }) {
            self.status = self.language.album_gone();
        } else {
            self.status = self.language.result_gone();
        }
    }

    /// Drop the collection menu when fresh results no longer contain
    /// its hit (nothing would render, but the modal freeze would
    /// linger and swallow hovers).
    pub(crate) fn prune_coll_menu(&mut self) {
        if self
            .coll_menu
            .as_ref()
            .is_some_and(|m| !self.coll_hit_present(m))
        {
            self.coll_menu = None;
        }
    }

    /// Stored-row URL at a display size (shared threshold, one place).
    pub(crate) fn cover_url_for(url: &str, size: f32) -> String {
        if size >= Self::HIRES_MIN_PX {
            yutu_core::hires_thumb_url(url, 544)
        } else {
            Self::row_art_url(url)
        }
    }
}
