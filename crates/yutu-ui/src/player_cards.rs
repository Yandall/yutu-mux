//! yutu-ui player_cards: player helpers, pills, row builders and cards (split from lib.rs, no behavior change).

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
    pub(crate) fn apply_volume(&self) {
        if let Some(a) = &self.audio {
            // Muted plays silence; the slider level is untouched so
            // unmuting restores it exactly.
            a.set_volume(if self.muted { 0.0 } else { self.volume });
        }
    }

    /// Backend clock (drives the progress bar + MPRIS position).
    pub(crate) fn audio_position(&self) -> f64 {
        self.audio
            .as_ref()
            .map(|a| a.position_secs())
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Backend duration when it knows better, else the track's metadata.
    pub(crate) fn audio_duration(&self) -> f64 {
        let fallback = self
            .queue
            .current()
            .map(|t| t.duration_secs as f64)
            .unwrap_or(0.0);
        self.audio
            .as_ref()
            .map(|a| a.duration_secs(fallback))
            .unwrap_or(fallback)
            .max(0.0)
    }

    /// Build the scrobble-submit task for a just-queued full play, if
    /// scrobbling is enabled, the play is eligible, and at least one target
    /// has credentials. Without the `submit` feature this is always none.
    pub(crate) fn maybe_scrobble_task(&mut self, track: &Track) -> iced::Task<Message> {
        if !(self.scrobble_enabled && should_scrobble(track.duration_secs, track.duration_secs)) {
            return iced::Task::none();
        }
        #[cfg(not(feature = "submit"))]
        {
            let _ = track;
            self.status_hint_no_scrobble_creds();
            iced::Task::none()
        }
        #[cfg(feature = "submit")]
        {
            #[allow(unused_imports)]
            use yutu_core::{build_listen_payload, build_scrobble_params, Scrobble};
            let now = now_unix();
            let mut tasks = Vec::new();
            if !self.scrobble_lb_token.trim().is_empty() {
                let payload =
                    build_listen_payload(&track.artist, &track.title, track.album.as_deref(), now);
                let token = self.scrobble_lb_token.clone();
                tasks.push(iced::Task::perform(
                    async move { yutu_core::submit_listenbrainz_listen(&payload, &token).await },
                    |result| Message::ScrobbleSubmitted {
                        target: "ListenBrainz".into(),
                        result,
                    },
                ));
            }
            if !(self.scrobble_fm_key.trim().is_empty()
                || self.scrobble_fm_secret.trim().is_empty()
                || self.scrobble_fm_sk.trim().is_empty())
            {
                let scrobble =
                    Scrobble::new(&track.artist, &track.title, now, track.album.as_deref());
                let params = build_scrobble_params(
                    &scrobble,
                    &self.scrobble_fm_key,
                    &self.scrobble_fm_sk,
                    &self.scrobble_fm_secret,
                );
                tasks.push(iced::Task::perform(
                    async move { yutu_core::submit_lastfm_scrobble(&params).await },
                    |result| Message::ScrobbleSubmitted {
                        target: "Last.fm".into(),
                        result,
                    },
                ));
            }
            if tasks.is_empty() {
                self.status_hint_no_scrobble_creds();
                iced::Task::none()
            } else {
                iced::Task::batch(tasks)
            }
        }
    }

    #[cfg(feature = "submit")]
    pub(crate) fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (add a ListenBrainz token or Last.fm keys in Settings)");
    }

    #[cfg(not(feature = "submit"))]
    pub(crate) fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (rebuild with `--features submit`)");
    }

    /// Big green PLAY pill for hero headers (Spotify-style).
    pub(crate) fn play_pill(label: String, msg: Message) -> iced::Element<'static, Message> {
        use iced::widget::{button, row, text};
        button(
            row![
                btn_icon("play", 16.0, iced::Color::WHITE),
                text(label).size(13).font(bold_font()),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(accent_button)
        .on_press(msg)
        .padding(9)
        .into()
    }

    /// Shuffle pill for hero headers: same red pill as play_pill with a
    /// shuffle glyph (search heroes).
    pub(crate) fn shuffle_pill(label: String, msg: Message) -> iced::Element<'static, Message> {
        use iced::widget::{button, row, text};
        button(
            row![
                btn_icon("shuffle", 16.0, iced::Color::WHITE),
                text(label).size(13).font(bold_font()),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(accent_button)
        .on_press(msg)
        .padding(9)
        .into()
    }

    /// Spotify-style numbered row: rank + art + play + title + time + actions.
    /// Row art wrapped in a click-to-play button: on hover the art
    /// dims behind a backdrop with a white play icon on top.
    pub(crate) fn art_play_button(
        &self,
        t: &Track,
        hovered: bool,
        play: Message,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{container, stack};
        let art: iced::Element<'_, Message> = if hovered {
            stack![
                self.thumb(t),
                container(
                    container(icon("play", 14.0, iced::Color::WHITE))
                        .align_x(iced::alignment::Horizontal::Center)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(iced::Length::Fixed(ROW_ART))
                        .height(iced::Length::Fixed(ROW_ART))
                )
                .style(art_dim)
                .width(iced::Length::Fixed(ROW_ART))
                .height(iced::Length::Fixed(ROW_ART))
            ]
            .into()
        } else {
            self.thumb(t)
        };
        row_button(art)
            .style(row_click)
            .on_press(play)
            .padding(0)
            .into()
    }

    /// Fixed box standing in for a hidden row action (rows never jump).
    pub(crate) fn action_spacer() -> iced::Element<'static, Message> {
        iced::widget::Space::new()
            .width(iced::Length::Fixed(ROW_ACTION_W))
            .height(iced::Length::Shrink)
            .into()
    }

    /// Like + more buttons for song rows: hidden until hover (spacers
    /// keep the width); the liked heart stays visible as state.
    pub(crate) fn song_actions(
        &self,
        t: &Track,
        hovered: bool,
    ) -> (iced::Element<'_, Message>, iced::Element<'_, Message>) {
        use iced::widget::button;
        let liked = self.liked.contains(&t.id);
        let heart_color = if liked { ACCENT } else { MUTED };
        let heart: iced::Element<'_, Message> = if hovered || liked {
            button(heart_icon(liked, 15.0, heart_color))
                .style(hover_row)
                .on_press(Message::Like(t.id.clone()))
                .padding(6)
                .into()
        } else {
            Self::action_spacer()
        };
        let more: iced::Element<'_, Message> = if hovered {
            button(btn_icon("ellipsis", 15.0, MUTED))
                .style(hover_row)
                .on_press(Message::OpenContext(t.id.clone()))
                .padding(6)
                .into()
        } else {
            Self::action_spacer()
        };
        (heart, more)
    }

    /// Hover `…` button for collection rows (opens the row's menu),
    /// or a spacer at rest.
    pub(crate) fn coll_more(hovered: bool, open: Message) -> iced::Element<'static, Message> {
        use iced::widget::button;
        if hovered {
            button(btn_icon("ellipsis", 15.0, MUTED))
                .style(hover_row)
                .on_press(open)
                .padding(6)
                .into()
        } else {
            Self::action_spacer()
        }
    }

    pub(crate) fn numbered_track_row(
        &self,
        t: &Track,
        n: usize,
        play: &RowPlay,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let (heart, more) = self.song_actions(t, hovered);
        let play_msg = play.message(&t.id, n);
        mouse_area(
            container(
                row![
                    text(format!("{n}")).size(13).color(MUTED).width(28),
                    self.art_play_button(t, hovered, play_msg),
                    Self::track_text_single(&t.title, &t.artist, 14, iced::Color::WHITE),
                    song_meta_text(t.plays.as_deref(), t.duration_secs),
                    heart,
                    more,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(8),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id.clone())))
        .on_exit(Message::TrackExit(hover_id))
        .into()
    }

    /// Album card with cover art (search grids, artist pages).
    /// Fixed standard size with single-line truncated labels: long titles
    /// crop instead of stretching the card, so every card aligns.
    pub(crate) fn album_card(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::column;
        use iced::widget::text::Wrapping;
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => format!("{} · {}", a.artist, y),
            None => a.artist.clone(),
        };
        row_button(
            column![
                self.art_thumb(cover.as_deref(), HOME_PICK_ART),
                column![
                    btn_text(truncate_title(&a.title, 20), 13)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                    btn_text(truncate_title(&sub, 22), 11)
                        .color(MUTED)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                ]
                .spacing(2)
                .width(Fill)
                .clip(true),
            ]
            .spacing(8)
            .padding(8)
            .width(Fill),
        )
        .style(card_button)
        .on_press(Message::OpenAlbum(a.id.clone()))
        .padding(0)
        .width(iced::Length::Fixed(HOME_CARD_W))
        .height(iced::Length::Fixed(HOME_CARD_H))
        .clip(true)
        .into()
    }

    /// Artist card with photo (search grids).
    /// Same fixed standard size as album cards and home picks.
    pub(crate) fn artist_card(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::column;
        use iced::widget::text::Wrapping;
        use iced::Fill;
        // 544 card key (grid sweeps warm it; the stager only stages
        // small row rasters).
        let photo = a
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        row_button(
            column![
                self.art_thumb(photo.as_deref(), HOME_PICK_ART),
                column![
                    btn_text(truncate_title(&a.name, 20), 13)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                    btn_text(self.language.artist_label(), 11)
                        .color(MUTED)
                        .width(Fill),
                ]
                .spacing(2)
                .width(Fill)
                .clip(true),
            ]
            .spacing(8)
            .padding(8)
            .width(Fill),
        )
        .style(card_button)
        .on_press(Message::OpenArtist(a.id.clone()))
        .padding(0)
        .width(iced::Length::Fixed(HOME_CARD_W))
        .height(iced::Length::Fixed(HOME_CARD_H))
        .clip(true)
        .into()
    }

    /// Cover source for an album: API art first, else the first known
    /// track's. Shared by the card-size and row-size keys below so the
    /// source never drifts.
    pub(crate) fn album_cover_source(&self, album: &Album) -> Option<String> {
        album.thumbnail_url.clone().or_else(|| {
            self.catalog
                .values()
                .find(|t| t.album.as_deref() == Some(album.title.as_str()))
                .and_then(|t| t.thumbnail_url.clone())
        })
    }

    /// Cover for an album: API art first, else the first known track's.
    pub(crate) fn album_cover(&self, album: &Album) -> Option<String> {
        // Hi-res for 140px+ cards and detail headers (hires passes
        // non-YTM hosts through unchanged, so tests stay green).
        self.album_cover_source(album)
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Row-size album cover (list rows render at 40px): same source,
    /// small raster (row slots warm this key, never the 544 one).
    pub(crate) fn album_row_cover(&self, album: &Album) -> Option<String> {
        self.album_cover_source(album)
            .map(|u| Self::row_art_url(&u))
    }

    /// Photo source for an artist: last search payload first, else
    /// catalog. Shared by the card-size and row-size keys below.
    pub(crate) fn artist_photo_source(&self, name: &str) -> Option<String> {
        self.results
            .artists
            .iter()
            .find(|a| a.name == name)
            .and_then(|a| a.thumbnail_url.clone())
            .or_else(|| {
                self.catalog
                    .values()
                    .find(|t| t.artist == name)
                    .and_then(|t| t.thumbnail_url.clone())
            })
    }

    /// Photo for an artist: last search payload first, else catalog.
    /// Hi-res for 140px+ cards and the detail header.
    pub(crate) fn artist_photo(&self, name: &str) -> Option<String> {
        self.artist_photo_source(name)
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Bulk-append without spamming history (one entry-free push per track),
    /// then play the first new track for real.
    /// Drop one id from the autoplay tail (queue ops absorb matches so
    /// the two lists never show the same song twice).
    pub(crate) fn absorb_autoplay(&mut self, id: &str) {
        self.autoplay.retain(|t| t.id != id);
    }
}
