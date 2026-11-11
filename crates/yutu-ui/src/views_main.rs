//! yutu-ui views_main: history, sidebar, player, drawer and center views (split from lib.rs, no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use dropdown::dropdown;
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
    /// Full listening history, grouped by recency with its own search.
    /// Rows render newest-first inside a render window ("Show more"
    /// pages in older plays); the header counts every match.
    pub(crate) fn history_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, text, text_input};
        use iced::Fill;
        let (groups, total) = self.history_groups(now_unix(), self.history_limit);
        let shown: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
        let head: iced::Element<'_, Message> = if self.history_filter.trim().is_empty() {
            text(self.language.history_plays(total))
                .size(12)
                .color(MUTED)
                .into()
        } else {
            text(self.language.history_matches(total))
                .size(12)
                .color(MUTED)
                .into()
        };
        let mut body = column![
            text(self.language.nav_history()).size(22),
            head,
            text_input(
                self.language.history_filter_placeholder(),
                &self.history_filter
            )
            .style(rounded_input)
            .on_input(Message::HistoryFilterChanged)
            .width(Fill)
            .padding(10),
        ]
        .spacing(10);
        if groups.is_empty() {
            let empty = if self.history_filter.trim().is_empty() {
                "No plays yet. Press play and they will show up here."
            } else {
                "No matches. Try a different spelling."
            };
            body = body.push(text(empty).size(12).color(MUTED));
        }
        // Flat row index across sections (hover is per-row, like the
        // queue: repeat plays share a track id).
        let mut next_row = 0usize;
        for (header, rows) in groups {
            let mut items: Vec<iced::Element<'_, Message>> = Vec::new();
            for t in rows {
                let row = self.history_row(&t, next_row);
                next_row += 1;
                items.push(row);
            }
            body = body.push(column![text(header).size(19), column(items).spacing(2),].spacing(6));
        }
        if total > shown {
            body = body.push(
                button(btn_text(self.language.show_more_older(total - shown), 12))
                    .style(ghost_button)
                    .on_press(Message::HistoryMore)
                    .padding(8),
            );
        }
        body.into()
    }

    /// History row: same click-to-play row as Recent, with the track
    /// length in the duration slot. Hover is per-row index (like the
    /// queue): repeat plays share a track id, so id-keyed hover would
    /// light them all up at once.
    pub(crate) fn history_row(&self, t: &Track, index: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_history == Some(index);
        let menu_id = t.id.clone();
        mouse_area(
            container(
                row![
                    self.thumb(t),
                    row_button(Self::btn_track_text_single(
                        &t.title,
                        &t.artist,
                        12,
                        iced::Color::WHITE
                    ))
                    .style(row_click)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(3)
                    .width(Fill),
                    text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center)
                .padding(8),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(menu_id))
        .on_enter(Message::HistoryHover(Some(index)))
        .on_exit(Message::HistoryExit(index))
        .into()
    }

    pub(crate) fn sidebar_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text, text_input};
        use iced::Fill;
        let pin_btn: iced::Element<'_, Message> = button(btn_icon(
            "panel-left",
            16.0,
            if self.sidebar_pinned { ACCENT } else { MUTED },
        ))
        .style(hover_row)
        .on_press(Message::ToggleSidebarPin)
        .padding(8)
        .into();
        if self.sidebar_w < 150.0 {
            return container(
                column![
                    button(btn_icon("house", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::Home)))
                        .on_press(Message::Navigate(Route::Home))
                        .padding(10),
                    button(btn_icon("search", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::Search)))
                        .on_press(Message::Navigate(Route::Search))
                        .padding(10),
                    button(btn_icon("list-music", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::History)))
                        .on_press(Message::Navigate(Route::History))
                        .padding(10),
                ]
                .spacing(2),
            )
            .style(sidebar_bg)
            .padding(8)
            .width(iced::Length::Fixed(self.sidebar_w))
            .height(Fill)
            .into();
        }
        let q = self.sidebar_filter.to_lowercase();
        let new_name = self.new_playlist_name.clone();
        let rows: Vec<iced::Element<'_, Message>> = self
            .all_playlists()
            .into_iter()
            .filter(|pl| q.is_empty() || pl.title.to_lowercase().contains(&q))
            .take(20)
            .map(|pl| {
                let art: iced::Element<'_, Message> = pl
                    .tracks
                    .first()
                    .and_then(|t| self.find_track(&t.id))
                    .map(|t| self.thumb(&t))
                    .unwrap_or_else(|| {
                        container(icon("music", 16.0, MUTED))
                            .style(card_bg)
                            .padding(0)
                            .width(iced::Length::Fixed(40.0))
                            .height(iced::Length::Fixed(40.0))
                            .align_x(iced::alignment::Horizontal::Center)
                            .align_y(iced::alignment::Vertical::Center)
                            .into()
                    });
                // Hover reveals a trailing play button; right-click
                // opens the playlist menu.
                let show_play = self.hovered_playlist.as_deref() == Some(pl.id.as_str())
                    && !pl.tracks.is_empty();
                let trailing: iced::Element<'_, Message> = if show_play {
                    button(btn_icon("play", 14.0, iced::Color::WHITE))
                        .style(accent_button)
                        .on_press(Message::PlayPlaylist(pl.id.clone()))
                        .padding(8)
                        .into()
                } else {
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(30.0))
                        .into()
                };
                let pl_id = pl.id.clone();
                let pl_hover = pl.id.clone();
                mouse_area(
                    row_button(
                        row![
                            art,
                            column![
                                btn_text(pl.title.clone(), 13),
                                btn_text(self.language.playlist_sub(pl.tracks.len()), 11)
                                    .color(MUTED),
                            ]
                            .spacing(1)
                            .width(Fill),
                            trailing,
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenPlaylist(pl.id.clone()))
                    .padding(8)
                    .width(Fill),
                )
                .on_right_press(Message::OpenPlaylistMenu(pl_id))
                .on_enter(Message::PlaylistHover(Some(pl_hover.clone())))
                .on_exit(Message::PlaylistExit(pl_hover))
                .into()
            })
            .collect();
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text(if self.all_playlists().is_empty() {
                self.language.no_playlists_below()
            } else {
                self.language.no_playlists_match()
            })
            .size(12)
            .color(MUTED)
            .into()
        } else {
            column(rows).spacing(2).into()
        };
        container(
            column![
                row![
                    logo_mark(34.0),
                    text("Yutu Mux")
                        .size(22)
                        .color(iced::Color::WHITE)
                        .width(Fill),
                    self.sync_spinner(),
                    pin_btn,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
                column![
                    button(
                        row![
                            btn_icon("house", 14.0, iced::Color::WHITE),
                            btn_text(self.language.nav_home(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(nav_style(self.is_nav(&Route::Home)))
                    .on_press(Message::Navigate(Route::Home))
                    .padding(8)
                    .width(Fill),
                    button(
                        row![
                            btn_icon("search", 14.0, iced::Color::WHITE),
                            btn_text(self.language.nav_search(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(nav_style(self.is_nav(&Route::Search)))
                    .on_press(Message::Navigate(Route::Search))
                    .padding(8)
                    .width(Fill),
                    button(
                        row![
                            btn_icon("list-music", 14.0, iced::Color::WHITE),
                            btn_text(self.language.nav_history(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(nav_style(self.is_nav(&Route::History)))
                    .on_press(Message::Navigate(Route::History))
                    .padding(8)
                    .width(Fill),
                ]
                .spacing(2),
                text_input(
                    self.language.sidebar_filter_placeholder(),
                    &self.sidebar_filter
                )
                .style(rounded_input)
                .on_input(Message::SidebarFilterChanged)
                .width(Fill)
                .padding(10),
                list,
                row![
                    text_input(
                        self.language.new_playlist_placeholder(),
                        &self.new_playlist_name
                    )
                    .style(rounded_input)
                    .on_input(Message::NewPlaylistNameChanged)
                    .width(Fill)
                    .padding(10),
                    button(btn_icon("plus", 16.0, iced::Color::WHITE))
                        .style(accent_button)
                        .on_press(Message::CreatePlaylist(new_name))
                        .padding(7),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(10),
        )
        .style(sidebar_bg)
        .padding(12)
        .width(iced::Length::Fixed(self.sidebar_w))
        .height(Fill)
        .into()
    }

    /// Queue row (plain visuals only): while live-dragging the origin is
    /// held by the floating ghost and the list shows a gap instead, so the
    /// row itself never renders a held state and siblings only shift for
    /// the gap. Armed-but-unmoved presses render the normal row, so a
    /// click never flickers the layout.
    pub(crate) fn queue_row(&self, i: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let items = self.queue.items();
        let Some(t) = items.get(i) else {
            return iced::widget::Space::new().width(Fill).into();
        };
        let pos = self.queue.position();
        let is_current = Some(i) == pos;
        let menu_id = t.id.clone();
        let remove_id = t.id.clone();
        let hovered = self.hovered_queue == Some(i);
        // Width-stable hover reveal (see `hover_remove_slot`): the
        // idle slot is the same × box, transparent and inert, so the
        // press dynamics are unchanged (mouse_area only exists armed).
        let remove_btn: iced::Element<'_, Message> = {
            let slot =
                Self::hover_remove_slot(hovered.then_some(Message::RemoveFromQueue(remove_id)));
            if hovered {
                mouse_area(slot)
                    .on_enter(Message::RemoveHover(Some(i)))
                    .on_exit(Message::RemoveExit(i))
                    .into()
            } else {
                slot
            }
        };
        let row_content = row![
            container(self.thumb(t)).padding(2),
            container(Self::track_text(
                &t.title,
                &t.artist,
                12,
                if is_current {
                    ACCENT
                } else {
                    iced::Color::WHITE
                },
            ))
            .padding(1)
            .width(Fill),
            container(text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED)).padding(3),
            remove_btn,
        ]
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center)
        .padding(6);
        // Fixed height: the title may wrap to two lines, anything more
        // clips instead of stretching the row. Padding budget matters:
        // a 2-line title + artist is 45.75px, so the row (6) + text
        // cell (1) padding must leave 48px inside the 60px row.
        let lifted: iced::Element<'_, Message> = container(row_content)
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(QUEUE_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center)
            .into();
        // One press path for the whole row: plain visuals let the press
        // reach this area from text, art, or gaps (only the tiny remove
        // button owns its press). A clean press/release plays the row
        // via the DragEnd snapshot; a move to a slot reorders instead.
        // The whole row is clickable, so it votes the pointing hand
        // (inner content votes None, the remove × votes Pointer itself).
        mouse_area(lifted)
            .interaction(iced::mouse::Interaction::Pointer)
            .on_press(Message::DragStart(i))
            .on_right_press(Message::OpenContext(menu_id))
            .on_enter(Message::PanelHover(i))
            .on_exit(Message::QueueExit(i))
            .into()
    }

    /// Edge insertion lane: slot 0 before the first row or slot len
    /// after the last. Transparent hover strips only, never a visible bar:
    /// the armed lane collapses to zero (the full-row gap marks the target
    /// instead) and idle lanes stay thin and invisible, so rows never
    /// restyle mid-drag.
    pub(crate) fn drop_slot(&self, k: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area};
        use iced::Fill;
        let armed = self.effective_drag_slot() == Some(k);
        let h = if armed { 0.0 } else { 10.0 };
        mouse_area(container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(h))
                .width(Fill),
        ))
        .on_enter(Message::DragSlot(k))
        .into()
    }

    /// Middle insertion lane: slot `k` (1..=len-1) as a transparent 4px
    /// hover strip. The armed lane collapses to zero since the gap box
    /// already marks the target; at rest there are no lanes at all.
    pub(crate) fn drop_divider(&self, k: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area};
        use iced::Fill;
        let armed = self.effective_drag_slot() == Some(k);
        let h = if armed { 0.0 } else { 4.0 };
        mouse_area(container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(h))
                .width(Fill),
        ))
        .on_enter(Message::DragSlot(k))
        .into()
    }

    /// Floating held row while live-dragging (YouTube-style): the dragged
    /// song as art + text on the elevated card, gliding per-pixel at the
    /// cursor (Space-offset idiom shared with the context menu, grab offset
    /// preserved from the press so it never jumps on grab). No handlers
    /// anywhere inside, so lane hovers and release-anywhere still land on
    /// the list underneath. Sits right above the base (modal layers cover
    /// it) and hides whenever a menu or the suggest popup is open.
    pub(crate) fn drag_ghost_layer(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{column, container, row, text};
        use iced::Fill;
        if self.context_menu.is_some()
            || self.pl_menu.is_some()
            || self.coll_menu.is_some()
            || self.sort_menu.is_some()
            || self.show_suggest
        {
            return None;
        }
        // Only one drag is ever live; the queue panel survives navigation
        // while playlist drags are scoped to one list.
        let (ghost, gx, gy): (iced::Element<'_, Message>, f32, f32) =
            if let Some(drag) = self.drag.filter(|d| d.live) {
                let t = self.queue.items().get(drag.from)?.clone();
                let gw = (self.right_w - 20.0).clamp(160.0, 600.0);
                let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                let press_x = self.press_snap.as_ref().and_then(|s| s.pos.map(|p| p.x));
                let (gx, gy) = drag_ghost_pos(
                    self.cursor,
                    self.window,
                    gw,
                    QUEUE_ROW_H,
                    press_x,
                    ww - self.right_w + 10.0,
                )?;
                let ghost: iced::Element<'_, Message> = container(
                    row![
                        self.thumb(&t),
                        Self::track_text(&t.title, &t.artist, 12, iced::Color::WHITE),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(6),
                )
                .style(card_bg)
                .width(iced::Length::Fixed(gw))
                .height(iced::Length::Fixed(QUEUE_ROW_H))
                .clip(true)
                .align_y(iced::alignment::Vertical::Center)
                .into();
                (ghost, gx, gy)
            } else {
                let drag = self.pl_drag.filter(|d| d.live)?;
                let pid = self.pl_drag_id.clone()?;
                let p = self.find_playlist(&pid)?;
                let ordered = self.playlist_ordered_tracks(&p);
                let t = ordered.get(drag.from)?.clone();
                let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                let gw = (ww - self.sidebar_w - self.right_w - 64.0).clamp(240.0, 760.0);
                let press_x = self.pl_press_snap.as_ref().and_then(|s| s.pos.map(|p| p.x));
                let (gx, gy) = drag_ghost_pos(
                    self.cursor,
                    self.window,
                    gw,
                    TRACK_ROW_H,
                    press_x,
                    self.sidebar_w + 24.0,
                )?;
                let ghost: iced::Element<'_, Message> = container(
                    row![
                        container(self.thumb(&t)).padding(2),
                        container(Self::track_text_single(
                            &t.title,
                            &t.artist,
                            14,
                            iced::Color::WHITE
                        ))
                        .padding(3)
                        .width(Fill),
                        text(fmt_time(t.duration_secs as f64)).size(12).color(MUTED),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(8),
                )
                .style(card_bg)
                .width(iced::Length::Fixed(gw))
                .height(iced::Length::Fixed(TRACK_ROW_H))
                .clip(true)
                .align_y(iced::alignment::Vertical::Center)
                .into();
                (ghost, gx, gy)
            };
        Some(
            container(column![
                iced::widget::Space::new().height(iced::Length::Fixed(gy)),
                row![
                    iced::widget::Space::new().width(iced::Length::Fixed(gx)),
                    ghost,
                ],
            ])
            .width(Fill)
            .height(Fill)
            .into(),
        )
    }

    pub(crate) fn right_panel_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
        use iced::Fill;
        let current = self.queue.current().cloned();
        let now: iced::Element<'_, Message> = match &current {
            Some(t) => column![
                text(self.language.now_playing()).size(13).color(MUTED),
                row![
                    self.thumb(t),
                    column![
                        text(t.title.clone()).size(15),
                        text(t.artist.clone()).size(12).color(MUTED),
                    ]
                    .spacing(2)
                    .width(Fill),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(4)
            .into(),
            None => column![
                text(self.language.now_playing()).size(13).color(MUTED),
                text(self.language.nothing_queued()).size(14).color(MUTED),
            ]
            .spacing(4)
            .into(),
        };
        // Live-drag YouTube-style list: the origin row is held by the
        // floating ghost, so the list shows every other row plus one
        // full-row gap at the commit target (siblings shift to make room).
        // Armed-but-unmoved presses render the rest layout, so clicks never
        // shift rows. At rest rows render plain with dividers.
        let n = self.queue.len();
        let from = self.drag.map(|d| d.from).unwrap_or(0);
        let live = self.dragging() && from < n;
        let mut upcoming: Vec<iced::Element<'_, Message>> =
            Vec::with_capacity(if live { 2 * n + 1 } else { n });
        if live {
            let slot = self.effective_drag_slot();
            let to = slot.and_then(|s| slot_commit(n, from, s)).unwrap_or(from);
            let (seq, lanes) = drag_display_order(n, from, to);
            // Constant full-row gap while live: teleporting the gap must
            // never change the list height (an eased grow/shrink here
            // collapses the rows below on every latch around the origin).
            // Only the position snaps, one row at a time.
            let gap_h = QUEUE_ROW_H;
            upcoming.push(self.drop_slot(lanes[0]));
            for (p, item) in seq.iter().enumerate() {
                match item {
                    Some(k) => upcoming.push(self.queue_row(*k)),
                    None => upcoming.push(drag_gap_box(gap_h)),
                }
                if p + 1 < n {
                    upcoming.push(self.drop_divider(lanes[p + 1]));
                } else {
                    upcoming.push(self.drop_slot(lanes[p + 1]));
                }
            }
        } else {
            for k in 0..n {
                upcoming.push(self.queue_row(k));
            }
        }
        let tab_queue = button(btn_text(self.language.queue_tab(), 12))
            .style(nav_style(self.right_tab == RightTab::Queue))
            .on_press(Message::SetRightTab(RightTab::Queue))
            .padding(8);
        let tab_recent = button(btn_text(self.language.recent_tab(), 12))
            .style(nav_style(self.right_tab == RightTab::Recent))
            .on_press(Message::SetRightTab(RightTab::Recent))
            .padding(8);
        let auto_mini: Vec<iced::Element<'_, Message>> = self
            .autoplay
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let menu_id = t.id.clone();
                let hovered = self.hovered_auto == Some(i);
                // Width-stable hover reveal (see `hover_remove_slot`).
                let remove_btn: iced::Element<'_, Message> =
                    Self::hover_remove_slot(hovered.then_some(Message::RemoveAutoplay(i)));
                mouse_area(
                    container(
                        row![
                            self.thumb(t),
                            row_button(Self::btn_track_text_single(
                                &t.title,
                                &t.artist,
                                12,
                                iced::Color::WHITE
                            ))
                            .style(row_click)
                            .on_press(Message::PlayAutoplay(i))
                            .padding(3)
                            .width(Fill),
                            text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                            remove_btn,
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(8),
                    )
                    .style(row_bg(hovered))
                    .width(Fill)
                    .height(iced::Length::Fixed(TRACK_ROW_H))
                    .clip(true)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .on_right_press(Message::OpenContext(menu_id))
                .on_enter(Message::AutoHover(Some(i)))
                .on_exit(Message::AutoExit(i))
                .into()
            })
            .collect();
        let auto_block: iced::Element<'_, Message> = if auto_mini.is_empty() {
            text("").size(1).into()
        } else {
            column![
                text(self.language.autoplay()).size(13).color(MUTED),
                column(auto_mini).spacing(0),
            ]
            .spacing(2)
            .into()
        };
        let list: iced::Element<'_, Message> = match self.right_tab {
            RightTab::Queue => column![
                row![
                    text(self.language.up_next())
                        .size(13)
                        .color(MUTED)
                        .width(Fill),
                    button(btn_text(self.language.clear_queue(self.queue.len()), 12))
                        .style(ghost_button)
                        .on_press(Message::QueueClear)
                        .padding(8),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
                scrollable(
                    column![
                        column(if live {
                            upcoming
                        } else {
                            with_dividers(upcoming)
                        })
                        .spacing(0),
                        auto_block,
                    ]
                    .spacing(2)
                    .padding(iced::Padding {
                        right: 14.0,
                        ..iced::Padding::ZERO
                    })
                )
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("queue")))
                .on_scroll(|v| Message::Scrolled("queue", v.absolute_offset().y))
                .height(Fill),
            ]
            .spacing(4)
            .into(),
            RightTab::Recent => {
                let mut seen = std::collections::HashSet::new();
                let mut recent: Vec<Track> = Vec::new();
                for id in self.history.iter().rev() {
                    if recent.len() >= 9 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            recent.push(t);
                        }
                    }
                }
                if recent.is_empty() {
                    text(self.language.no_recent_plays())
                        .size(12)
                        .color(MUTED)
                        .into()
                } else {
                    column(
                        recent
                            .iter()
                            .map(|t| {
                                let menu_id = t.id.clone();
                                let hover_id = t.id.clone();
                                let hovered = self.hovered_recent.as_deref() == Some(t.id.as_str());
                                mouse_area(
                                    container(
                                        row![
                                            self.thumb(t),
                                            row_button(Self::btn_track_text_single(
                                                &t.title,
                                                &t.artist,
                                                12,
                                                iced::Color::WHITE
                                            ))
                                            .style(row_click)
                                            .on_press(Message::PlayNow(t.id.clone()))
                                            .padding(3)
                                            .width(Fill),
                                            text(fmt_time(t.duration_secs as f64))
                                                .size(11)
                                                .color(MUTED),
                                        ]
                                        .spacing(6)
                                        .align_y(iced::alignment::Vertical::Center)
                                        .padding(8),
                                    )
                                    .style(row_bg(hovered))
                                    .width(Fill)
                                    .height(iced::Length::Fixed(TRACK_ROW_H))
                                    .clip(true)
                                    .align_y(iced::alignment::Vertical::Center),
                                )
                                .on_right_press(Message::OpenContext(menu_id))
                                .on_enter(Message::RecentHover(Some(hover_id.clone())))
                                .on_exit(Message::RecentExit(hover_id))
                                .into()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(0)
                    .into()
                }
            }
        };
        let pin_btn: iced::Element<'_, Message> = button(btn_icon(
            "panel-right",
            16.0,
            if self.right_pinned { ACCENT } else { MUTED },
        ))
        .style(hover_row)
        .on_press(Message::ToggleRightPin)
        .padding(6)
        .into();
        if self.right_w < 140.0 {
            // Collapsed rail fills the available height: a queue window
            // anchored on the live song plus autoplay with what is left.
            // Same jump targets as the expanded rows.
            let total = self.collapsed_rail_capacity();
            let n = self.queue.items().len();
            let m = self.autoplay.len();
            let pos = self.queue.position();
            let (q_start, q_end) = collapsed_rail_window(pos, n, total);
            let q_shown = q_end.saturating_sub(q_start);
            let auto_shown = m.min(total.saturating_sub(q_shown));
            let mut rail: Vec<iced::Element<'_, Message>> = Vec::new();
            for idx in q_start..q_end {
                let Some(t) = self.queue.items().get(idx) else {
                    continue;
                };
                let is_current = pos == Some(idx);
                let btn: iced::Element<'_, Message> =
                    row_button(self.thumb_sized(t, RAIL_THUMB_PX))
                        .style(hover_row)
                        .on_press(Message::QueueJump(idx))
                        .padding(2)
                        .into();
                rail.push(
                    container(btn)
                        .style(rail_thumb_bg(is_current))
                        .padding(1)
                        .into(),
                );
            }
            if q_shown > 0 && auto_shown > 0 {
                rail.push(
                    container(
                        iced::widget::Space::new()
                            .height(iced::Length::Fixed(1.0))
                            .width(Fill),
                    )
                    .style(card_bg)
                    .padding(0)
                    .into(),
                );
            }
            for (i, t) in self.autoplay.iter().enumerate().take(auto_shown) {
                let btn: iced::Element<'_, Message> =
                    row_button(self.thumb_sized(t, RAIL_THUMB_PX))
                        .style(hover_row)
                        .on_press(Message::PlayAutoplay(i))
                        .padding(2)
                        .into();
                rail.push(container(btn).style(rail_thumb_bg(false)).padding(1).into());
            }
            return container(
                column(rail)
                    .spacing(6)
                    .align_x(iced::alignment::Horizontal::Center),
            )
            .style(sidebar_bg)
            .padding(8)
            .width(iced::Length::Fixed(self.right_w))
            .height(Fill)
            .into();
        }
        container(column![row![pin_btn, tab_queue, tab_recent].spacing(4), now, list,].spacing(8))
            .style(sidebar_bg)
            .padding(12)
            .width(iced::Length::Fixed(self.right_w))
            .height(Fill)
            .into()
    }

    /// Mute toggle beside the volume slider: speaker glyph, crossed
    /// when muted. Level untouched, so unmuting restores exactly.
    pub(crate) fn mute_button(&self) -> iced::Element<'_, Message> {
        use iced::widget::button;
        button(ctrl_box(btn_icon(
            if self.muted { "volume-x" } else { "volume" },
            14.0,
            MUTED,
        )))
        .style(hover_row)
        .on_press(Message::ToggleMute)
        .padding(6)
        .into()
    }

    pub(crate) fn player_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{
            button, column, container, mouse_area, row, text, text::Wrapping, tooltip,
        };
        use iced::Fill;
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let mode = player_layout_mode(ww);
        let current = self.queue.current().cloned();
        let loading = self.loading_track.is_some();
        let title: String = if loading {
            self.language.loading_ellipsis()
        } else {
            current
                .as_ref()
                .map(|t| t.title.clone())
                .unwrap_or_default()
        };
        let artist: String = current
            .as_ref()
            .map(|t| t.artist.clone())
            .unwrap_or_default();
        let play_icon = if self.paused { "play" } else { "pause" };
        let duration = self.audio_duration();
        let slider_max = duration.max(1.0);
        let shown_pos = self
            .seek_drag
            .map(f64::from)
            .unwrap_or(self.position_secs)
            .clamp(0.0, slider_max);
        // Compact mode tightens everything but keeps every control.
        let tight = mode == PlayerLayout::Compact;
        let art_size = if tight { 32.0 } else { 40.0 };
        let title_size = if tight { 13 } else { 14 };
        let (left_part, center_part) = if tight { (3, 4) } else { (2, 3) };
        let transport_gap = if tight { 4.0 } else { 6.0 };
        let pad_side = if tight { 6 } else { 8 };
        let pad_play = if tight { 8 } else { 10 };
        let volume_w = if tight { 80 } else { 120 };
        let main_gap = if tight { 8.0 } else { 12.0 };
        let like_button: iced::Element<'_, Message> = match &current {
            Some(t) => {
                let heart_color = if self.liked.contains(&t.id) {
                    ACCENT
                } else {
                    MUTED
                };
                button(ctrl_box(heart_icon(
                    self.liked.contains(&t.id),
                    15.0,
                    heart_color,
                )))
                .style(hover_row)
                .on_press(Message::Like(t.id.clone()))
                .padding(6)
                .into()
            }
            None => text("").size(1).into(),
        };
        let shuffle_button: iced::Element<'_, Message> = if self.queue.shuffle {
            button(ctrl_box(btn_icon("shuffle", 14.0, iced::Color::WHITE)))
                .style(accent_button)
                .on_press(Message::ToggleShuffle)
                .padding(pad_side)
                .into()
        } else {
            button(ctrl_box(btn_icon("shuffle", 14.0, MUTED)))
                .style(hover_row)
                .on_press(Message::ToggleShuffle)
                .padding(pad_side)
                .into()
        };
        let repeat_icon = match self.queue.repeat {
            yutu_player::RepeatMode::Off => "repeat",
            yutu_player::RepeatMode::All => "repeat",
            yutu_player::RepeatMode::One => "repeat-1",
        };
        let repeat_active = !matches!(self.queue.repeat, yutu_player::RepeatMode::Off);
        // Stacked minimal player: every control survives, reflowed into
        // rows so nothing clips below 500px.
        if mode == PlayerLayout::Minimal {
            let info_art: iced::Element<'_, Message> = match &current {
                Some(t) => self.thumb_sized(t, 32.0),
                None => container(icon("music", 20.0, MUTED))
                    .style(card_bg)
                    .padding(10)
                    .into(),
            };
            let info_like: iced::Element<'_, Message> = match &current {
                Some(t) => {
                    let heart_color = if self.liked.contains(&t.id) {
                        ACCENT
                    } else {
                        MUTED
                    };
                    button(ctrl_box(heart_icon(
                        self.liked.contains(&t.id),
                        15.0,
                        heart_color,
                    )))
                    .style(hover_row)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(6)
                    .into()
                }
                None => text("").size(1).into(),
            };
            let drawer_btn: iced::Element<'_, Message> = button(ctrl_box(btn_icon(
                if self.drawer_open {
                    "chevron-down"
                } else {
                    "chevron-up"
                },
                14.0,
                iced::Color::WHITE,
            )))
            .style(hover_row)
            .on_press(Message::ToggleDrawer)
            .padding(7)
            .into();
            let info = row![
                info_art,
                column![
                    text(title.clone())
                        .size(13)
                        .width(Fill)
                        .wrapping(Wrapping::WordOrGlyph),
                    text(artist.clone()).size(12).color(MUTED),
                ]
                .spacing(1)
                .width(Fill),
                info_like,
                drawer_btn,
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center);
            let transport = row![
                {
                    let shuffle_btn: iced::Element<'_, Message> = if self.queue.shuffle {
                        button(ctrl_box(btn_icon("shuffle", 14.0, iced::Color::WHITE)))
                            .style(accent_button)
                            .on_press(Message::ToggleShuffle)
                            .padding(6)
                            .into()
                    } else {
                        button(ctrl_box(btn_icon("shuffle", 14.0, MUTED)))
                            .style(hover_row)
                            .on_press(Message::ToggleShuffle)
                            .padding(6)
                            .into()
                    };
                    shuffle_btn
                },
                button(ctrl_box(btn_icon("skip-back", 18.0, iced::Color::WHITE)))
                    .style(hover_row)
                    .on_press(Message::Prev)
                    .padding(6),
                button(ctrl_box(btn_icon(play_icon, 18.0, iced::Color::WHITE)))
                    .style(accent_button)
                    .on_press(Message::TogglePause)
                    .padding(8),
                button(ctrl_box(btn_icon("skip-forward", 18.0, iced::Color::WHITE)))
                    .style(hover_row)
                    .on_press(Message::Next)
                    .padding(6),
                repeat_button(repeat_icon, repeat_active, 6),
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center);
            let transport_centered: iced::Element<'_, Message> = container(transport)
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .into();
            let mut bar_col = iced::widget::Column::new().spacing(6);
            if let Some(status) = self.status_row() {
                bar_col = bar_col.push(status);
            }
            return container(
                bar_col
                    .push(info)
                    .push(transport_centered)
                    .push(
                        row![
                            text(fmt_time(shown_pos)).size(11).color(MUTED),
                            seek_bar::seek_bar(shown_pos as f32, slider_max as f32),
                            text(fmt_time(duration)).size(11).color(MUTED),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .push(
                        row![
                            self.mute_button(),
                            pointing_slider(0.0..=1.0, self.volume, Message::SetVolume)
                                .step(0.01_f32)
                                .style(themed_slider)
                                .width(Fill),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    ),
            )
            .style(player_bg)
            .padding(10)
            .into();
        }
        let mut bar_col = iced::widget::Column::new().spacing(4);
        if let Some(status) = self.status_row() {
            bar_col = bar_col.push(status);
        }
        container(
            bar_col.push(
                row![
                    {
                        let art: iced::Element<'_, Message> = match &current {
                            Some(t) => self.thumb_sized(t, art_size),
                            None => container(icon("music", 20.0, MUTED))
                                .style(card_bg)
                                .padding(10)
                                .into(),
                        };
                        let artist_el: iced::Element<'_, Message> = match &current {
                            Some(t) => button(btn_text(artist, 12).color(MUTED))
                                .style(row_click)
                                .on_press(Message::OpenTrackArtist(t.id.clone()))
                                .padding(0)
                                .into(),
                            None => text(artist).size(12).color(MUTED).into(),
                        };
                        let left = row![
                            art,
                            column![
                                text(title)
                                    .size(title_size)
                                    .width(Fill)
                                    .wrapping(Wrapping::WordOrGlyph),
                                artist_el,
                            ]
                            .spacing(1)
                            .width(Fill),
                            like_button,
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center);
                        let boxed: iced::Element<'_, Message> = match &current {
                            Some(t) => mouse_area(left)
                                .on_right_press(Message::OpenContext(t.id.clone()))
                                .into(),
                            None => left.into(),
                        };
                        let wrapped: iced::Element<'_, Message> = container(boxed)
                            .width(iced::Length::FillPortion(left_part))
                            .into();
                        wrapped
                    },
                    column![
                        row![
                            shuffle_button,
                            button(ctrl_box(btn_icon("skip-back", 18.0, iced::Color::WHITE)))
                                .style(hover_row)
                                .on_press(Message::Prev)
                                .padding(pad_side),
                            button(ctrl_box(btn_icon(play_icon, 18.0, iced::Color::WHITE)))
                                .style(accent_button)
                                .on_press(Message::TogglePause)
                                .padding(pad_play),
                            button(ctrl_box(btn_icon("skip-forward", 18.0, iced::Color::WHITE)))
                                .style(hover_row)
                                .on_press(Message::Next)
                                .padding(pad_side),
                            repeat_button(repeat_icon, repeat_active, pad_side),
                        ]
                        .spacing(transport_gap)
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            text(fmt_time(shown_pos)).size(11).color(MUTED),
                            seek_bar::seek_bar(shown_pos as f32, slider_max as f32),
                            text(fmt_time(duration)).size(11).color(MUTED),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    ]
                    .spacing(2)
                    .align_x(iced::alignment::Horizontal::Center)
                    .width(iced::Length::FillPortion(center_part)),
                    row![
                        iced::widget::Space::new().width(Fill),
                        self.mute_button(),
                        tooltip(
                            pointing_slider(0.0..=1.0, self.volume, Message::SetVolume)
                                .step(0.01_f32)
                                .style(themed_slider)
                                .width(volume_w),
                            text(if self.muted {
                                self.language.volume_muted(self.volume * 100.0)
                            } else {
                                self.language.volume(self.volume * 100.0)
                            })
                            .size(12),
                            tooltip::Position::Top,
                        )
                        .gap(8),
                        button(ctrl_box(btn_icon(
                            if self.drawer_open {
                                "chevron-down"
                            } else {
                                "chevron-up"
                            },
                            14.0,
                            iced::Color::WHITE
                        )))
                        .style(hover_row)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center)
                    .width(iced::Length::FillPortion(2)),
                ]
                .spacing(main_gap)
                .align_y(iced::alignment::Vertical::Center),
            ),
        )
        .style(player_bg)
        .padding(10)
        .into()
    }

    /// Highlighted verse index into the VISIBLE (non-empty) synced
    /// lines for `id`, or None without synced lyrics / before the
    /// first timestamp. Same filter + `line_at` call as
    /// [`lyrics_content`]: the follow geometry must agree with the
    /// highlight, so both read through here.
    pub(crate) fn lyric_cursor_at(&self, id: &str) -> Option<usize> {
        let (_, cached) = self.lyrics_display_entry(id)?;
        // Instrumentals render a static label (no verses to follow),
        // even when stray synced lines exist.
        if cached.synced.is_empty() || cached.instrumental {
            return None;
        }
        // +1ms truncation guard, mirroring the view.
        let pos_ms = (self.position_secs * 1000.0 + 1.0) as u64;
        let visible: Vec<yutu_core::LyricLine> = cached
            .synced
            .iter()
            .filter(|l| !l.text.trim().is_empty())
            .cloned()
            .collect();
        yutu_core::line_at(&visible, pos_ms)
    }

    /// Estimated height of one lyric row: wrapped visual lines at the
    /// lyrics column width times the sized line height, plus button
    /// padding. Wrapping is approximate (average advance), so callers
    /// treat the result as a centering hint, never truth.
    pub(crate) fn lyric_row_h(text_len: usize, current: bool, col_w: f32) -> f32 {
        let (size, lh) = if current { (19.0, 1.4) } else { (15.0, 1.5) };
        let avail = (col_w - LYRIC_ROW_CHROME).max(120.0);
        let per_line = (avail / (size * 0.47)).max(8.0) as usize;
        let lines = text_len.max(1).div_ceil(per_line);
        lines as f32 * size * lh + LYRIC_BTN_PAD
    }

    /// Absolute drawer-scroll y that centers verse `at` (index into
    /// the visible lines): hero estimate, exact pitch walk over the
    /// cached rows (stanza gaps included, current row at its larger
    /// size), viewport centering, floored at zero. Takes the cursor
    /// instead of recomputing it (one clone per tick, not two).
    pub(crate) fn lyric_follow_y(&self, id: &str, at: usize) -> Option<f32> {
        let (_, cached) = self.lyrics_display_entry(id)?;
        if cached.synced.is_empty() || cached.instrumental {
            return None;
        }
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let wh = self.window.map(|s| s.height).unwrap_or(800.0);
        // Same width formula as the drawer lyrics column.
        let col_w = (ww - self.sidebar_w - self.right_w - 96.0).clamp(280.0, 560.0);
        // Window minus header/player chrome (approximate: only shifts
        // the center, never loses the verse).
        let viewport = (wh - LYRIC_VIEWPORT_PAD).max(320.0);
        let mut y = LYRIC_HERO_H + LYRIC_LIST_GAP;
        let mut vi = 0usize;
        let mut target = None;
        for l in &cached.synced {
            if l.text.trim().is_empty() {
                y += LYRIC_GAP_H + LYRIC_ROW_SPACING;
                continue;
            }
            let current = vi == at;
            let h = Self::lyric_row_h(l.text.chars().count(), current, col_w);
            if current {
                target = Some(y + h / 2.0 - viewport / 2.0);
            }
            y += h + LYRIC_ROW_SPACING;
            vi += 1;
        }
        target.map(|t| t.max(0.0))
    }

    /// Lyrics follow step (drawer only): on verse or track change with
    /// follow engaged, record the verse target (the Animate ticker
    /// glides there, see [`lyrics_glide_task`]). Snaps fire once per
    /// change, never per tick. Manual scrolls disengage (proximity
    /// re-engages); closed drawer and unsynced lyrics are no-ops.
    /// With animations off, snaps instantly instead of gliding.
    pub(crate) fn lyric_follow_task(&mut self) -> iced::Task<Message> {
        let Some(t) = self.queue.current() else {
            return iced::Task::none();
        };
        if !self.drawer_open {
            return iced::Task::none();
        }
        let id = t.id.clone();
        let Some(at) = self.lyric_cursor_at(&id) else {
            return iced::Task::none();
        };
        let changed_track = self
            .lyrics_follow_at
            .as_ref()
            .is_none_or(|(fid, _)| fid != &id);
        if changed_track {
            self.lyrics_follow = true;
        }
        if !self.lyrics_follow {
            // Proximity resume: the user scrolled back near the verse,
            // so follow picks up silently (already centered: no snap).
            if let Some(y) = self.lyric_follow_y(&id, at) {
                if (self.drawer_scroll - y).abs() <= LYRIC_RESUME_PX {
                    self.lyrics_follow = true;
                }
            }
            self.lyrics_follow_at = Some((id, at));
            return iced::Task::none();
        }
        if !changed_track && self.lyrics_follow_at == Some((id.clone(), at)) {
            return iced::Task::none();
        }
        self.lyrics_follow_at = Some((id.clone(), at));
        match self.lyric_follow_y(&id, at) {
            Some(y) => {
                self.lyrics_target = y;
                if self.animations_enabled {
                    // The ticker glides there (smooth follow).
                    iced::Task::none()
                } else {
                    self.lyrics_cmds = [y, f32::NAN, f32::NAN];
                    Self::lyrics_scroll_task(y)
                }
            }
            None => iced::Task::none(),
        }
    }

    /// Command the drawer lyrics scrollable to an absolute y (single
    /// jump; the glide chains these per tick).
    pub(crate) fn lyrics_scroll_task(y: f32) -> iced::Task<Message> {
        iced::advanced::widget::operate(iced::advanced::widget::operation::scrollable::scroll_to::<
            Message,
        >(
            iced::widget::Id::new(LYRICS_SCROLL_ID),
            iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                x: None,
                y: Some(y),
            },
        ))
    }

    /// One follow-glide step: ease the drawer toward the verse target
    /// (ease-out: 25% of the remaining gap per tick, 2px floor) and
    /// command the scrollable there. `None` when settled, closed,
    /// disengaged, or unfollowed. The commanded offset is recorded so
    /// its echo never trips the manual-takeover check (snaps land
    /// exactly: only real user scrolls differ).
    pub(crate) fn lyrics_glide_task(&mut self) -> Option<iced::Task<Message>> {
        if !self.drawer_open || !self.lyrics_follow || self.lyrics_follow_at.is_none() {
            return None;
        }
        let gap = self.lyrics_target - self.drawer_scroll;
        if gap.abs() <= 1.0 {
            return None;
        }
        let step = (gap.abs() * 0.25).max(2.0);
        let y = ease_toward(self.drawer_scroll, self.lyrics_target, step);
        self.lyrics_cmds = [y, self.lyrics_cmds[0], self.lyrics_cmds[1]];
        Some(Self::lyrics_scroll_task(y))
    }

    /// True while a follow glide is converging (runs the ticker; off
    /// with animations disabled, where snaps are instant instead).
    pub(crate) fn lyrics_gliding(&self) -> bool {
        self.animations_enabled
            && self.drawer_open
            && self.lyrics_follow
            && self.lyrics_follow_at.is_some()
            && (self.drawer_scroll - self.lyrics_target).abs() > 1.0
    }

    /// Shared synced-lyrics body (lyrics page + Now Playing drawer):
    /// loading / instrumental / empty / plain states included.
    ///
    /// Drawer follow geometry (see [`lyric_follow_y`]) mirrors the row
    /// construction below exactly (gaps, pitches, current-row size):
    /// keep the two in sync when this layout changes.
    pub(crate) fn lyrics_content(&self, id: &str) -> iced::Element<'_, Message> {
        use iced::widget::{column, row, text};
        match self.lyrics_display_entry(id) {
            Some((_, cached)) if cached.instrumental => text(self.language.instrumental())
                .size(14)
                .color(MUTED)
                .into(),
            Some((_, cached)) if !cached.synced.is_empty() => {
                // Highlight reads through the shared cursor so the
                // follow geometry can never disagree with it.
                let at = self.lyric_cursor_at(id);
                let mut rows: Vec<iced::Element<'_, Message>> = Vec::new();
                let mut vi = 0usize;
                for l in &cached.synced {
                    if l.text.trim().is_empty() {
                        rows.push(
                            iced::widget::Space::new()
                                .height(iced::Length::Fixed(10.0))
                                .width(iced::Fill)
                                .into(),
                        );
                        continue;
                    }
                    let is_current = Some(vi) == at;
                    vi += 1;
                    let ms = l.start_ms;
                    let line: iced::Element<'_, Message> = if is_current {
                        row![
                            btn_icon("music", 15.0, ACCENT),
                            text(l.text.clone())
                                .size(19)
                                .line_height(iced::widget::text::LineHeight::Relative(1.4),)
                                .font(bold_font())
                                .color(ACCENT),
                        ]
                        .spacing(10)
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                    } else {
                        row![
                            iced::widget::Space::new().width(iced::Length::Fixed(25.0)),
                            btn_text(l.text.clone(), 15)
                                .line_height(iced::widget::text::LineHeight::Relative(1.5),)
                                .color(iced::Color::from_rgb8(0xD6, 0xD6, 0xD6)),
                        ]
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                    };
                    let row_el: iced::Element<'_, Message> = row_button(line)
                        .style(hover_row)
                        .on_press(Message::SeekToMs(ms))
                        .padding(4)
                        .width(iced::Fill)
                        .into();
                    rows.push(row_el);
                }
                column(rows).spacing(6).into()
            }
            Some((_, cached)) => column(
                cached
                    .plain
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| {
                        text(l.to_string())
                            .size(15)
                            .line_height(iced::widget::text::LineHeight::Relative(1.6))
                            .into()
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(6)
            .into(),
            None if self.lyrics_fetch_pending(id) => {
                text(self.language.loading_lyrics(self.lyrics_provider.label()))
                    .color(MUTED)
                    .into()
            }
            None => text(self.language.no_lyrics()).color(MUTED).into(),
        }
    }

    /// Lyrics provider `< >` stepper (drawer + lyrics page): switches
    /// the default provider (instant when preloaded: both providers
    /// fetch up front at song start) and pins the viewed track to
    /// strict display (no automatic fallback afterwards). The label
    /// names the provider actually on screen (default while
    /// loading/empty, fallback source when only it has lyrics), so it
    /// never credits the wrong one.
    pub(crate) fn lyrics_provider_stepper(&self, track_id: &str) -> iced::Element<'_, Message> {
        use iced::widget::{button, row, text};
        let label = self.lyrics_shown_provider(track_id).label();
        let items: Vec<iced::Element<'_, Message>> = vec![
            button(btn_icon("chevron-left", 14.0, MUTED))
                .style(hover_row)
                .on_press(Message::CycleLyricsProvider(-1))
                .padding(4)
                .into(),
            text(label.to_string())
                .size(12)
                .color(MUTED)
                .width(iced::Length::Fixed(LYRICS_PROVIDER_LABEL_W))
                .align_x(iced::alignment::Horizontal::Center)
                .into(),
            button(btn_icon("chevron-right", 14.0, MUTED))
                .style(hover_row)
                .on_press(Message::CycleLyricsProvider(1))
                .padding(4)
                .into(),
        ];
        row(items)
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    /// Now Playing drawer content: big art,
    /// centered song info, then the synced lyrics home. No queue lists
    /// (the sidebar owns those), no Lyrics/Radio buttons, no timings.
    pub(crate) fn drawer_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
        use iced::Fill;
        let Some(t) = self.queue.current().cloned() else {
            return container(column![text(self.language.nothing_playing()).size(22),])
                .padding(24)
                .into();
        };
        let art_url = self.hero_art_for(&t);
        let hero: iced::Element<'_, Message> = column![
            self.art_thumb(art_url.as_deref(), 300.0),
            text(t.title.clone()).size(24),
            text(t.artist.clone()).size(15).color(MUTED),
            text(t.album.clone().unwrap_or_else(|| "Single".into()))
                .size(12)
                .color(MUTED),
            row![button(heart_icon(
                self.liked.contains(&t.id),
                16.0,
                if self.liked.contains(&t.id) {
                    ACCENT
                } else {
                    MUTED
                }
            ))
            .style(hover_row)
            .on_press(Message::Like(t.id.clone()))
            .padding(7),]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center),
        ]
        .spacing(8)
        .align_x(iced::alignment::Horizontal::Center)
        .into();
        // Sticky compact header once the lyrics scroll past 200px.
        // Sticky reuses the already-fetched 300px hero key on purpose
        // (zero marginal fetch), even though other 40px rows use raw.
        let sticky: iced::Element<'_, Message> = if self.drawer_scroll > 200.0 {
            container(
                row![
                    self.art_thumb(art_url.as_deref(), 40.0),
                    column![
                        text(t.title.clone()).size(14),
                        text(t.artist.clone()).size(12).color(MUTED),
                    ]
                    .spacing(1)
                    .width(Fill),
                ]
                .spacing(10)
                .align_y(iced::alignment::Vertical::Center),
            )
            .style(card_bg)
            .padding(8)
            .into()
        } else {
            text("").size(1).into()
        };
        let stepper = self.lyrics_provider_stepper(&t.id);
        let body: iced::Element<'_, Message> = column![
            sticky,
            scrollable(
                column![
                    container(hero)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    container(stepper)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    // Narrow centered lyrics column that never overflows
                    // the available center space (window minus sidebars).
                    container(self.lyrics_content(&t.id)).width(iced::Length::Fixed(
                        (self.window.map(|s| s.width).unwrap_or(1280.0)
                            - self.sidebar_w
                            - self.right_w
                            - 96.0)
                            .clamp(280.0, 560.0),
                    )),
                ]
                .spacing(16)
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .padding(iced::Padding {
                    bottom: 40.0,
                    ..iced::Padding::ZERO
                })
            )
            .height(Fill)
            .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("drawer")))
            .id(iced::widget::Id::new(LYRICS_SCROLL_ID))
            .on_scroll(|v| Message::DrawerScrolled(v.absolute_offset().y)),
        ]
        .spacing(8)
        .into();
        // Right-click anywhere opens this song's menu.
        let track_id = t.id.clone();
        mouse_area(body)
            .on_right_press(Message::OpenContext(track_id))
            .into()
    }

    /// Outer width of a nav arrow button (22px icon + 2x10 padding).
    pub(crate) const NAV_BTN_W: f32 = 42.0;
    /// Outer width of the settings gear (14px icon + 2x6 padding).
    pub(crate) const SETTINGS_W: f32 = 26.0;

    /// Window-relative search pill width (capped, floored).
    pub(crate) fn search_width(&self) -> f32 {
        self.window
            .map(|s| {
                (s.width * 0.42)
                    .clamp(200.0, 640.0)
                    .min((s.width - 220.0).max(200.0))
            })
            .unwrap_or(560.0)
    }

    /// Pill left edge in window coords (base pad + sidebar + gaps + navs).
    pub(crate) fn search_pill_left(&self) -> f32 {
        8.0 + self.sidebar_w + 8.0 + Self::NAV_BTN_W + 4.0 + Self::NAV_BTN_W + 4.0
    }

    /// Pill geometry shared by the header pill, the suggest popup and
    /// the scrim hole (same left + width everywhere, so popup edges
    /// match the input exactly and only the input stays clickable).
    pub(crate) fn search_pill_geom(&self) -> (f32, f32) {
        let search_w = self.search_width();
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let chrome = Self::NAV_BTN_W + 4.0 + Self::NAV_BTN_W + 4.0 + Self::SETTINGS_W + 4.0;
        let gaps = if self.right_target() != 0.0 {
            16.0
        } else {
            8.0
        };
        let center_avail = ww - 16.0 - self.sidebar_w - gaps - self.right_target() - chrome - 16.0;
        let w = if center_avail < search_w {
            center_avail.max(140.0)
        } else {
            search_w
        };
        let left = self.search_pill_left().clamp(0.0, ww.max(0.0));
        (left, w.min(ww).max(0.0))
    }

    /// Global header (nav + search + settings), shared by the center and
    /// the drawer-open state so the search bar never vanishes.
    pub(crate) fn header_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, row, text, text_input};
        use iced::Fill;
        let can_back = !self.nav_back.is_empty();
        let can_forward = !self.nav_forward.is_empty();
        let back_btn: iced::Element<'_, Message> = if can_back {
            button(btn_icon("chevron-left", 22.0, iced::Color::WHITE))
                .style(nav_arrow)
                .on_press(Message::NavBack)
                .padding(10)
                .into()
        } else {
            button(btn_icon("chevron-left", 22.0, MUTED))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let fwd_btn: iced::Element<'_, Message> = if can_forward {
            button(btn_icon("chevron-right", 22.0, iced::Color::WHITE))
                .style(nav_arrow)
                .on_press(Message::NavForward)
                .padding(10)
                .into()
        } else {
            button(btn_icon("chevron-right", 22.0, MUTED))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let clear_btn: iced::Element<'_, Message> = if self.search_query.is_empty() {
            text("").size(1).into()
        } else {
            button(btn_icon("x", 13.0, MUTED))
                .style(hover_row)
                .on_press(Message::HomeSearchChanged(String::new()))
                .padding(6)
                .into()
        };
        // Submit magnifier: same message as Enter in the input, so
        // clicking it and pressing Enter always do the same thing.
        let go_btn: iced::Element<'_, Message> =
            button(btn_icon("search", 14.0, iced::Color::WHITE))
                .style(hover_row)
                .on_press(Message::SuggestPick)
                .padding(6)
                .into();
        // Pill width comes from the shared geometry (same number the
        // popup and scrim use, so edges always match).
        let (_, pill_w) = self.search_pill_geom();
        let pill_w = iced::Length::Fixed(pill_w);
        let pill: iced::Element<'_, Message> = container(
            row![
                icon("search", 14.0, MUTED),
                text_input(self.language.search_placeholder(), &self.search_query)
                    .style(search_inner_input)
                    .id("yutu-search")
                    .on_input(Message::HomeSearchChanged)
                    .on_submit(Message::SuggestPick)
                    .width(Fill)
                    .padding(10),
                clear_btn,
                go_btn,
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(search_pill)
        .padding(iced::Padding {
            left: 8.0,
            right: 4.0,
            ..iced::Padding::ZERO
        })
        .width(pill_w)
        .into();
        let gear: iced::Element<'_, Message> =
            button(btn_icon("settings", 14.0, iced::Color::WHITE))
                .style(hover_row)
                .on_press(Message::Navigate(Route::Settings))
                .padding(6)
                .into();
        // Fixed pill always (shared geometry with the popup): Fill
        // space docks the gear to the header right edge.
        let mut h = iced::widget::Row::with_children(vec![back_btn, fwd_btn, pill]);
        let spacer: iced::Element<'_, Message> = iced::widget::Space::new().width(Fill).into();
        h = h.push(spacer);
        h = h.push(gear);
        h.spacing(4)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    /// Home shelf card: big square cover with single-line title and
    /// subtitle below (same fixed size as the Jump-back-in picks).
    /// Click behavior comes from the caller (open a playlist, play an
    /// album); no right-click menu (the id would be a guess, and menus
    /// live on the rows and detail pages instead).
    /// Home shelf card: art + title + subtitle open on left-click,
    /// right-click opens the card menu (playlist or album actions).
    /// Cards follow the app-wide pattern: no separate play button,
    /// extra actions live in the right-click menu only.
    pub(crate) fn home_card(
        &self,
        art: &Track,
        title: String,
        sub: String,
        press: Message,
        right: Message,
    ) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, mouse_area};
        use iced::Fill;
        mouse_area(
            row_button(
                column![
                    self.thumb_hero(art, HOME_PICK_ART),
                    column![
                        btn_text(truncate_title(&title, 20), 13)
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
            .on_press(press)
            .padding(0)
            .width(iced::Length::Fixed(HOME_CARD_W))
            .height(iced::Length::Fixed(HOME_CARD_H))
            .clip(true),
        )
        .on_right_press(right)
        .into()
    }

    /// Home song row: the art (hover play overlay) and the
    /// title/artist both play the song, with no separate play button.
    /// This is the app-wide row pattern: rows play on art/title
    /// click, extra actions live in the right-click menu. Id-keyed
    /// hover (quick picks and more-from never repeat a track).
    pub(crate) fn home_song_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let menu_id = t.id.clone();
        let hover_id = t.id.clone();
        mouse_area(
            container(
                row![
                    self.art_play_button(t, hovered, Message::PlayNow(t.id.clone())),
                    row_button(Self::btn_track_text_single(
                        &t.title,
                        &t.artist,
                        14,
                        iced::Color::WHITE
                    ))
                    .style(row_click)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(3)
                    .width(Fill),
                    text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                ]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center)
                .padding(8),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(menu_id))
        .on_enter(Message::TrackHover(Some(hover_id.clone())))
        .on_exit(Message::TrackExit(hover_id))
        .into()
    }

    pub(crate) fn center_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{
            button, checkbox, column, container, mouse_area, row, scrollable, text, toggler,
        };
        use iced::Fill;
        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let picks: Vec<Track> = self.home_pick_tracks();
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    column![
                        text(self.language.home_empty_hint()).color(MUTED),
                        text(self.language.home_empty_hint2()).size(12).color(MUTED),
                    ]
                    .spacing(8)
                    .into()
                } else {
                    // Horizontal pick strip: big square cover on top with
                    // single-line title + artist below. Fixed card size so
                    // long names truncate instead of stretching.
                    let cards: Vec<iced::Element<'_, Message>> = picks
                        .iter()
                        .map(|t| {
                            let menu_id = t.id.clone();
                            mouse_area(
                                row_button(
                                    column![
                                        self.thumb_hero(t, HOME_PICK_ART),
                                        column![
                                            btn_text(truncate_title(&t.title, 20), 13)
                                                .width(Fill)
                                                .wrapping(Wrapping::None),
                                            btn_text(truncate_title(&t.artist, 22), 11)
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
                                .on_press(Message::PlayNow(t.id.clone()))
                                .padding(0)
                                .width(iced::Length::Fixed(HOME_CARD_W))
                                .height(iced::Length::Fixed(HOME_CARD_H))
                                .clip(true),
                            )
                            .on_right_press(Message::OpenContext(menu_id))
                            .into()
                        })
                        .collect();
                    scrollable(row(cards).spacing(10))
                        .direction(scrollable::Direction::Horizontal(slim_bar()))
                        .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("home")))
                        .on_scroll(|v| Message::Scrolled("home", v.absolute_offset().y))
                        .width(Fill)
                        .height(iced::Length::Fixed(HOME_CARD_H + HOME_STRIP_GAP))
                        .into()
                };
                // Bottom breathing room: the floating scrollbar sits on
                // the strip's bottom edge, clear of the next section.
                let grid: iced::Element<'_, Message> = container(grid)
                    .width(Fill)
                    .padding(iced::Padding {
                        bottom: 10.0,
                        ..iced::Padding::ZERO
                    })
                    .into();
                // Home never shows search results: typing filters the
                // suggest popup, and results live on the Search page.
                let weekly_n = self.home_shelves.weekly.len();
                let forgotten_n = self.home_shelves.forgotten.len();
                // YouTube-style shelves, all local: heavy rotation, the
                // user's own playlists, catalog albums by plays, and the
                // top artist's other songs. Empty shelves hide (a fresh
                // install shows the hint plus Made-for-you instead).
                // Shelves read the Tick/nav-refreshed memo (ranking scans
                // the whole catalog, so views must not recompute it).
                let quick = &self.home_shelves.quick;
                let quick_section: iced::Element<'_, Message> = if quick.is_empty() {
                    text("").size(1).into()
                } else {
                    let left: Vec<iced::Element<'_, Message>> = quick
                        .iter()
                        .step_by(2)
                        .map(|t| self.home_song_row(t))
                        .collect();
                    let right: Vec<iced::Element<'_, Message>> = quick
                        .iter()
                        .skip(1)
                        .step_by(2)
                        .map(|t| self.home_song_row(t))
                        .collect();
                    column![
                        row![
                            text(self.language.quick_picks()).size(19).width(Fill),
                            button(
                                row![
                                    btn_icon("play", 13.0, iced::Color::WHITE),
                                    btn_text(self.language.play_all(), 13)
                                ]
                                .spacing(6)
                                .align_y(iced::alignment::Vertical::Center)
                            )
                            .style(accent_button)
                            .on_press(Message::PlayQuickPicks)
                            .padding(7),
                        ]
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            column(left).spacing(2).width(Fill),
                            column(right).spacing(2).width(Fill),
                        ]
                        .spacing(8),
                    ]
                    .spacing(10)
                    .into()
                };
                let lib_cards: Vec<iced::Element<'_, Message>> = self
                    .home_strip_playlists()
                    .into_iter()
                    .map(|p| {
                        let art = p
                            .tracks
                            .first()
                            .and_then(|t| self.find_track(&t.id))
                            .unwrap_or_else(|| p.tracks[0].clone());
                        let sub = self.language.playlist_sub(p.tracks.len());
                        self.home_card(
                            &art,
                            p.title.clone(),
                            sub,
                            Message::OpenPlaylist(p.id.clone()),
                            Message::OpenPlaylistMenu(p.id.clone()),
                        )
                    })
                    .collect();
                let library_section: iced::Element<'_, Message> = if lib_cards.is_empty() {
                    text("").size(1).into()
                } else {
                    column![
                        text(self.language.from_library()).size(19),
                        scrollable(row(lib_cards).spacing(10))
                            .direction(scrollable::Direction::Horizontal(slim_bar()))
                            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("home")))
                            .on_scroll(|v| Message::Scrolled("home", v.absolute_offset().y))
                            .width(Fill)
                            .height(iced::Length::Fixed(HOME_CARD_H + HOME_STRIP_GAP)),
                    ]
                    .spacing(10)
                    .into()
                };
                let album_groups = self.recommended_albums(10);
                let album_cards: Vec<iced::Element<'_, Message>> = album_groups
                    .iter()
                    .map(|g| {
                        self.home_card(
                            &g.tracks[0],
                            g.title.clone(),
                            g.artist.clone(),
                            Message::PlayHomeAlbum {
                                album: g.title.clone(),
                                artist: g.artist.clone(),
                            },
                            Message::OpenHomeAlbumMenu {
                                album: g.title.clone(),
                                artist: g.artist.clone(),
                            },
                        )
                    })
                    .collect();
                let albums_section: iced::Element<'_, Message> = if album_cards.is_empty() {
                    text("").size(1).into()
                } else {
                    column![
                        text(self.language.recommended_albums()).size(19),
                        scrollable(row(album_cards).spacing(10))
                            .direction(scrollable::Direction::Horizontal(slim_bar()))
                            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("home")))
                            .on_scroll(|v| Message::Scrolled("home", v.absolute_offset().y))
                            .width(Fill)
                            .height(iced::Length::Fixed(HOME_CARD_H + HOME_STRIP_GAP)),
                    ]
                    .spacing(10)
                    .into()
                };
                let more_section: iced::Element<'_, Message> =
                    match self.home_shelves.more_artist.clone() {
                        Some(artist) => {
                            let tracks = &self.home_shelves.more;
                            if tracks.is_empty() {
                                text("").size(1).into()
                            } else {
                                let rows: Vec<iced::Element<'_, Message>> =
                                    tracks.iter().map(|t| self.home_song_row(t)).collect();
                                column![
                                    text(self.language.more_from(&artist)).size(19),
                                    column(rows).spacing(2),
                                ]
                                .spacing(10)
                                .into()
                            }
                        }
                        None => text("").size(1).into(),
                    };
                column![
                    text(self.language.greeting(local_hour())).size(24),
                    quick_section,
                    library_section,
                    albums_section,
                    more_section,
                    text(self.language.jump_back_in()).size(19),
                    grid,
                    row![
                        text(self.language.made_for_you()).size(19).width(Fill),
                        button(
                            row![
                                btn_text(self.language.discovery_link(), 12),
                                container(btn_icon("chevron-right", 12.0, iced::Color::WHITE,))
                                    .padding(iced::Padding {
                                        bottom: 2.0,
                                        ..Default::default()
                                    }),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(ghost_button)
                        .on_press(Message::Navigate(Route::Discovery))
                        .padding(8),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    row![
                        row_button(
                            column![
                                btn_text(self.language.weekly_mix(), 15),
                                btn_text(self.language.track_count(weekly_n), 12).color(MUTED),
                            ]
                            .spacing(2)
                            .padding(12)
                            .width(Fill),
                        )
                        .style(card_button)
                        .on_press(Message::PlayWeeklyMix)
                        .padding(0),
                        row_button(
                            column![
                                btn_text(self.language.forgotten(), 15),
                                btn_text(self.language.track_count(forgotten_n), 12).color(MUTED),
                            ]
                            .spacing(2)
                            .padding(12)
                            .width(Fill),
                        )
                        .style(card_button)
                        .on_press(Message::PlayForgotten)
                        .padding(0),
                    ]
                    .spacing(8),
                ]
                .spacing(12)
                .into()
            }
            Route::Search => {
                let query = self.search_query.trim().to_string();
                // Empty query: a hint to use the bar above (never a
                // blank page). Single characters show it too (no
                // network fires below 2 chars for results).
                // Every branch below is page content only: the header
                // (search input) and scroll wrapper always render.
                let content: iced::Element<'_, Message> = if query.len() < 2 {
                    column![text(self.language.search_hint_empty()).color(MUTED),]
                        .spacing(10)
                        .into()
                } else if self.searching
                    && results_empty(&self.results)
                    && all_empty(&self.all_results)
                {
                    self.search_loading()
                } else if results_empty(&self.results) && all_empty(&self.all_results) {
                    column![text(self.language.no_results()).color(MUTED),]
                        .spacing(10)
                        .into()
                } else {
                    let chips = self.search_filter_chips();
                    match self.search_filter {
                        SearchFilter::All => {
                            // Server relevance order straight from the
                            // unfiltered response: top hit in the hero
                            // card, shelves as type-labelled rows.
                            let hero_hit = top_hero(&self.all_results);
                            let hero: iced::Element<'_, Message> = match &hero_hit {
                                Some(h) => self.search_hero(h),
                                None => text("").size(1).into(),
                            };
                            let rows: Vec<iced::Element<'_, Message>> =
                                order_all_hits(&self.all_results)
                                    .into_iter()
                                    .map(|h| self.search_hit_row(&h))
                                    .collect();
                            column![chips, hero, column(rows).spacing(2),]
                                .spacing(10)
                                .into()
                        }
                        SearchFilter::Songs => {
                            let songs: Vec<iced::Element<'_, Message>> = self
                                .results
                                .tracks
                                .iter()
                                .enumerate()
                                .map(|(i, t)| self.numbered_track_row(t, i + 1, &RowPlay::Now))
                                .collect();
                            let list: iced::Element<'_, Message> = if songs.is_empty() {
                                text(self.language.no_songs_for(&query)).color(MUTED).into()
                            } else {
                                column(songs).spacing(2).into()
                            };
                            column![chips, list,].spacing(10).into()
                        }
                        SearchFilter::Albums => {
                            let cards: Vec<iced::Element<'_, Message>> = self
                                .results
                                .albums
                                .iter()
                                .map(|a| {
                                    let id = a.id.clone();
                                    iced::widget::mouse_area(self.album_card(a))
                                        .on_right_press(Message::OpenAlbumMenu(id))
                                        .into()
                                })
                                .collect();
                            let list: iced::Element<'_, Message> = if cards.is_empty() {
                                text(self.language.no_albums_for(&query))
                                    .color(MUTED)
                                    .into()
                            } else {
                                Self::card_strip(cards)
                            };
                            column![chips, list,].spacing(10).into()
                        }
                        SearchFilter::Artists => {
                            let cards: Vec<iced::Element<'_, Message>> = self
                                .results
                                .artists
                                .iter()
                                .map(|a| {
                                    let id = a.id.clone();
                                    iced::widget::mouse_area(self.artist_card(a))
                                        .on_right_press(Message::OpenArtistMenu(id))
                                        .into()
                                })
                                .collect();
                            let list: iced::Element<'_, Message> = if cards.is_empty() {
                                text(self.language.no_artists_for(&query))
                                    .color(MUTED)
                                    .into()
                            } else {
                                Self::card_strip(cards)
                            };
                            column![chips, list,].spacing(10).into()
                        }
                        SearchFilter::Playlists => {
                            let rows: Vec<iced::Element<'_, Message>> = self
                                .results
                                .playlists
                                .iter()
                                .map(|p| self.search_playlist_row(p))
                                .collect();
                            let list: iced::Element<'_, Message> = if rows.is_empty() {
                                text(self.language.no_playlists_for(&query))
                                    .color(MUTED)
                                    .into()
                            } else {
                                column(rows).spacing(2).into()
                            };
                            column![chips, list,].spacing(10).into()
                        }
                    }
                };
                content
            }
            Route::History => self.history_view(),
            Route::Discovery => {
                let weekly: Vec<iced::Element<'_, Message>> = self
                    .home_shelves
                    .weekly
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let forgotten: Vec<iced::Element<'_, Message>> = self
                    .home_shelves
                    .forgotten
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let weekly_section: iced::Element<'_, Message> = if weekly.is_empty() {
                    text(self.language.discovery_hint_play())
                        .color(MUTED)
                        .into()
                } else {
                    column(weekly).spacing(2).into()
                };
                let forgotten_section: iced::Element<'_, Message> = if forgotten.is_empty() {
                    text(self.language.discovery_hint_like())
                        .color(MUTED)
                        .into()
                } else {
                    column(forgotten).spacing(2).into()
                };
                column![
                    text(self.language.made_for_you()).size(22),
                    row![
                        text(self.language.weekly_mix()).size(17).width(Fill),
                        button(
                            row![
                                btn_icon("play", 13.0, iced::Color::WHITE),
                                btn_text(self.language.play_all(), 13)
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                        )
                        .style(accent_button)
                        .on_press(Message::PlayWeeklyMix)
                        .padding(7),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    weekly_section,
                    row![
                        text(self.language.forgotten()).size(17).width(Fill),
                        button(
                            row![
                                btn_icon("play", 13.0, iced::Color::WHITE),
                                btn_text(self.language.play_all(), 13)
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                        )
                        .style(accent_button)
                        .on_press(Message::PlayForgotten)
                        .padding(7),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    forgotten_section,
                ]
                .spacing(10)
                .into()
            }
            Route::Settings => {
                let eq_names: Vec<String> = EqPreset::builtin_presets()
                    .into_iter()
                    .map(|p| p.name)
                    .collect();
                // Segmented preset picker: one boxed group with the kept
                // preset filled red even while bypassed (the toggle owns
                // the on/off state). Rows flow like flex-wrap: greedy
                // fill per row, so short presets join the first row even
                // when the full strip cannot fit.
                let win_w = self.window.map(|s| s.width).unwrap_or(1280.0);
                let body_w =
                    (win_w - self.sidebar_w - self.right_w - 32.0).min(CENTER_MAX_W) - 40.0;
                let eq_body_w = body_w.min(SETTINGS_INPUT_W);
                let eq_group: iced::Element<'_, Message> = container(
                    iced::widget::Column::with_children(
                        eq_flow_rows(&eq_names, eq_body_w)
                            .into_iter()
                            .map(|row_names| {
                                iced::widget::Row::with_children(
                                    row_names
                                        .iter()
                                        .map(|n| {
                                            let active = *n == self.eq_preset;
                                            button(btn_text(n.clone(), 12))
                                                .style(seg_button(active))
                                                .on_press(Message::SelectEqPreset(n.clone()))
                                                .padding(8)
                                                .into()
                                        })
                                        .collect::<Vec<iced::Element<'_, Message>>>(),
                                )
                                .spacing(2)
                                .into()
                            })
                            .collect::<Vec<iced::Element<'_, Message>>>(),
                    )
                    .spacing(2),
                )
                .style(seg_group)
                .padding(4)
                .into();
                // Rodio/no-device honesty note only (mpv shows nothing, so
                // the capped layout stays clean on the real app).
                let eq_warn: iced::Element<'_, Message> = {
                    let note = self.eq_note();
                    if note.is_empty() {
                        text("").size(1).into()
                    } else {
                        text(note).size(12).color(MUTED).into()
                    }
                };
                // Graphic EQ panel: bypass toggle top-right, canvas
                // sliders + curve below. Presets above are predefined
                // curves for the same gains.
                let eq_panel: iced::Element<'_, Message> = container(
                    column![
                        row![
                            iced::widget::Space::new().width(Fill),
                            text(self.language.enable_custom_eq()).size(12).color(MUTED),
                            toggler(self.eq_enabled)
                                .on_toggle(Message::EqToggled)
                                .style(red_toggle),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                        eq_graph::eq_graph(self.eq_gains),
                    ]
                    .spacing(8),
                )
                .style(card_bg)
                .padding(12)
                .width(Fill)
                .into();
                // Uninstalled pick: the stored custom family is gone, so
                // body text silently uses the fallback until repicked
                // (bundled Adwaita Sans and System can never go missing).
                let stale_font_note: iced::Element<'_, Message> =
                    if is_font_missing(self.font_family, &self.system_font_rows) {
                        text(self.language.stale_font_note())
                            .size(12)
                            .color(MUTED)
                            .into()
                    } else {
                        text("").size(1).into()
                    };
                // Pending pick: one click restarts into the new font
                // (the queue snapshot restores paused, so nothing a
                // manual quit would keep is lost).
                let relaunch_button: iced::Element<'_, Message> =
                    if font_pending(self.font_family, effective_font()) {
                        button(btn_text(self.language.relaunch_to_apply(), 12))
                            .style(ghost_button)
                            .on_press(Message::RelaunchNow)
                            .padding(8)
                            .into()
                    } else {
                        text("").size(1).into()
                    };
                column![
                    text(self.language.settings_title()).size(22),
                    text(self.language.language_section()).size(17),
                    settings_input({
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_text(self.language.label(), 13).width(Fill),
                                btn_icon("chevron-down", 13.0, MUTED),
                            ]
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(select_face)
                        .on_press(Message::OpenSelectMenu(SelectMenu::Language))
                        .padding(8)
                        .width(Fill)
                        .into();
                        let menu = (self.sort_menu == Some(SelectMenu::Language))
                            .then(|| self.select_menu_card(&SelectMenu::Language, 240.0));
                        dropdown(face, menu).into()
                    }),
                    text(self.language.playback_section()).size(17),
                    column![
                        text(if self.muted {
                            self.language.volume_muted(self.volume * 100.0)
                        } else {
                            self.language.volume(self.volume * 100.0)
                        })
                        .size(13),
                        settings_input(
                            row![
                                pointing_slider(0.0..=1.0, self.volume, Message::SetVolume)
                                    .step(0.01_f32)
                                    .style(themed_slider)
                                    .width(Fill),
                                self.mute_button(),
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                            .into(),
                        ),
                    ]
                    .spacing(4),
                    settings_input(eq_group),
                    settings_input(eq_warn),
                    settings_input(eq_panel),
                    text(self.language.lyrics_section()).size(17),
                    text(self.language.default_lyrics_provider()).size(13),
                    settings_input({
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_text(self.lyrics_provider.label(), 13).width(Fill),
                                btn_icon("chevron-down", 13.0, MUTED),
                            ]
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(select_face)
                        .on_press(Message::OpenSelectMenu(SelectMenu::LyricsProvider))
                        .padding(8)
                        .width(Fill)
                        .into();
                        let menu = (self.sort_menu == Some(SelectMenu::LyricsProvider))
                            .then(|| self.select_menu_card(&SelectMenu::LyricsProvider, 240.0));
                        dropdown(face, menu).into()
                    }),
                    text(self.language.appearance_section()).size(17),
                    text(self.language.display_font()).size(13),
                    settings_input({
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_text(self.font_family.label(), 13).width(Fill),
                                btn_icon("chevron-down", 13.0, MUTED),
                            ]
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(select_face)
                        .on_press(Message::OpenSelectMenu(SelectMenu::FontFamily))
                        .padding(8)
                        .width(Fill)
                        .into();
                        let menu = (self.sort_menu == Some(SelectMenu::FontFamily))
                            .then(|| self.select_menu_card(&SelectMenu::FontFamily, 240.0));
                        dropdown(face, menu).into()
                    }),
                    relaunch_button,
                    stale_font_note,
                    column![
                        text(self.language.interface_scale(self.ui_scale * 100.0)).size(13),
                        settings_input(
                            pointing_slider(
                                UI_SCALE_MIN..=UI_SCALE_MAX,
                                self.ui_scale,
                                Message::UiScaleChanged
                            )
                            .step(0.05_f32)
                            .style(themed_slider)
                            .width(Fill)
                            .into(),
                        ),
                        text(self.language.scale_hint()).size(12).color(MUTED),
                    ]
                    .spacing(4),
                    text(self.language.animations_section()).size(17),
                    checkbox(self.animations_enabled)
                        .label(self.language.enable_animations())
                        .on_toggle(Message::AnimationsToggled),
                    column![
                        text(self.language.animation_duration(self.anim_ms)).size(13),
                        settings_input(
                            pointing_slider(
                                ANIM_MIN_MS as f32..=ANIM_MAX_MS as f32,
                                self.anim_ms as f32,
                                Message::AnimMsChanged
                            )
                            .step(5.0_f32)
                            .style(themed_slider)
                            .width(Fill)
                            .into(),
                        ),
                    ]
                    .spacing(4),
                    text(self.language.scrobbling_section()).size(17),
                    button(btn_text(self.language.setup_scrobbling(), 12))
                        .style(accent_button)
                        .on_press(Message::OpenScrobbleModal)
                        .padding(8),
                    text(self.language.cookie_section_title()).size(17),
                    self.cookie_section(),
                    {
                        #[cfg(feature = "innertube")]
                        let show_sync = self.logged_in || self.cookie_logged_in;
                        #[cfg(not(feature = "innertube"))]
                        let show_sync = false;
                        if show_sync {
                            iced::Element::from(
                                column![
                                    text(self.language.sync_section_title()).size(17),
                                    self.sync_section(),
                                ]
                                .spacing(10),
                            )
                        } else {
                            iced::Element::from(text("").size(1))
                        }
                    },
                    self.error_section(),
                    self.cache_section(),
                    text(self.language.about_section()).size(17),
                    column![
                        text(self.language.about_version()).size(13),
                        text(self.language.about_license()).size(12).color(MUTED),
                        button(btn_text(self.language.about_source(), 12))
                            .style(ghost_button)
                            .on_press(Message::OpenUrl(GITHUB_URL.to_string()))
                            .padding(8),
                        text(self.language.about_notices()).size(12).color(MUTED),
                        text(self.language.about_backend()).size(12).color(MUTED),
                    ]
                    .spacing(4),
                    text(self.language.shortcuts_hint()).size(12).color(MUTED),
                ]
                .spacing(10)
                .into()
            }
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let tracks = self.artist_tracks(&name);
                let albums = self.artist_albums(&name);
                let total: u32 = tracks.iter().map(|t| t.duration_secs).sum();
                // Popular prefers the live server rank (true popularity
                // with counts); the catalog fallback is alphabetical.
                // A pending fetch with nothing cached yet shows the
                // loading row, so the page never flashes the stale
                // list before the live one lands. Live shows the head
                // plus a "Show more" expander for the full ranked
                // shelf (album column included, like YTM).
                let live = self.artist_live_tracks(id);
                let channel = self.artist_channel_id(id);
                let full = channel
                    .as_ref()
                    .is_some_and(|c| self.artist_full.contains(c.as_str()));
                let fetching_full = channel.as_ref().is_some_and(|c| {
                    self.artist_expanded
                        && !full
                        && self.artist_full_loading.as_deref() == Some(c.as_str())
                });
                let pending = self.artist_list_pending(id);
                let display: &[Track] = match &live {
                    Some(songs) => songs,
                    None => &tracks,
                };
                let (popular, show_all): (
                    Vec<iced::Element<'_, Message>>,
                    iced::Element<'_, Message>,
                ) = if pending {
                    (
                        vec![text(self.language.loading_top_songs()).color(MUTED).into()],
                        text("").size(1).into(),
                    )
                } else if let Some(songs) = &live {
                    // Collapsed shows the head (YTM shows 5); expanded
                    // shows the full shelf, with a spinner beside the
                    // button while the shelf fetch is in flight (extra
                    // presses are no-ops). The button shows whenever
                    // expanding would change something: collapsed, or
                    // expanded-but-still-fetching-failed (then it
                    // retries), or expanded-and-loading.
                    let expanded = self.artist_expanded || songs.len() <= 5;
                    let more = if !expanded || (!full && !fetching_full) {
                        button(btn_text(self.language.show_more(), 12))
                            .style(ghost_button)
                            .on_press(Message::ArtistTopExpand)
                            .padding(8)
                            .into()
                    } else if fetching_full {
                        row![
                            button(btn_text(self.language.show_more(), 12))
                                .style(ghost_button)
                                .on_press(Message::ArtistTopExpand)
                                .padding(8),
                            Self::spinner_frame(self.spin_frame, 12.0, MUTED),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                    } else {
                        text("").size(1).into()
                    };
                    let rows: Vec<iced::Element<'_, Message>> =
                        (if expanded { &songs[..] } else { &songs[..5] })
                            .iter()
                            .enumerate()
                            .map(|(i, t)| self.artist_top_row(t, i + 1, id))
                            .collect();
                    (rows, more)
                } else {
                    // Catalog fallback: same suffix rule, alphabetical.
                    let play = RowPlay::ArtistAt(id.to_string());
                    (
                        display
                            .iter()
                            .take(10)
                            .enumerate()
                            .map(|(i, t)| self.numbered_track_row(t, i + 1, &play))
                            .collect(),
                        text("").size(1).into(),
                    )
                };
                let cards: Vec<iced::Element<'_, Message>> =
                    albums.iter().take(5).map(|a| self.album_card(a)).collect();
                // Radio seeds from the displayed list (live top hit when
                // fetched); header counts stay library-scoped by design.
                let radio_btn: iced::Element<'_, Message> = match display.first() {
                    None => text("").size(1).into(),
                    Some(seed) => button(
                        row![
                            btn_icon("radio", 13.0, iced::Color::WHITE),
                            btn_text(self.language.radio_label(), 13),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(ghost_button)
                    .on_press(Message::StartRadio(seed.id.clone()))
                    .padding(8)
                    .into(),
                };
                column![
                    row![
                        self.art_thumb(self.artist_photo(&name).as_deref(), 160.0),
                        column![
                            text(self.language.artist_label()).size(12).color(MUTED),
                            text(name.clone()).size(30),
                            text(self.language.artist_songs_albums(
                                tracks.len(),
                                albums.len(),
                                &fmt_time(total as f64),
                            ))
                            .size(12)
                            .color(MUTED),
                            row![
                                Self::play_pill(self.language.play_pill_label(), Message::PlayAll),
                                Self::shuffle_pill(
                                    self.language.shuffle_pill_label(),
                                    Message::PlayAllShuffled
                                ),
                                radio_btn,
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center),
                        ]
                        .spacing(6)
                        .width(Fill),
                    ]
                    .spacing(16)
                    .align_y(iced::alignment::Vertical::Center),
                    Self::section(self.language, self.language.popular(), popular),
                    show_all,
                    Self::section(
                        self.language,
                        self.language.albums_section(),
                        if cards.is_empty() {
                            vec![]
                        } else {
                            vec![Self::card_strip(cards)]
                        }
                    ),
                ]
                .spacing(12)
                .into()
            }
            Route::Album(id) => {
                let title = self.resolve_album_title(id);
                // Live `track_no`-ordered tracklist when fetched, else
                // the catalog fallback (alphabetical). Pending fetch:
                // the loading row stands in for the catalog fallback
                // (same no-flash rule as artist Popular).
                let live = self.album_live_tracks(id);
                let pending = self.album_list_pending(id);
                let tracks: Vec<Track> = match live {
                    Some(songs) => songs,
                    None => self.album_tracks(id),
                };
                let total: u32 = tracks.iter().map(|t| t.duration_secs).sum();
                let artist = tracks.first().map(|t| t.artist.clone()).unwrap_or_default();
                let year = self
                    .results
                    .albums
                    .iter()
                    .find(|a| a.title == title)
                    .and_then(|a| a.year)
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                // One policy (see `album_header_cover`): the header and
                // its warming share the key, so it never placeholders.
                let cover = self.album_header_cover(id);
                let play = RowPlay::AlbumAt(id.to_string());
                let rows: Vec<iced::Element<'_, Message>> = if pending {
                    vec![text(self.language.loading_album_songs())
                        .color(MUTED)
                        .into()]
                } else {
                    tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| self.numbered_track_row(t, i + 1, &play))
                        .collect()
                };
                let meta = if year.is_empty() {
                    self.language
                        .album_song_meta(&artist, tracks.len(), &fmt_time(total as f64))
                } else {
                    self.language.album_song_meta_year(
                        &artist,
                        &year,
                        tracks.len(),
                        &fmt_time(total as f64),
                    )
                };
                column![
                    row![
                        self.art_thumb(cover.as_deref(), 180.0),
                        column![
                            text(self.language.album_label()).size(12).color(MUTED),
                            text(title.clone()).size(28),
                            text(meta).size(12).color(MUTED),
                            row![
                                Self::play_pill(self.language.play_pill_label(), Message::PlayAll),
                                Self::shuffle_pill(
                                    self.language.shuffle_pill_label(),
                                    Message::PlayAllShuffled
                                ),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center),
                        ]
                        .spacing(6)
                        .width(Fill),
                    ]
                    .spacing(16)
                    .align_y(iced::alignment::Vertical::Center),
                    column(rows).spacing(2),
                ]
                .spacing(12)
                .into()
            }
            Route::PlaylistDetail(id) => match self.find_playlist(id) {
                None => column![text(self.language.playlist_not_found(id)).size(20)]
                    .spacing(8)
                    .into(),
                Some(p) => {
                    let pid = p.id.clone();
                    let is_fav = pid == Self::FAVORITES_ID;
                    let sort = self.playlist_sort_cached(&pid);
                    let ordered = self.playlist_ordered_tracks(&p);
                    // Sort control for own playlists only (Favorites keeps
                    // its liked-order view with no picker, like before):
                    // sort icon + current sort in the quiet button family.
                    // The open card floats from this button (dropdown
                    // overlay), never from the cursor. Fixed width
                    // (longest pick) so the `...` trigger never shifts
                    // when the pick changes.
                    let sort_button: iced::Element<'_, Message> = if is_fav {
                        text("").size(1).into()
                    } else {
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_icon("arrow-up-down", 13.0, MUTED),
                                btn_text(sort.label(), 13),
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(select_face)
                        .on_press(Message::OpenSelectMenu(SelectMenu::Sort(pid.clone())))
                        .padding(8)
                        .width(iced::Length::Fixed(SORT_BUTTON_W))
                        .into();
                        let menu = (self.sort_menu == Some(SelectMenu::Sort(pid.clone())))
                            .then(|| self.select_menu_card(&SelectMenu::Sort(pid.clone()), 220.0));
                        dropdown(face, menu).into()
                    };
                    // Rows: Manual reuses the exact same row visuals as
                    // every other pick at rest (same spacing, no grip,
                    // no dividers); drag arming is invisible. A live drag
                    // holds the origin in the floating ghost and opens one
                    // full-row gap at the commit target (siblings shift to
                    // make room); only the transparent slot lanes appear
                    // on top of that.
                    let list: iced::Element<'_, Message> = if !is_fav
                        && sort == PlaylistSort::Manual
                        && ordered.len() >= 2
                    {
                        let len = ordered.len();
                        let from = self.pl_drag.map(|d| d.from).unwrap_or(0);
                        let live = self.pl_dragging()
                            && self.pl_drag_id.as_deref() == Some(pid.as_str())
                            && from < len;
                        if live {
                            let slot = self.pl_effective_slot(len);
                            let to = slot.and_then(|s| slot_commit(len, from, s)).unwrap_or(from);
                            let (seq, lanes) = drag_display_order(len, from, to);
                            // Constant full-row gap like the queue: a
                            // teleporting gap must never resize the list.
                            let gap_h = TRACK_ROW_H;
                            let mut items: Vec<iced::Element<'_, Message>> =
                                Vec::with_capacity(2 * len + 1);
                            items.push(self.pl_drop_slot(len, lanes[0]));
                            for (p, item) in seq.iter().enumerate() {
                                match item {
                                    Some(k) => {
                                        items.push(self.playlist_manual_row(&pid, &ordered[*k], *k))
                                    }
                                    None => items.push(drag_gap_box(gap_h)),
                                }
                                if p + 1 < len {
                                    items.push(self.pl_drop_divider(len, lanes[p + 1]));
                                } else {
                                    items.push(self.pl_drop_slot(len, lanes[p + 1]));
                                }
                            }
                            iced::widget::Column::with_children(items).spacing(0).into()
                        } else {
                            let rows: Vec<iced::Element<'_, Message>> = ordered
                                .iter()
                                .enumerate()
                                .map(|(i, t)| self.playlist_manual_row(&pid, t, i))
                                .collect();
                            column(rows).spacing(2).into()
                        }
                    } else {
                        let rows: Vec<iced::Element<'_, Message>> = ordered
                            .iter()
                            .enumerate()
                            .map(|(i, t)| self.playlist_track_row(&pid, t, i))
                            .collect();
                        column(rows).spacing(2).into()
                    };
                    // Actions: play, shuffle, sort (own lists only), menu.
                    // Built as a vec so Favorites simply omits the sort
                    // slot instead of leaving an empty gap in the row.
                    let mut actions: Vec<iced::Element<'_, Message>> = vec![
                        button(
                            row![
                                btn_icon("play", 13.0, iced::Color::WHITE),
                                btn_text(self.language.play_all(), 13)
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(accent_button)
                        .on_press(Message::PlayAll)
                        .padding(8)
                        .into(),
                        button(
                            row![
                                btn_icon("shuffle", 13.0, iced::Color::WHITE),
                                btn_text(self.language.shuffle_label(), 13)
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(accent_button)
                        .on_press(Message::PlayAllShuffled)
                        .padding(8)
                        .into(),
                    ];
                    if !is_fav {
                        actions.push(sort_button);
                    }
                    actions.push(
                        button(btn_icon("ellipsis", 15.0, MUTED))
                            .style(hover_row)
                            .on_press(Message::OpenPlaylistMenu(p.id.clone()))
                            .padding(8)
                            .into(),
                    );
                    column![
                        text(p.title.clone()).size(24),
                        text(self.language.playlist_sub(p.tracks.len()))
                            .size(12)
                            .color(MUTED),
                        iced::widget::Row::with_children(actions)
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center),
                        list,
                    ]
                    .spacing(10)
                    .into()
                }
            },
            Route::Lyrics(id) => {
                let title = self
                    .find_track(id)
                    .map(|t| format!("{} · {}", t.title, t.artist))
                    .unwrap_or_else(|| id.clone());
                let stepper = self.lyrics_provider_stepper(id);
                let content = self.lyrics_content(id);
                column![
                    text(title).size(22),
                    row![
                        button(
                            row![
                                btn_icon("play", 13.0, iced::Color::WHITE),
                                btn_text(self.language.play_label(), 13)
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                        )
                        .style(accent_button)
                        .on_press(Message::PlayNow(id.clone()))
                        .padding(7),
                        button(btn_text(self.language.radio_label(), 13))
                            .style(ghost_button)
                            .on_press(Message::StartRadio(id.clone()))
                            .padding(8),
                    ]
                    .spacing(8),
                    stepper,
                    content,
                ]
                .spacing(10)
                .into()
            }
            Route::Radio(seed) => {
                let rows: Vec<iced::Element<'_, Message>> = self
                    .radio_tracks
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let seed_label = self
                    .find_track(seed)
                    .map(|t| format!("{} · {}", t.title, t.artist))
                    .unwrap_or_else(|| seed.clone());
                column![
                    text(seed_label).size(24),
                    text(self.language.radio_label()).size(12).color(MUTED),
                    button(
                        row![
                            btn_icon("play", 13.0, iced::Color::WHITE),
                            btn_text(self.language.start_radio(), 13)
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center)
                    )
                    .style(accent_button)
                    .on_press(Message::StartRadio(seed.clone()))
                    .padding(8),
                    column(rows).spacing(2),
                ]
                .spacing(10)
                .into()
            }
        };
        // Single header source of truth (drawer-open state reuses it so
        // the search bar never vanishes).
        let header = self.header_view();
        // Quick-results popup, YouTube-style: suggestions + top tracks.
        // Enter jumps to the full Search page; picking a track plays now.
        // Rendered at window level (above the drawer), not here.
        // Very wide windows center a capped body so rows stop gluing
        // to the far edges; normal windows stay full bleed.
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let scrolled: iced::Element<'_, Message> = if ww > CENTER_MARGIN_BREAK {
            scrollable(
                container(
                    container(body)
                        .padding(20)
                        .width(Fill)
                        .max_width(CENTER_MAX_W),
                )
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Center),
            )
            .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("body")))
            .on_scroll(|v| Message::Scrolled("body", v.absolute_offset().y))
            .height(Fill)
            .into()
        } else {
            scrollable(container(body).padding(20).width(Fill))
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("body")))
                .on_scroll(|v| Message::Scrolled("body", v.absolute_offset().y))
                .height(Fill)
                .into()
        };
        column![header, scrolled,].spacing(4).into()
    }

    /// Search popup content, if the gate holds (window-level layer above
    /// the drawer; dismissed by scrim, Esc, navigation, or any action).
    pub(crate) fn suggest_popup(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{column, container, row, text};
        use iced::Fill;
        let qnow = self.search_query.trim().to_string();
        if !(self.show_suggest
            && qnow.len() >= 2
            && (!self.suggest.is_empty() || !self.results.tracks.is_empty()))
        {
            return None;
        }
        {
            let sug_rows: Vec<iced::Element<'_, Message>> = self
                .suggest
                .iter()
                .enumerate()
                .map(|(i, sug)| {
                    row_button(
                        row![
                            btn_icon("search", 13.0, MUTED),
                            btn_text(sug.clone(), 13).width(Fill),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(if self.suggest_idx == Some(i) {
                        selected_row
                    } else {
                        hover_row
                    })
                    .on_press(Message::PickSuggestion(sug.clone()))
                    .padding(5)
                    .width(Fill)
                    .into()
                })
                .collect();
            let top: Vec<iced::Element<'_, Message>> = self
                .results
                .tracks
                .iter()
                .take(5)
                .enumerate()
                .map(|(i, t)| {
                    self.suggest_track_row(t, self.suggest_idx == Some(self.suggest.len() + i))
                })
                .collect();
            let progress: iced::Element<'_, Message> = if self.searching {
                text(self.language.searching_short())
                    .size(11)
                    .color(ACCENT)
                    .into()
            } else {
                text(self.language.suggest_hint())
                    .size(11)
                    .color(MUTED)
                    .into()
            };
            Some(
                container(
                    column![
                        column(sug_rows).spacing(0),
                        progress,
                        column(top).spacing(2),
                    ]
                    .spacing(6),
                )
                .style(card_bg)
                .padding(10)
                .into(),
            )
        }
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        #[cfg(feature = "debug-panel")]
        let t0 = std::time::Instant::now();
        let el = self.view_inner();
        #[cfg(feature = "debug-panel")]
        self.view_build_ms.set(t0.elapsed().as_secs_f32() * 1000.0);
        el
    }

    pub(crate) fn view_inner(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, mouse_area, row, text, Row, Stack};
        use iced::Fill;
        // The drawer covers the center body only: header (search) stays
        // visible at all times. Fully open it replaces the body so
        // nothing behind scrolls or clicks.
        let center_body: iced::Element<'_, Message> = if self.drawer_anim > 0.98 {
            iced::widget::column![self.header_view(), self.drawer_body_layer()]
                .spacing(4)
                .into()
        } else if self.drawer_anim > 0.02 {
            Stack::with_children(vec![self.center_view(), self.drawer_layer()]).into()
        } else {
            self.center_view()
        };
        let center: iced::Element<'_, Message> =
            container(center_body).width(Fill).height(Fill).into();
        // The collapsed rail always reserves its strip, even when empty.
        let main_children = vec![self.sidebar_view(), center, self.right_panel_view()];
        let main = Row::with_children(main_children).spacing(8).height(Fill);
        // Import banner first (in-flow, pushes content down): `None`
        // when idle, so the layout is untouched outside imports.
        #[cfg(feature = "innertube")]
        let banner = self.import_banner_view();
        #[cfg(not(feature = "innertube"))]
        let banner: Option<iced::Element<'_, Message>> = None;
        let mut base_children: Vec<iced::Element<'_, Message>> = Vec::new();
        if let Some(banner) = banner {
            base_children.push(banner);
        }
        base_children.push(main.into());
        base_children.push(self.player_view());
        let base: iced::Element<'_, Message> = container(
            iced::widget::Column::with_children(base_children)
                .spacing(8)
                .height(Fill),
        )
        .style(app_bg)
        // Roomier top padding so the header never hugs the window
        // edge (sides/bottom stay tight).
        .padding(iced::Padding {
            top: 16.0,
            left: 8.0,
            right: 8.0,
            bottom: 8.0,
        })
        .into();
        let mut layers: Vec<iced::Element<'_, Message>> = vec![base];
        // Floating drag ghost right above the base (modal layers cover it):
        // `None` unless a drag is live with a known cursor.
        if let Some(ghost) = self.drag_ghost_layer() {
            layers.push(ghost);
        }
        // Search popup above everything in the center (including the
        // drawer), below the context menu. The scrim wraps a hole the
        // exact size of the input pill (shared geometry, so only the
        // input stays clickable); anything else outside dismisses like
        // a context menu.
        if let Some(popup) = self.suggest_popup() {
            // Shared pill edges: popup width always matches the input.
            let (pop_left, pop_w) = self.search_pill_geom();
            // Header band height: the pill sits inside it, the popup
            // starts right below it.
            const HOLE_H: f32 = 66.0;
            let dismiss = |w: iced::Length, h: iced::Length| {
                mouse_area(iced::widget::Space::new().width(w).height(h))
                    .on_press(Message::CloseSuggest)
                    .on_right_press(Message::CloseSuggest)
                    .on_enter(Message::ClearHovers)
                    .on_exit(Message::ClearHovers)
            };
            let suggest_scrim: iced::Element<'_, Message> = container(column![
                row![
                    dismiss(iced::Length::Fixed(pop_left), iced::Length::Fixed(HOLE_H)),
                    // Pill hole: passive space, clicks reach the input.
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(pop_w))
                        .height(iced::Length::Fixed(HOLE_H)),
                    dismiss(iced::Length::Fill, iced::Length::Fixed(HOLE_H)),
                ]
                .height(iced::Length::Fixed(HOLE_H)),
                dismiss(iced::Length::Fill, iced::Length::Fill),
            ])
            .width(Fill)
            .height(Fill)
            .into();
            let suggest_placed: iced::Element<'_, Message> = container(column![
                iced::widget::Space::new().height(iced::Length::Fixed(HOLE_H)),
                row![
                    iced::widget::Space::new().width(iced::Length::Fixed(pop_left)),
                    container(popup).width(iced::Length::Fixed(pop_w)),
                ],
            ])
            .width(Fill)
            .height(Fill)
            .into();
            layers.push(suggest_scrim);
            layers.push(suggest_placed);
        }
        if let Some(menu_id) = self.context_menu.clone() {
            if let Some(t) = self.find_track(&menu_id) {
                let (heart_color, heart) = if self.liked.contains(&t.id) {
                    (ACCENT, self.language.remove_from_favorites())
                } else {
                    (MUTED, self.language.add_to_favorites())
                };
                let mut items: Vec<iced::Element<'_, Message>> = vec![
                    row_button(
                        row![
                            btn_icon("play", 14.0, iced::Color::WHITE),
                            btn_text(self.language.play_now(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            btn_icon("list-plus", 14.0, iced::Color::WHITE),
                            btn_text(self.language.play_next(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::PlayNext(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            btn_icon("plus", 14.0, iced::Color::WHITE),
                            btn_text(self.language.add_to_queue(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::QueueTrack(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            btn_icon("radio", 14.0, iced::Color::WHITE),
                            btn_text(self.language.start_mix(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::StartRadio(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            btn_icon("mic", 14.0, iced::Color::WHITE),
                            btn_text(self.language.lyrics_menu(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenLyrics(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            heart_icon(self.liked.contains(&t.id), 14.0, heart_color),
                            btn_text(heart, 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    {
                        let disliked = self.disliked.contains(&t.id);
                        let (thumb_color, thumb_label) = if disliked {
                            (ACCENT, self.language.recommend_again())
                        } else {
                            (MUTED, self.language.dont_recommend())
                        };
                        row_button(
                            row![
                                thumbs_down_icon(disliked, 14.0, thumb_color),
                                btn_text(thumb_label, 14)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill)
                            .height(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::Dislike(t.id.clone()))
                        .padding(8)
                        .width(Fill)
                        .height(iced::Length::Fixed(MENU_ROW_H))
                        .into()
                    },
                    row_button(
                        row![
                            btn_icon("disc", 14.0, iced::Color::WHITE),
                            btn_text(self.language.go_to_album(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenTrackAlbum(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                    row_button(
                        row![
                            btn_icon("user-round", 14.0, iced::Color::WHITE),
                            btn_text(self.language.go_to_artist(), 14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill)
                        .height(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenTrackArtist(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .height(iced::Length::Fixed(MENU_ROW_H))
                    .into(),
                ];
                if self.queue.items().iter().any(|q| q.id == t.id) {
                    items.push(
                        row_button(
                            row![
                                btn_icon("trash", 14.0, iced::Color::WHITE),
                                btn_text(self.language.remove_from_queue(), 14)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill)
                            .height(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RemoveFromQueue(t.id.clone()))
                        .padding(8)
                        .width(Fill)
                        .height(iced::Length::Fixed(MENU_ROW_H))
                        .into(),
                    );
                }
                if let Some(ai) = self.autoplay.iter().position(|q| q.id == t.id) {
                    items.push(
                        row_button(
                            row![
                                btn_icon("trash", 14.0, iced::Color::WHITE),
                                btn_text(self.language.remove_from_autoplay(), 14)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill)
                            .height(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RemoveAutoplay(ai))
                        .padding(8)
                        .width(Fill)
                        .height(iced::Length::Fixed(MENU_ROW_H))
                        .into(),
                    );
                }
                // Playlist membership for every row (precomputed: the menu
                // buttons below borrow nothing from self). Built-ins
                // lead via all_playlists.
                let membership: Vec<(String, String, bool)> = self
                    .all_playlists()
                    .iter()
                    .map(|pl| {
                        let has = self.playlist_has(&pl.id, &t.id);
                        (pl.id.clone(), pl.title.clone(), has)
                    })
                    .collect();
                // Flyout toggle: the playlist list opens to the side so
                // transport rows stay put and multi-add never closes.
                // Hover opens it (no click needed); the Tick grace
                // closes it once both the toggle and the panel are
                // cold. Its index below anchors the flyout height.
                let sub_id = t.id.clone();
                let sub_open = self.submenu_playlists_for.as_deref() == Some(t.id.as_str());
                let toggle_idx = items.len();
                let sub_id_enter = sub_id.clone();
                items.push(
                    mouse_area(
                        row_button(
                            row![
                                btn_icon("queue", 14.0, iced::Color::WHITE),
                                btn_text(self.language.playlists_toggle(membership.len()), 14)
                                    .width(Fill),
                                container(btn_icon("chevron-right", 14.0, MUTED)).padding(
                                    iced::Padding {
                                        bottom: 2.0,
                                        ..Default::default()
                                    },
                                ),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill)
                            .height(Fill),
                        )
                        .style(if sub_open { selected_row } else { hover_row })
                        .on_press(Message::TogglePlaylistSubmenu(sub_id))
                        .padding(8)
                        .width(Fill)
                        .height(iced::Length::Fixed(MENU_ROW_H)),
                    )
                    .on_enter(Message::SubmenuTriggerHot(Some(sub_id_enter)))
                    .on_exit(Message::SubmenuTriggerHot(None))
                    .into(),
                );
                // Playlist adds leave the menu open (multi-add); everything else
                // closes it via its own handler.
                let menu = container(
                    column![
                        row![
                            self.thumb_hero(&t, 56.0),
                            column![
                                text(t.title.clone()).size(16),
                                text(t.artist.clone()).size(12).color(MUTED),
                            ]
                            .spacing(2)
                            .width(Fill),
                        ]
                        .spacing(10)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                        column(items).spacing(0),
                    ]
                    .spacing(8),
                )
                .style(card_bg)
                .padding(14)
                .width(300);
                // Dismiss layer: left-click outside closes. No right-press
                // handler, so right-clicks pass through: right-clicking
                // another song moves the menu in one step.
                let scrim: iced::Element<'_, Message> =
                    mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                        .on_press(Message::CloseContext)
                        .on_enter(Message::ClearHovers)
                        .on_exit(Message::ClearHovers)
                        .into();
                // Cursor-anchored placement (clamped on screen), YTM-style.
                let (mx, my) = menu_pos(self.menu_at, self.window);
                let placed: iced::Element<'_, Message> = container(column![
                    iced::widget::Space::new().height(iced::Length::Fixed(my)),
                    row![
                        iced::widget::Space::new().width(iced::Length::Fixed(mx)),
                        menu,
                    ],
                ])
                .width(Fill)
                .height(Fill)
                .into();
                layers.push(scrim);
                layers.push(placed);
                // Playlist flyout: second panel to the right of the menu.
                // Hover opens it (toggle hover, no click needed); the
                // Tick grace closes it once both sides are cold. The
                // menu stays open so multi-add keeps working.
                if self.submenu_playlists_for.as_deref() == Some(t.id.as_str()) {
                    let mut fly_rows: Vec<iced::Element<'_, Message>> = Vec::new();
                    for (pl_id, pl_title, in_pl) in &membership {
                        let msg = if *in_pl {
                            Message::RemoveFromPlaylist {
                                playlist_id: pl_id.clone(),
                                track_id: t.id.clone(),
                            }
                        } else {
                            Message::AddToPlaylist {
                                playlist_id: pl_id.clone(),
                                track_id: t.id.clone(),
                            }
                        };
                        let cover = self
                            .find_playlist(pl_id)
                            .map(|pl| self.playlist_cover(&pl, 28.0));
                        let mut row_items: Vec<iced::Element<'_, Message>> = Vec::new();
                        if let Some(cover) = cover {
                            row_items.push(cover);
                        }
                        row_items.push(
                            column![
                                text(truncate_title(pl_title, 20)).size(13),
                                text(if *in_pl {
                                    "Tap to remove"
                                } else {
                                    "Tap to add"
                                })
                                .size(11)
                                .color(if *in_pl {
                                    ACCENT
                                } else {
                                    MUTED
                                }),
                            ]
                            .spacing(1)
                            .width(Fill)
                            .into(),
                        );
                        fly_rows.push(
                            row_button(
                                row(row_items)
                                    .spacing(8)
                                    .align_y(iced::alignment::Vertical::Center),
                            )
                            .style(hover_row)
                            .on_press(msg)
                            .padding(6)
                            .width(Fill)
                            .into(),
                        );
                    }
                    if fly_rows.is_empty() {
                        fly_rows.push(
                            text(self.language.no_playlists_menu())
                                .size(12)
                                .color(MUTED)
                                .into(),
                        );
                    }
                    let fly_h = ((fly_rows.len() as f32) * 44.0).min(340.0);
                    let flyout: iced::Element<'_, Message> = mouse_area(
                        container(
                            iced::widget::scrollable(column(fly_rows).spacing(2).width(Fill))
                                .direction(
                                    iced::widget::scrollable::Direction::Vertical(slim_bar()),
                                )
                                .style(move |t, s| {
                                    slim_scroll_alpha(t, s, self.scroll_opacity("flyout"))
                                })
                                .on_scroll(|v| Message::Scrolled("flyout", v.absolute_offset().y))
                                .height(iced::Length::Fixed(fly_h))
                                .width(Fill),
                        )
                        .style(card_bg)
                        .padding(10)
                        .width(iced::Length::Fixed(280.0)),
                    )
                    .on_enter(Message::SubmenuFlyHot(true))
                    .on_exit(Message::SubmenuFlyHot(false))
                    .into();
                    let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                    let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                    // Dock right of the menu; flip left when there is no
                    // room (right-edge menus would bury themselves).
                    let mx2 = if mx + 300.0 + 8.0 + 280.0 <= ww {
                        mx + 300.0 + 8.0
                    } else {
                        (mx - 280.0 - 8.0).max(0.0)
                    };
                    // Top-aligned with the Playlists toggle row: menu top
                    // padding (14) + header thumb (56) + column gap (8)
                    // plus the rows above its recorded index. The index
                    // is measured and every row is MENU_ROW_H by
                    // construction, so the anchor cannot drift.
                    let header_h = 78.0;
                    let my2 = (my + header_h + toggle_idx as f32 * MENU_ROW_H)
                        .min((wh - fly_h - 16.0).max(0.0))
                        .max(0.0);
                    let placed_fly: iced::Element<'_, Message> = container(column![
                        iced::widget::Space::new().height(iced::Length::Fixed(my2)),
                        row![
                            iced::widget::Space::new().width(iced::Length::Fixed(mx2)),
                            flyout,
                        ],
                    ])
                    .width(Fill)
                    .height(Fill)
                    .into();
                    layers.push(placed_fly);
                }
            }
        }
        // Playlist menu: same frozen-anchor scrim pattern, no flyout.
        if let Some(pl_id) = self.pl_menu.clone() {
            if let Some(p) = self.find_playlist(&pl_id) {
                let pid = p.id.clone();
                let pid2 = p.id.clone();
                let pid2b = p.id.clone();
                let pid3 = p.id.clone();
                let items: Vec<iced::Element<'_, Message>> = vec![
                    row_button(
                        row![
                            btn_icon("play", 14.0, iced::Color::WHITE),
                            btn_text(self.language.play_label(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::PlayPlaylist(pid))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    row_button(
                        row![
                            btn_icon("plus", 14.0, iced::Color::WHITE),
                            btn_text(self.language.add_to_queue(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::QueuePlaylist(pid2))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    row_button(
                        row![
                            btn_icon("shuffle", 14.0, iced::Color::WHITE),
                            btn_text(self.language.shuffle_play(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::ShufflePlaylist(pid2b))
                    .padding(8)
                    .width(Fill)
                    .into(),
                ];
                let mut pl_items = items;
                if pid3 != Self::FAVORITES_ID {
                    let pid4 = pid3.clone();
                    pl_items.push(
                        row_button(
                            row![
                                btn_icon("trash", 14.0, iced::Color::WHITE),
                                btn_text(self.language.delete_playlist(), 14),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RequestDeletePlaylist(pid4))
                        .padding(8)
                        .width(Fill)
                        .into(),
                    );
                }
                // Focal header: hero art when the first track resolves.
                let pl_art: iced::Element<'_, Message> =
                    match p.tracks.first().and_then(|t| self.find_track(&t.id)) {
                        Some(t) => self.thumb_hero(&t, 56.0),
                        None => self.playlist_cover(&p, 56.0),
                    };
                let pl_menu = container(
                    column![
                        row![
                            pl_art,
                            column![
                                text(p.title.clone()).size(16),
                                text(self.language.playlist_sub(p.tracks.len()))
                                    .size(12)
                                    .color(MUTED),
                            ]
                            .spacing(2)
                            .width(Fill),
                        ]
                        .spacing(10)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                        column(pl_items).spacing(0),
                    ]
                    .spacing(8),
                )
                .style(card_bg)
                .padding(14)
                .width(300);
                let pl_scrim: iced::Element<'_, Message> =
                    mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                        .on_press(Message::CloseContext)
                        .on_enter(Message::ClearHovers)
                        .on_exit(Message::ClearHovers)
                        .into();
                let (mx, my) = menu_pos(self.menu_at, self.window);
                let pl_placed: iced::Element<'_, Message> = container(column![
                    iced::widget::Space::new().height(iced::Length::Fixed(my)),
                    row![
                        iced::widget::Space::new().width(iced::Length::Fixed(mx)),
                        pl_menu,
                    ],
                ])
                .width(Fill)
                .height(Fill)
                .into();
                layers.push(pl_scrim);
                layers.push(pl_placed);
            }
        }
        // Selector scrim while any dropdown menu is open (the open card
        // floats from its button via the dropdown overlay): press
        // dismisses, hovers stay frozen underneath like the other menus.
        if self.sort_menu.is_some() {
            let sort_scrim: iced::Element<'_, Message> =
                mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                    .on_press(Message::CloseContext)
                    .on_enter(Message::ClearHovers)
                    .on_exit(Message::ClearHovers)
                    .into();
            layers.push(sort_scrim);
        }
        // Collection menu: same frozen-anchor scrim pattern, no
        // flyout. Search hits resolve live (results may have moved on
        // since the menu opened, and then nothing renders); home album
        // groups resolve against the catalog instead.
        if let Some(menu) = self.coll_menu.clone() {
            let hit: Option<(iced::Element<'_, Message>, String, String)> = match &menu {
                CollectionMenu::Album(id) => {
                    self.results.albums.iter().find(|a| &a.id == id).map(|a| {
                        let sub = match a.year {
                            Some(y) => self.language.album_sub_year(&a.artist, y),
                            None => self.language.album_sub(&a.artist),
                        };
                        (
                            self.art_thumb(self.album_cover(a).as_deref(), 56.0),
                            a.title.clone(),
                            sub,
                        )
                    })
                }
                CollectionMenu::Artist(id) => {
                    self.results.artists.iter().find(|a| &a.id == id).map(|a| {
                        let photo = a
                            .thumbnail_url
                            .as_deref()
                            .map(|u| yutu_core::hires_thumb_url(u, 544));
                        (
                            self.art_thumb(photo.as_deref(), 56.0),
                            a.name.clone(),
                            "Artist".to_string(),
                        )
                    })
                }
                CollectionMenu::Playlist(id) => self
                    .results
                    .playlists
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| {
                        (
                            self.playlist_art(p, 56.0),
                            p.title.clone(),
                            self.language.playlist_sub_full(&p.author, &p.detail),
                        )
                    }),
                CollectionMenu::HomeAlbum { album, artist } => self
                    .catalog
                    .values()
                    .find(|t| t.album.as_deref() == Some(album.as_str()) && &t.artist == artist)
                    .map(|t| {
                        let n = self
                            .catalog
                            .values()
                            .filter(|x| {
                                x.album.as_deref() == Some(album.as_str()) && &x.artist == artist
                            })
                            .count();
                        (
                            self.thumb_hero(t, 56.0),
                            album.clone(),
                            self.language.home_album_sub(artist, n),
                        )
                    }),
            };
            if let Some((art, title, sub)) = hit {
                let mut items: Vec<iced::Element<'_, Message>> = Vec::new();
                match menu {
                    CollectionMenu::Album(id) => {
                        let artist = self
                            .results
                            .albums
                            .iter()
                            .find(|a| a.id == id)
                            .map(|a| a.artist.clone())
                            .unwrap_or_default();
                        let tracks = self.album_tracks(&id);
                        if let Some(seed) = tracks.first().map(|t| t.id.clone()) {
                            items.push(menu_item(
                                "shuffle",
                                self.language.shuffle_play(),
                                Message::PlayAlbumShuffled(id.clone()),
                            ));
                            items.push(menu_item(
                                "radio",
                                self.language.start_mix(),
                                Message::StartRadio(seed),
                            ));
                        }
                        if !artist.is_empty() {
                            items.push(menu_item(
                                "user-round",
                                self.language.go_to_artist(),
                                Message::OpenArtist(artist),
                            ));
                        }
                    }
                    CollectionMenu::Artist(id) => {
                        let name = self
                            .results
                            .artists
                            .iter()
                            .find(|a| a.id == id)
                            .map(|a| a.name.clone())
                            .unwrap_or_default();
                        let tracks = self.artist_tracks(&name);
                        if let Some(seed) = tracks.first().map(|t| t.id.clone()) {
                            items.push(menu_item(
                                "shuffle",
                                self.language.shuffle_play(),
                                Message::PlayArtistShuffled(name),
                            ));
                            items.push(menu_item(
                                "radio",
                                self.language.start_mix(),
                                Message::StartRadio(seed),
                            ));
                        }
                    }
                    CollectionMenu::Playlist(id) => {
                        items.push(menu_item(
                            "play",
                            self.language.play_label(),
                            Message::PlaySearchPlaylist(id.clone()),
                        ));
                        items.push(menu_item(
                            "shuffle",
                            self.language.shuffle_play(),
                            Message::PlaySearchPlaylistShuffled(id),
                        ));
                    }
                    CollectionMenu::HomeAlbum { album, artist } => {
                        items.push(menu_item(
                            "play",
                            self.language.play_label(),
                            Message::PlayHomeAlbum {
                                album: album.clone(),
                                artist: artist.clone(),
                            },
                        ));
                        items.push(menu_item(
                            "plus",
                            self.language.add_to_queue(),
                            Message::QueueHomeAlbum {
                                album: album.clone(),
                                artist: artist.clone(),
                            },
                        ));
                        items.push(menu_item(
                            "shuffle",
                            self.language.shuffle_play(),
                            Message::ShuffleHomeAlbum {
                                album: album.clone(),
                                artist: artist.clone(),
                            },
                        ));
                        if let Some(seed) = self
                            .catalog
                            .values()
                            .find(|t| {
                                t.album.as_deref() == Some(album.as_str()) && t.artist == *artist
                            })
                            .map(|t| t.id.clone())
                        {
                            items.push(menu_item(
                                "radio",
                                self.language.start_mix(),
                                Message::StartRadio(seed),
                            ));
                        }
                        if !artist.is_empty() {
                            items.push(menu_item(
                                "user-round",
                                self.language.go_to_artist(),
                                Message::OpenArtist(artist),
                            ));
                        }
                    }
                }
                let coll_menu = container(
                    column![
                        row![
                            art,
                            column![text(title).size(16), text(sub).size(12).color(MUTED),]
                                .spacing(2)
                                .width(Fill),
                        ]
                        .spacing(10)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                        column(items).spacing(0),
                    ]
                    .spacing(8),
                )
                .style(card_bg)
                .padding(14)
                .width(300);
                let coll_scrim: iced::Element<'_, Message> =
                    mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                        .on_press(Message::CloseContext)
                        .on_enter(Message::ClearHovers)
                        .on_exit(Message::ClearHovers)
                        .into();
                let (mx, my) = menu_pos(self.menu_at, self.window);
                let coll_placed: iced::Element<'_, Message> = container(column![
                    iced::widget::Space::new().height(iced::Length::Fixed(my)),
                    row![
                        iced::widget::Space::new().width(iced::Length::Fixed(mx)),
                        coll_menu,
                    ],
                ])
                .width(Fill)
                .height(Fill)
                .into();
                layers.push(coll_scrim);
                layers.push(coll_placed);
            }
        }
        // Cookie cleanup modal: topmost (above menus), centered card over
        // a dismiss scrim. Esc/scrim keeps the file (safe default).
        #[cfg(feature = "innertube")]
        if let Some(dialog) = self.cookie_cleanup_dialog() {
            layers.push(dialog);
        }
        // Scrobbling dialog above everything else: same dimmed frame,
        // scrim/Esc closes it with edits kept.
        if let Some(dialog) = self.scrobble_dialog() {
            layers.push(dialog);
        }
        // Playlist-delete confirm above everything else: same dimmed
        // frame, scrim/Esc keeps the playlist (safe default).
        if let Some(dialog) = self.playlist_delete_dialog() {
            layers.push(dialog);
        }
        // Sign-out confirm above everything else: same dimmed frame,
        // scrim/Esc stays signed in (safe default).
        #[cfg(feature = "innertube")]
        if let Some(dialog) = self.signout_dialog() {
            layers.push(dialog);
        }
        // Always a Stack (even with one layer): flipping between bare
        // content and Stack would rebuild all widget state (search focus
        // and cursor included) every time a popup appears or vanishes.
        Stack::with_children(layers).into()
    }

    /// Now Playing sheet that overlays the center column (drawer slide).
    /// Gated on `drawer_anim` by the caller.
    pub(crate) fn drawer_layer(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let win_h = self.window.map(|s| s.height).unwrap_or(800.0);
        // The sheet lives in the center column below the header: a fixed
        // top reserve keeps the header (nav + search) always visible, and
        // the sheet runs to the bottom with no trailing gap. The open
        // animation shrinks the sheet upward instead of pushing past the
        // bottom edge, so the column height stays constant mid-glide.
        let center_h = (win_h - 148.0).max(300.0);
        let sheet_h = (center_h - DRAWER_TOP_RESERVE).max(0.0);
        let offset = (1.0 - self.drawer_anim).max(0.0) * sheet_h;
        let sheet: iced::Element<'_, Message> = container(
            column![
                row![
                    button(ctrl_box(btn_icon("chevron-down", 14.0, iced::Color::WHITE)))
                        .style(hover_row)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    text(self.language.now_playing())
                        .size(13)
                        .color(MUTED)
                        .width(Fill),
                ]
                .align_y(iced::alignment::Vertical::Center),
                self.drawer_view(),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(Fill)
        .height(Fill)
        .into();
        // Spacer plus Fill sheet: bottom leftover is always 0 at rest so
        // the drawer meets the player bar exactly like the sidebars.
        container(column![
            iced::widget::Space::new().height(iced::Length::Fixed(DRAWER_TOP_RESERVE + offset)),
            sheet,
        ])
        .width(Fill)
        .height(Fill)
        .into()
    }

    /// Drawer body without the header reserve (used under the persistent
    /// header when fully open, so no double gap appears).
    pub(crate) fn drawer_body_layer(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let win_h = self.window.map(|s| s.height).unwrap_or(800.0);
        let center_h = (win_h - 148.0).max(300.0);
        let sheet_h = (center_h - DRAWER_TOP_RESERVE).max(0.0);
        let offset = (1.0 - self.drawer_anim).max(0.0) * sheet_h;
        let sheet: iced::Element<'_, Message> = container(
            column![
                row![
                    button(ctrl_box(btn_icon("chevron-down", 14.0, iced::Color::WHITE)))
                        .style(hover_row)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    text(self.language.now_playing())
                        .size(13)
                        .color(MUTED)
                        .width(Fill),
                ]
                .align_y(iced::alignment::Vertical::Center),
                self.drawer_view(),
            ]
            .spacing(8),
        )
        .style(card_bg)
        .padding(16)
        .width(Fill)
        .height(Fill)
        .into();
        container(column![
            iced::widget::Space::new().height(iced::Length::Fixed(offset)),
            sheet,
        ])
        .width(Fill)
        .height(Fill)
        .into()
    }
}
