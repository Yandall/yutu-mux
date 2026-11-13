//! yutu-ui views_search: theme, search rows and heroes (split from lib.rs, no behavior change).

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
    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    /// Window title: the live song as "title · artist" (house
    /// separator, same as the Playing status), plain app name when the
    /// queue is empty. Iced queries it live, so no message or task is
    /// needed. Paused and stopped keep naming the song (the queue is
    /// still the session); only an empty queue falls back.
    pub fn window_title(&self) -> String {
        match self.queue.current() {
            Some(t) => {
                let title = t.title.trim();
                let artist = t.artist.trim();
                if title.is_empty() {
                    "yutu-mux".to_string()
                } else if artist.is_empty() {
                    title.to_string()
                } else {
                    format!("{title} · {artist}")
                }
            }
            None => "yutu-mux".to_string(),
        }
    }

    pub(crate) fn is_nav(&self, route: &Route) -> bool {
        std::mem::discriminant(&self.route) == std::mem::discriminant(route)
    }

    /// One compact track row: green play, title/artist, heart. All other
    /// actions live on detail screens: this killed the button-soup.
    pub(crate) fn track_row(&self, scope: &str, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            ACCENT
        } else {
            MUTED
        };
        let hover_id = track_hover_key(scope, &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str());
        mouse_area(
            container(
                row![
                    self.thumb(t),
                    button(btn_icon("play", 13.0, iced::Color::WHITE))
                        .style(accent_button)
                        .on_press(Message::PlayNow(t.id.clone()))
                        .padding(6),
                    Self::track_text_single(&t.title, &t.artist, 14, iced::Color::WHITE),
                    song_meta_text(t.plays.as_deref(), t.duration_secs),
                    button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                        .style(hover_row)
                        .on_press(Message::Like(t.id.clone()))
                        .padding(6),
                    button(btn_icon("ellipsis", 15.0, MUTED))
                        .style(hover_row)
                        .on_press(Message::OpenContext(t.id.clone()))
                        .padding(6),
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

    /// Playlist row: same controls as track_row, but art, text and play
    /// all start the playlist from this song on (the rest of the list
    /// queues behind it).
    pub(crate) fn playlist_track_row(
        &self,
        playlist_id: &str,
        t: &Track,
        index: usize,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row, text};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            ACCENT
        } else {
            MUTED
        };
        let hover_id = track_hover_key("playlist", &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str())
            || self.pl_hover == Some(index);
        let play_here = Message::PlayPlaylistAt {
            playlist_id: playlist_id.to_string(),
            index,
            track_id: t.id.clone(),
        };
        let play_here_text = Message::PlayPlaylistAt {
            playlist_id: playlist_id.to_string(),
            index,
            track_id: t.id.clone(),
        };
        mouse_area(
            container(
                row![
                    row_button(self.thumb(t))
                        .style(row_click)
                        .on_press(play_here)
                        .padding(2),
                    row_button(Self::btn_track_text_single(
                        &t.title,
                        &t.artist,
                        14,
                        iced::Color::WHITE
                    ))
                    .style(row_click)
                    .on_press(play_here_text)
                    .padding(3)
                    .width(Fill),
                    text(fmt_time(t.duration_secs as f64)).size(12).color(MUTED),
                    button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                        .style(hover_row)
                        .on_press(Message::Like(t.id.clone()))
                        .padding(6),
                    button(btn_icon("ellipsis", 15.0, MUTED))
                        .style(hover_row)
                        .on_press(Message::OpenContext(t.id.clone()))
                        .padding(6),
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

    /// Manual-sort playlist row: byte-for-byte the same visuals as
    /// [`Self::playlist_track_row`] at rest (same art, text, duration,
    /// heart, ellipsis, spacing), so switching sorts never restyles
    /// the list. One press path for the whole row (art, text, gaps,
    /// duration): plain visuals let the press reach the row area from
    /// anywhere, so a drag can start anywhere in the row; only the
    /// heart and ellipsis buttons own their press (hover-guarded, never
    /// arming). Arming is invisible and only goes live after the cursor
    /// moves past click slop: a plain click with no move just plays via
    /// the DragEnd snapshot instead of reordering. While live-dragging
    /// the origin is held by the floating ghost and the list shows a gap
    /// instead, so this always renders the plain row.
    pub(crate) fn playlist_manual_row(
        &self,
        playlist_id: &str,
        t: &Track,
        index: usize,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row, text};
        use iced::Fill;
        let pid = playlist_id.to_string();
        // No live-origin branch: while live-dragging the origin is held by
        // the floating ghost and the list shows a gap instead, so this
        // always renders the plain row.
        let heart_color = if self.liked.contains(&t.id) {
            ACCENT
        } else {
            MUTED
        };
        let hover_id = track_hover_key("playlist", &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str())
            || self.pl_hover == Some(index);
        // No inner play buttons: plain visuals let the press reach the
        // outer row area from art, text, gaps, or duration (only the
        // heart and ellipsis buttons own their press, guarded below). A
        // clean press/release plays via the DragEnd snapshot; a move to
        // a slot reorders instead. Same one-press-path contract as the
        // queue row.
        let heart_btn: iced::Element<'_, Message> =
            button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                .style(hover_row)
                .on_press(Message::Like(t.id.clone()))
                .padding(6)
                .into();
        let more_btn: iced::Element<'_, Message> = button(btn_icon("ellipsis", 15.0, MUTED))
            .style(hover_row)
            .on_press(Message::OpenContext(t.id.clone()))
            .padding(6)
            .into();
        let heart_cell: iced::Element<'_, Message> = mouse_area(heart_btn)
            .on_enter(Message::PlActionHover(Some(index)))
            .on_exit(Message::PlActionExit(index))
            .into();
        let more_cell: iced::Element<'_, Message> = mouse_area(more_btn)
            .on_enter(Message::PlActionHover(Some(index)))
            .on_exit(Message::PlActionExit(index))
            .into();
        mouse_area(
            container(
                row![
                    container(self.thumb(t)).padding(2),
                    container(Self::btn_track_text_single(
                        &t.title,
                        &t.artist,
                        14,
                        iced::Color::WHITE
                    ))
                    .padding(3)
                    .width(Fill),
                    text(fmt_time(t.duration_secs as f64)).size(12).color(MUTED),
                    heart_cell,
                    more_cell,
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
        .interaction(iced::mouse::Interaction::Pointer)
        .on_press(Message::PlDragStart {
            playlist_id: pid,
            index,
        })
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id.clone())))
        .on_exit(Message::TrackExit(hover_id))
        .into()
    }

    /// Edge insertion lane for manual playlist drags (slot 0 / slot len):
    /// transparent hover strips only, never a visible bar. The armed lane
    /// collapses to zero (the full-row gap marks the target instead).
    pub(crate) fn pl_drop_slot(&self, len: usize, k: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area};
        use iced::Fill;
        let armed = self.pl_effective_slot(len) == Some(k);
        let h = if armed { 0.0 } else { 10.0 };
        mouse_area(container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(h))
                .width(Fill),
        ))
        .on_enter(Message::PlDragSlot(k))
        .into()
    }

    /// Middle insertion lane for manual playlist drags: transparent 4px
    /// hover strip, collapsing to zero once armed like the queue lanes.
    pub(crate) fn pl_drop_divider(&self, len: usize, k: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area};
        use iced::Fill;
        let armed = self.pl_effective_slot(len) == Some(k);
        let h = if armed { 0.0 } else { 4.0 };
        mouse_area(container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(h))
                .width(Fill),
        ))
        .on_enter(Message::PlDragSlot(k))
        .into()
    }

    /// Compact search-popup row: smaller cover, single-line title/artist,
    /// play count + duration, no inline buttons. The whole row clicks to
    /// play; right-click opens the context menu. Fixed height plus a
    /// clipped text column so long names cut instead of painting over
    /// the play count.
    pub(crate) fn suggest_track_row(
        &self,
        t: &Track,
        selected: bool,
    ) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, container, mouse_area, row};
        use iced::Fill;
        let hover_id = track_hover_key("suggest", &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str());
        let play_id = t.id.clone();
        let menu_id = t.id.clone();
        let plays = t.plays.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let meta = match plays {
            Some(p) => format!("{p} · {}", fmt_time(t.duration_secs as f64)),
            None => fmt_time(t.duration_secs as f64),
        };
        mouse_area(
            container(
                row_button(
                    row![
                        self.thumb_sized(t, 32.0),
                        column![
                            btn_text(truncate_title(&t.title, 30), 13)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            btn_text(truncate_title(&t.artist, 28), 11)
                                .color(MUTED)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                        ]
                        .spacing(2)
                        .width(Fill)
                        .clip(true),
                        btn_text(meta, 11).color(MUTED).wrapping(Wrapping::None),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(4),
                )
                .style(row_click)
                .on_press(Message::PlayNow(play_id))
                .padding(0)
                .width(Fill),
            )
            .style(row_bg(hovered || selected))
            .width(Fill)
            .height(iced::Length::Fixed(SUGGEST_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(menu_id))
        .on_enter(Message::TrackHover(Some(hover_id.clone())))
        .on_exit(Message::TrackExit(hover_id))
        .into()
    }

    pub(crate) fn section<'a>(
        lang: Language,
        title: String,
        rows: Vec<iced::Element<'a, Message>>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, text};
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text(lang.section_empty()).size(12).color(MUTED).into()
        } else {
            column(rows).spacing(2).into()
        };
        column![text(title).size(19), list,].spacing(6).into()
    }

    /// Wrapping card strip: fixed-size cards flow onto the next line when
    /// the window is narrow instead of squashing, so every card keeps its
    /// standard size and height alignment.
    pub(crate) fn card_strip(cards: Vec<iced::Element<'_, Message>>) -> iced::Element<'_, Message> {
        use iced::Fill;
        iced::widget::Row::with_children(cards)
            .spacing(10)
            .width(Fill)
            .wrap()
            .vertical_spacing(10)
            .into()
    }

    /// Filter chip row for the search page (All/Songs/Albums/Artists).
    /// Chips filter the unified list in place; the selection survives
    /// new queries until the user picks another chip.
    pub(crate) fn search_filter_chips(&self) -> iced::Element<'_, Message> {
        use iced::widget::button;
        use iced::Fill;
        fn chip(
            label: String,
            f: SearchFilter,
            active: SearchFilter,
        ) -> iced::Element<'static, Message> {
            button(btn_text(label, 13))
                .style(nav_style(active == f))
                .on_press(Message::SetSearchFilter(f))
                .padding(8)
                .into()
        }
        let active = self.search_filter;
        iced::widget::Row::with_children(vec![
            chip(self.language.filter_all(), SearchFilter::All, active),
            chip(self.language.filter_songs(), SearchFilter::Songs, active),
            chip(self.language.filter_albums(), SearchFilter::Albums, active),
            chip(
                self.language.filter_artists(),
                SearchFilter::Artists,
                active,
            ),
            chip(
                self.language.filter_playlists(),
                SearchFilter::Playlists,
                active,
            ),
        ])
        .spacing(8)
        .width(Fill)
        .wrap()
        .into()
    }

    /// Loading state for the search page: text plus an animated
    /// ellipsis (the 500ms Tick repaints it, so no extra state).
    pub(crate) fn search_loading(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, text};
        use iced::Fill;
        let dots = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_millis() / 500 % 4) as usize)
            .unwrap_or(0);
        column![text(self.language.searching(dots)).size(15).color(MUTED),]
            .spacing(10)
            .width(Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .into()
    }

    /// Top-result hero for the unified search list: the head hit in a
    /// nice card with its type in the subtitle. Songs keep play/like
    /// controls; albums and artists open their detail page.
    pub(crate) fn search_hero(&self, hit: &SearchHit) -> iced::Element<'_, Message> {
        match hit {
            SearchHit::Song(t) => self.search_track_hero(t),
            SearchHit::Album(a) => self.search_album_hero(a),
            SearchHit::Artist(a) => self.search_artist_hero(a),
            SearchHit::Playlist(p) => self.search_playlist_hero(p),
        }
    }

    /// Hero track subtitle ("Song · artist · album · plays" or
    /// "Video · artist · plays"): empty parts drop out, so album-less
    /// hits never dangle a trailing separator. Videos carry no album;
    /// the view/play count survives here because the hero is the top
    /// result (rows show it on the right instead).
    pub(crate) fn hero_song_subtitle(
        is_video: bool,
        artist: &str,
        album: Option<&str>,
        plays: Option<&str>,
    ) -> String {
        let kind = if is_video { "Video" } else { "Song" };
        [kind, artist.trim()]
            .into_iter()
            .chain(album.map(str::trim))
            .chain(plays.map(str::trim))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// "Song" vs "Video" label for unified search rows.
    /// Card view/play count for a hero track id (the card is the only
    /// source when the recording has no shelf row): `Some` only when
    /// the card id matches, so unrelated heroes never borrow it.
    pub(crate) fn top_hit_plays(&self, id: &str) -> Option<&str> {
        self.all_results
            .top
            .as_ref()
            .and_then(|top| (top.id == id).then_some(top.plays.as_deref()).flatten())
    }

    /// Track hero: the previous top-result card, now with the result
    /// type in the subtitle (the mixed list has no sections). The
    /// view/play count joins the subtitle here (the card is the only
    /// source when the recording has no shelf row); rows show it on
    /// the right instead.
    pub(crate) fn search_track_hero(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            ACCENT
        } else {
            MUTED
        };
        mouse_area(
            container(
                row![
                    self.thumb_sized(t, 96.0),
                    column![
                        text(self.language.top_result()).size(12).color(MUTED),
                        text(t.title.clone()).size(20),
                        text(Self::hero_song_subtitle(
                            t.is_video,
                            &t.artist,
                            t.album.as_deref(),
                            t.plays.as_deref().or_else(|| self.top_hit_plays(&t.id))
                        ))
                        .size(13)
                        .color(MUTED),
                        row![
                            button(
                                row![
                                    btn_icon("play", 13.0, iced::Color::WHITE),
                                    btn_text(self.language.play_label(), 13),
                                ]
                                .spacing(6)
                                .align_y(iced::alignment::Vertical::Center)
                            )
                            .style(accent_button)
                            .on_press(Message::PlayNow(t.id.clone()))
                            .padding(8),
                            button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                                .style(hover_row)
                                .on_press(Message::Like(t.id.clone()))
                                .padding(8),
                            button(btn_icon("ellipsis", 15.0, MUTED))
                                .style(hover_row)
                                .on_press(Message::OpenContext(t.id.clone()))
                                .padding(8),
                        ]
                        .spacing(4)
                        .align_y(iced::alignment::Vertical::Center),
                    ]
                    .spacing(4)
                    .width(Fill),
                ]
                .spacing(14)
                .align_y(iced::alignment::Vertical::Center),
            )
            .style(card_bg)
            .padding(14),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .into()
    }

    /// Album hero: clickable cover/title open the album; Shuffle and
    /// Radio play the known catalog tracks (hidden when none are known).
    pub(crate) fn search_album_hero(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => self.language.album_sub_year(&a.artist, y),
            None => self.language.album_sub(&a.artist),
        };
        let tracks = self.album_tracks(&a.id);
        let actions: iced::Element<'_, Message> = if tracks.is_empty() {
            text("").size(1).into()
        } else {
            let seed = tracks.first().map(|t| t.id.clone()).unwrap_or_default();
            row![
                Self::shuffle_pill(
                    self.language.shuffle_pill_label(),
                    Message::PlayAlbumShuffled(a.id.clone())
                ),
                button(
                    row![
                        btn_icon("radio", 13.0, iced::Color::WHITE),
                        btn_text(self.language.radio_label(), 13),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(ghost_button)
                .on_press(Message::StartRadio(seed))
                .padding(8),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        };
        container(
            row![
                row_button(self.art_thumb(cover.as_deref(), 96.0))
                    .style(row_click)
                    .on_press(Message::OpenAlbum(a.id.clone()))
                    .padding(0),
                column![
                    text(self.language.top_result()).size(12).color(MUTED),
                    row_button(btn_text(a.title.clone(), 20))
                        .style(row_click)
                        .on_press(Message::OpenAlbum(a.id.clone()))
                        .padding(0)
                        .width(Fill),
                    text(sub).size(13).color(MUTED),
                    actions,
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(14)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(card_bg)
        .padding(14)
        .into()
    }

    /// Artist hero: clickable photo/name open the artist; Shuffle and
    /// Radio play the known catalog tracks (hidden when none are known).
    pub(crate) fn search_artist_hero(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let photo = a
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        let tracks = self.artist_tracks(&a.name);
        let actions: iced::Element<'_, Message> = if tracks.is_empty() {
            text("").size(1).into()
        } else {
            let seed = tracks.first().map(|t| t.id.clone()).unwrap_or_default();
            row![
                Self::shuffle_pill(
                    "SHUFFLE".into(),
                    Message::PlayArtistShuffled(a.name.clone())
                ),
                button(
                    row![
                        btn_icon("radio", 13.0, iced::Color::WHITE),
                        btn_text(self.language.radio_label(), 13),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(ghost_button)
                .on_press(Message::StartRadio(seed))
                .padding(8),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        };
        container(
            row![
                row_button(self.art_thumb(photo.as_deref(), 96.0))
                    .style(row_click)
                    .on_press(Message::OpenArtist(a.id.clone()))
                    .padding(0),
                column![
                    text(self.language.top_result()).size(12).color(MUTED),
                    row_button(btn_text(a.name.clone(), 20))
                        .style(row_click)
                        .on_press(Message::OpenArtist(a.id.clone()))
                        .padding(0)
                        .width(Fill),
                    text(self.language.artist_label()).size(13).color(MUTED),
                    actions,
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(14)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(card_bg)
        .padding(14)
        .into()
    }

    /// One row of the unified search list, dispatched by result type.
    pub(crate) fn search_hit_row(&self, hit: &SearchHit) -> iced::Element<'_, Message> {
        match hit {
            SearchHit::Song(t) => self.search_song_row(t),
            SearchHit::Album(a) => self.search_album_row(a),
            SearchHit::Artist(a) => self.search_artist_row(a),
            SearchHit::Playlist(p) => self.search_playlist_row(p),
        }
    }

    /// Track row for the unified search list: art with a hover play
    /// overlay (dim backdrop + white icon on top, whole art plays),
    /// type-led subtitle ("Song" or "Video" plus artist), and actions
    /// (like, more) that only appear on hover. Hidden actions keep
    /// same-size spacers so rows never jump. The liked heart stays
    /// visible: it is state, not just an action. Raw All results carry
    /// no durations, so a zero one renders blank.
    pub(crate) fn search_song_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row};
        use iced::Fill;
        let hover_id = track_hover_key("hits", &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str());
        let (heart, more) = self.song_actions(t, hovered);
        mouse_area(
            container(
                row![
                    self.art_play_button(t, hovered, Message::PlayNow(t.id.clone())),
                    Self::track_text_single(
                        &t.title,
                        &format!("{} · {}", self.language.track_kind(t.is_video), t.artist),
                        14,
                        iced::Color::WHITE
                    ),
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

    /// Album row for the unified search list: cover, type-led subtitle,
    /// one fixed row height, hover highlight + menu. Art and text open
    /// the album; the hover `…` (or right-click) opens its menu.
    pub(crate) fn search_album_row(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, container, mouse_area, row};
        use iced::Fill;
        let cover = self.album_row_cover(a);
        let sub = match a.year {
            Some(y) => self.language.album_sub_year(&a.artist, y),
            None => self.language.album_sub(&a.artist),
        };
        let hovered = self.hovered_album.as_deref() == Some(a.id.as_str());
        mouse_area(
            container(
                row![
                    row_button(self.art_thumb(cover.as_deref(), 40.0))
                        .style(row_click)
                        .on_press(Message::OpenAlbum(a.id.clone()))
                        .padding(0),
                    row_button(
                        column![
                            btn_text(truncate_title(&a.title, 45), 14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            btn_text(truncate_title(&sub, 40), 11)
                                .color(MUTED)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                        ]
                        .spacing(2)
                        .width(Fill)
                        .clip(true),
                    )
                    .style(row_click)
                    .on_press(Message::OpenAlbum(a.id.clone()))
                    .padding(3)
                    .width(Fill),
                    Self::coll_more(hovered, Message::OpenAlbumMenu(a.id.clone())),
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
        .on_right_press(Message::OpenAlbumMenu(a.id.clone()))
        .on_enter(Message::AlbumHover(Some(a.id.clone())))
        .on_exit(Message::AlbumExit(a.id.clone()))
        .into()
    }

    /// Artist row for the unified search list: photo, type-led subtitle,
    /// one fixed row height, hover highlight + menu. Photo and name open
    /// the artist; the hover `…` (or right-click) opens its menu.
    pub(crate) fn search_artist_row(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, container, mouse_area, row};
        use iced::Fill;
        // Per-hit key (not the name-merged photo): duplicate artist
        // names must never alias each other's art.
        let photo = a.thumbnail_url.as_deref().map(Self::row_art_url);
        let hovered = self.hovered_artist.as_deref() == Some(a.id.as_str());
        mouse_area(
            container(
                row![
                    row_button(self.art_thumb(photo.as_deref(), 40.0))
                        .style(row_click)
                        .on_press(Message::OpenArtist(a.id.clone()))
                        .padding(0),
                    row_button(
                        column![
                            btn_text(truncate_title(&a.name, 45), 14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            btn_text(self.language.artist_label(), 11)
                                .color(MUTED)
                                .width(Fill),
                        ]
                        .spacing(2)
                        .width(Fill)
                        .clip(true),
                    )
                    .style(row_click)
                    .on_press(Message::OpenArtist(a.id.clone()))
                    .padding(3)
                    .width(Fill),
                    Self::coll_more(hovered, Message::OpenArtistMenu(a.id.clone())),
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
        .on_right_press(Message::OpenArtistMenu(a.id.clone()))
        .on_enter(Message::ArtistHover(Some(a.id.clone())))
        .on_exit(Message::ArtistExit(a.id.clone()))
        .into()
    }

    /// Playlist art at hero/row sizes (same 544px key the warmer stages).
    pub(crate) fn playlist_art(&self, p: &PlaylistHit, size: f32) -> iced::Element<'_, Message> {
        let url = p
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        self.art_thumb(url.as_deref(), size)
    }

    /// Row-size playlist art (list rows render at 40px): small raster
    /// (row slots warm this key; hero and menu keep the 544px key).
    pub(crate) fn playlist_row_art(p: &PlaylistHit) -> Option<String> {
        p.thumbnail_url.as_deref().map(Self::row_art_url)
    }

    /// Playlist hero: art, type-led subtitle, Play + Shuffle pills.
    /// Clicking the art or title plays it too (there is no playlist
    /// detail page: playing is the open action).
    pub(crate) fn search_playlist_hero(&self, p: &PlaylistHit) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, row, text};
        use iced::Fill;
        let sub = self.language.playlist_sub_full(&p.author, &p.detail);
        container(
            row![
                row_button(self.playlist_art(p, 96.0))
                    .style(row_click)
                    .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                    .padding(0),
                column![
                    text(self.language.top_result()).size(12).color(MUTED),
                    row_button(btn_text(p.title.clone(), 20))
                        .style(row_click)
                        .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                        .padding(0)
                        .width(Fill),
                    text(sub).size(13).color(MUTED),
                    row![
                        Self::play_pill(
                            self.language.play_pill_label(),
                            Message::PlaySearchPlaylist(p.id.clone())
                        ),
                        Self::shuffle_pill(
                            "SHUFFLE".into(),
                            Message::PlaySearchPlaylistShuffled(p.id.clone())
                        ),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(14)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(card_bg)
        .padding(14)
        .into()
    }

    /// Playlist row for the unified search list: art, type-led subtitle,
    /// one fixed row height, hover highlight + menu. Art and text play
    /// the playlist; the hover `…` (or right-click) opens its menu.
    pub(crate) fn search_playlist_row(&self, p: &PlaylistHit) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, container, mouse_area, row};
        use iced::Fill;
        let sub = self.language.playlist_sub_full(&p.author, &p.detail);
        let hovered = self.hovered_search_playlist.as_deref() == Some(p.id.as_str());
        mouse_area(
            container(
                row![
                    row_button(self.art_thumb(Self::playlist_row_art(p).as_deref(), 40.0))
                        .style(row_click)
                        .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                        .padding(0),
                    row_button(
                        column![
                            btn_text(truncate_title(&p.title, 45), 14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            btn_text(truncate_title(&sub, 40), 11)
                                .color(MUTED)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                        ]
                        .spacing(2)
                        .width(Fill)
                        .clip(true),
                    )
                    .style(row_click)
                    .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                    .padding(3)
                    .width(Fill),
                    Self::coll_more(hovered, Message::OpenSearchPlaylistMenu(p.id.clone())),
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
        .on_right_press(Message::OpenSearchPlaylistMenu(p.id.clone()))
        .on_enter(Message::SearchPlaylistHover(Some(p.id.clone())))
        .on_exit(Message::SearchPlaylistExit(p.id.clone()))
        .into()
    }

    /// YTM-style sidebar: Home + Search nav, playlist search, inline
    /// new-playlist creation, and an expand/collapse rail.
    /// Queue/Recent live in the right panel; Settings lives in the header.
    /// Grouped history for the page: (section header, rows) plus the
    /// total match count, newest first. Only the `limit` newest
    /// matches resolve (clone) tracks; counting the rest is
    /// borrow-only, so scanning a multi-thousand log stays cheap.
    /// The log stays newest-first, so equal sections run together;
    /// empty groups never materialize.
    pub(crate) fn history_groups(
        &self,
        now: i64,
        limit: usize,
    ) -> (Vec<(String, Vec<Track>)>, usize) {
        let q = self.history_filter.to_lowercase();
        let mut groups: Vec<(HistorySection, Vec<Track>)> = Vec::new();
        let mut shown = 0usize;
        let mut total = 0usize;
        for e in &self.history_full {
            let Some(t) = self.catalog.get(&e.track_id) else {
                continue;
            };
            if !q.is_empty()
                && !t.title.to_lowercase().contains(&q)
                && !t.artist.to_lowercase().contains(&q)
            {
                continue;
            }
            total += 1;
            if shown >= limit {
                continue;
            }
            shown += 1;
            let section = history_section(e.played_at_unix, now);
            match groups.last_mut() {
                Some((s, rows)) if *s == section => rows.push(t.clone()),
                _ => groups.push((section, vec![t.clone()])),
            }
        }
        let groups = groups
            .into_iter()
            .map(|(s, rows)| (self.language.history_section(&s), rows))
            .collect();
        (groups, total)
    }
}
