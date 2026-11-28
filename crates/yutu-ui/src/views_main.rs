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
        use iced::widget::{button, column, text};
        let (groups, total) = self.history_groups(now_unix(), self.history_limit);
        let shown: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
        let head: iced::Element<'_, Message> = if self.history_filter.trim().is_empty() {
            text(self.language.history_plays(total))
                .size(12)
                .color(tk().text_muted)
                .into()
        } else {
            text(self.language.history_matches(total))
                .size(12)
                .color(tk().text_muted)
                .into()
        };
        let mut body = column![
            text(self.language.nav_history()).size(22),
            head,
            // Same rules as the playlist search: compact pill at rest,
            // grows while focused.
            self.edit_menu(
                pill_search_input(
                    self.language.history_filter_placeholder(),
                    &self.history_filter,
                    Message::HistoryFilterChanged,
                    self.history_search_width(),
                ),
                &self.history_filter,
                Some(Message::HistoryFilterChanged),
                false,
            ),
        ]
        .spacing(10);
        if groups.is_empty() {
            let empty = if self.history_filter.trim().is_empty() {
                "No plays yet. Press play and they will show up here."
            } else {
                "No matches. Try a different spelling."
            };
            body = body.push(text(empty).size(12).color(tk().text_muted));
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
                    .padding([12, 16])
                    .height(iced::Length::Fixed(PILL_H)),
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
                        tk().text
                    ))
                    .style(row_click)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(3)
                    .width(Fill),
                    text(fmt_len(self.track_secs(t)))
                        .size(11)
                        .color(tk().text_muted),
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

    /// Library panel drawn at width `w`: the icon rail below 150px,
    /// the full panel above. A Narrow drawer always draws the full
    /// panel at its final width and reveals it through a clip while
    /// it glides, so the content never reflows mid-animation.
    pub(crate) fn sidebar_view_at(&self, w: f32) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container};
        use iced::Fill;
        let mode = self.layout();
        let drawer = mode == LayoutMode::Narrow;
        // Pin only where it means something (push modes); the Narrow
        // drawer gets a close button instead.
        let pin_btn: iced::Element<'_, Message> = match mode {
            LayoutMode::Narrow => button(btn_icon("x", 16.0, tk().text_muted))
                .style(icon_hover)
                .on_press(Message::CloseDrawers)
                .padding(8)
                .into(),
            LayoutMode::Medium => iced::widget::Space::new().into(),
            _ => button(btn_icon(
                "panel-left",
                16.0,
                if self.sidebar_pinned {
                    tk().accent
                } else {
                    tk().text_muted
                },
            ))
            .style(icon_hover)
            .on_press(Message::ToggleSidebarPin)
            .padding(8)
            .into(),
        };
        if drawer {
            return container(self.sidebar_full(self.sidebar_full_w(), pin_btn))
                .width(iced::Length::Fixed(w))
                .height(Fill)
                .clip(true)
                .into();
        }
        if w < 150.0 {
            // Logo header mirrors the full panel's: same 12px top inset,
            // same 34px row and 10px gap, so the logo and the nav stay
            // put when the rail expands.
            let header = container(logo_mark(34.0))
                .width(Fill)
                .height(iced::Length::Fixed(34.0))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center);
            return container(
                column![
                    header,
                    // 6 + 2 + 2 column spacing = the full panel's 10px gap.
                    iced::widget::Space::new().height(iced::Length::Fixed(6.0)),
                    button(btn_icon("house", 18.0, tk().text))
                        .style(nav_style(self.is_nav(&Route::Home)))
                        .on_press(Message::Navigate(Route::Home))
                        .padding(10),
                    button(btn_icon("search", 18.0, tk().text))
                        .style(nav_style(self.is_nav(&Route::Search)))
                        .on_press(Message::Navigate(Route::Search))
                        .padding(10),
                    button(btn_icon("list-music", 18.0, tk().text))
                        .style(nav_style(self.is_nav(&Route::History)))
                        .on_press(Message::Navigate(Route::History))
                        .padding(10),
                ]
                .spacing(2),
            )
            .style(sidebar_bg)
            .padding(iced::Padding {
                top: 12.0,
                ..iced::Padding::new(8.0)
            })
            .width(iced::Length::Fixed(w))
            .height(Fill)
            .into();
        }
        self.sidebar_full(w, pin_btn)
    }

    /// Full library panel (logo, nav, "New playlist" button, playlist
    /// list) at width `w`.
    fn sidebar_full<'a>(
        &'a self,
        w: f32,
        pin_btn: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let rows: Vec<iced::Element<'_, Message>> = self
            .playlist_heads()
            .into_iter()
            .map(|pl| {
                let art: iced::Element<'_, Message> = if pl.id == Self::FAVORITES_ID {
                    self.favorites_art(40.0)
                } else {
                    pl.first
                        .and_then(|t| self.find_track(&t.id))
                        .map(|t| self.thumb(&t))
                        .unwrap_or_else(|| {
                            container(icon("music", 16.0, tk().text_muted))
                                .style(card_bg)
                                .padding(0)
                                .width(iced::Length::Fixed(40.0))
                                .height(iced::Length::Fixed(40.0))
                                .align_x(iced::alignment::Horizontal::Center)
                                .align_y(iced::alignment::Vertical::Center)
                                .into()
                        })
                };
                // Hover reveals a trailing play button; right-click
                // opens the playlist menu.
                let show_play = self.hovered_playlist.as_deref() == Some(pl.id) && pl.len > 0;
                let trailing: iced::Element<'_, Message> = if show_play {
                    button(btn_icon("play", 14.0, tk().on_accent))
                        .style(accent_button)
                        .on_press(Message::PlayPlaylist(pl.id.to_string()))
                        .padding(8)
                        .into()
                } else {
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(30.0))
                        .into()
                };
                let pl_id = pl.id.to_string();
                let pl_hover = pl.id.to_string();
                mouse_area(
                    row_button(
                        row![
                            art,
                            column![
                                btn_text(pl.title.to_string(), 13),
                                btn_text(self.language.playlist_sub(pl.len), 11)
                                    .color(tk().text_muted),
                            ]
                            .spacing(1)
                            .width(Fill),
                            trailing,
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenPlaylist(pl_id.clone()))
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
            text(self.language.no_playlists_above())
                .size(12)
                .color(tk().text_muted)
                .into()
        } else if self.sidebar_short() {
            // Short window: the whole panel scrolls instead (below).
            column(rows).spacing(2).into()
        } else {
            // Scrolls on its own below the "New playlist" button, so
            // the button stays reachable on long libraries.
            iced::widget::scrollable(column(rows).spacing(2).padding(iced::Padding {
                right: 10.0,
                ..iced::Padding::ZERO
            }))
            .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("sidebar")))
            .on_scroll(|v| Message::Scrolled("sidebar", v.absolute_offset().y))
            .height(Fill)
            .into()
        };
        let panel = container(
            column![
                row![
                    logo_mark(34.0),
                    text("Yutu Mux").size(22).color(tk().text).width(Fill),
                    self.sync_spinner(),
                    pin_btn,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
                column![
                    button(
                        row![
                            btn_icon("house", 14.0, tk().text),
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
                            btn_icon("search", 14.0, tk().text),
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
                            btn_icon("list-music", 14.0, tk().text),
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
                .push(
                    // The Narrow header has no room for the gear:
                    // Settings lives in the library drawer instead.
                    (self.layout() == LayoutMode::Narrow).then(|| {
                        button(
                            row![
                                btn_icon("settings", 14.0, tk().text),
                                btn_text(self.language.settings_title(), 14),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(nav_style(self.is_nav(&Route::Settings)))
                        .on_press(Message::Navigate(Route::Settings))
                        .padding(8)
                        .width(Fill)
                    }),
                )
                .spacing(2),
                column![
                    // Heads the playlist list, shaped like a playlist row
                    // (40px tile + label) so it reads as "add one here".
                    row_button(
                        row![
                            container(btn_icon("plus", 18.0, tk().text))
                                .style(card_bg)
                                .width(iced::Length::Fixed(40.0))
                                .height(iced::Length::Fixed(40.0))
                                .align_x(iced::alignment::Horizontal::Center)
                                .align_y(iced::alignment::Vertical::Center),
                            btn_text(self.language.new_playlist(), 13).width(Fill),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenNewPlaylist)
                    .padding(8)
                    .width(Fill),
                    list,
                ]
                .spacing(2),
            ]
            .spacing(10),
        );
        // Too short for the header, nav and new-playlist button plus
        // a few playlists: the whole panel scrolls as one list so every
        // playlist stays reachable (a Fill list would get no height).
        let panel: iced::Element<'_, Message> = if self.sidebar_short() {
            container(
                iced::widget::scrollable(panel.padding(iced::Padding {
                    right: 10.0,
                    ..iced::Padding::ZERO
                }))
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("sidebar")))
                .on_scroll(|v| Message::Scrolled("sidebar", v.absolute_offset().y))
                .height(Fill),
            )
            .into()
        } else {
            panel.into()
        };
        container(panel)
            .style(sidebar_bg)
            .padding(12)
            .width(iced::Length::Fixed(w))
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
                if is_current { tk().accent } else { tk().text },
            ))
            .padding(1)
            .width(Fill),
            container(
                text(fmt_len(self.track_secs(t)))
                    .size(11)
                    .color(tk().text_muted)
            )
            .padding(3),
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

    /// Main area with the floating side panels stacked above the slot
    /// row (Medium hover expand, Narrow drawers). The layers cover only
    /// the main area, so the player bar below stays usable. A floating
    /// panel swallows the pointer over its own bounds (never leaking
    /// hover or clicks to the rows underneath); a Narrow drawer also
    /// dims the content behind a scrim that closes it on click.
    pub(crate) fn with_panel_overlays<'a>(
        &'a self,
        row_layer: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{container, mouse_area, row, Space, Stack};
        use iced::Fill;
        let left = self.sidebar_overlay_w();
        let right = self.right_overlay_w();
        // Always a Stack with the row first, even with nothing floating:
        // switching Row <-> Stack here would rebuild the center and reset
        // its scroll whenever a rail expands.
        let mut layers: Vec<iced::Element<'a, Message>> = vec![row_layer];
        // Scrim only behind an open Narrow drawer: without one it would
        // dim the window and swallow every click.
        if self.layout() == LayoutMode::Narrow && (left.is_some() || right.is_some()) {
            layers.push(
                mouse_area(
                    container(Space::new())
                        .width(Fill)
                        .height(Fill)
                        .style(drawer_scrim),
                )
                .on_press(Message::CloseDrawers)
                .on_right_press(Message::CloseDrawers)
                .interaction(iced::mouse::Interaction::Idle)
                .into(),
            );
        }
        let float = |panel: iced::Element<'a, Message>| -> iced::Element<'a, Message> {
            mouse_area(container(panel).style(floating_panel_bg).height(Fill))
                .interaction(iced::mouse::Interaction::Idle)
                .into()
        };
        if let Some(w) = left {
            layers.push(
                row![float(self.sidebar_view_at(w)), Space::new().width(Fill)]
                    .height(Fill)
                    .into(),
            );
        }
        if let Some(w) = right {
            layers.push(
                row![Space::new().width(Fill), float(self.right_panel_view_at(w))]
                    .height(Fill)
                    .into(),
            );
        }
        Stack::with_children(layers).width(Fill).height(Fill).into()
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
                        Self::track_text(&t.title, &t.artist, 12, tk().text),
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
                let gw = (self.center_width() - 32.0).clamp(240.0, 760.0);
                let press_x = self.pl_press_snap.as_ref().and_then(|s| s.pos.map(|p| p.x));
                let slot = self.sidebar_slot_w();
                let left = WINDOW_PAD_X / 2.0 + slot + if slot > 0.0 { PANEL_GAP } else { 0.0 };
                let (gx, gy) = drag_ghost_pos(
                    self.cursor,
                    self.window,
                    gw,
                    TRACK_ROW_H,
                    press_x,
                    left + 8.0,
                )?;
                let ghost: iced::Element<'_, Message> = container(
                    row![
                        container(self.thumb(&t)).padding(2),
                        container(Self::track_text_single(&t.title, &t.artist, 14, tk().text))
                            .padding(3)
                            .width(Fill),
                        text(fmt_len(self.track_secs(&t)))
                            .size(12)
                            .color(tk().text_muted),
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

    /// Queue panel drawn at width `w` (thumbnail rail below 140px).
    /// The Narrow drawer draws the full panel at its final width and
    /// reveals it through a clip, like the library drawer.
    pub(crate) fn right_panel_view_at(&self, w: f32) -> iced::Element<'_, Message> {
        let mode = self.layout();
        if mode == LayoutMode::Narrow {
            return iced::widget::container(self.right_panel_inner(self.right_full_w()))
                .width(iced::Length::Fixed(w))
                .height(iced::Fill)
                .clip(true)
                .into();
        }
        self.right_panel_inner(w)
    }

    fn right_panel_inner(&self, w: f32) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
        use iced::Fill;
        let current = self.queue.current().cloned();
        let now: iced::Element<'_, Message> = match &current {
            Some(t) => column![
                text(self.language.now_playing())
                    .size(13)
                    .color(tk().text_muted),
                row![
                    self.thumb(t),
                    column![
                        text(t.title.clone()).size(15),
                        text(t.artist.clone()).size(12).color(tk().text_muted),
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
                text(self.language.now_playing())
                    .size(13)
                    .color(tk().text_muted),
                text(self.language.nothing_queued())
                    .size(14)
                    .color(tk().text_muted),
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
        // Single panel (Recent moved to the History page): a plain
        // heading where the tab row used to be, same height.
        let queue_title = container(btn_text(self.language.queue_tab(), 12)).padding(8);
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
                                tk().text
                            ))
                            .style(row_click)
                            .on_press(Message::PlayAutoplay(i))
                            .padding(3)
                            .width(Fill),
                            text(fmt_len(self.track_secs(t)))
                                .size(11)
                                .color(tk().text_muted),
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
                text(self.language.autoplay())
                    .size(13)
                    .color(tk().text_muted),
                column(auto_mini).spacing(0),
            ]
            .spacing(2)
            .into()
        };
        // "Playing from" context (a started mix), YouTube Music style:
        // small muted caption over the source name, above Up next.
        let playing_from: Option<iced::Element<'_, Message>> = self
            .playing_from
            .as_deref()
            .filter(|_| !self.queue.is_empty())
            .map(|ctx| {
                column![
                    text(self.language.playing_from())
                        .size(11)
                        .color(tk().text_muted),
                    // Up to two lines; a longer label ends in "…"
                    // (fitted to the real width at layout time).
                    iced::widget::responsive(move |size| {
                        text(fit_lines(ctx, size.width, 14.0, 2))
                            .size(14)
                            .color(tk().text)
                            .wrapping(iced::widget::text::Wrapping::WordOrGlyph)
                            .into()
                    })
                    .height(iced::Length::Shrink),
                ]
                .spacing(2)
                .width(Fill)
                .clip(true)
                .into()
            });
        let list: iced::Element<'_, Message> = {
            let mut parts: Vec<iced::Element<'_, Message>> = Vec::new();
            parts.extend(playing_from);
            parts.push(
                row![
                    text(self.language.up_next())
                        .size(13)
                        .color(tk().text_muted)
                        .width(Fill),
                    button(btn_text(self.language.clear_queue(self.queue.len()), 12))
                        .style(ghost_button)
                        .on_press(Message::QueueClear)
                        .padding([12, 16])
                        .height(iced::Length::Fixed(PILL_H)),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            );
            parts.push(
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
                    }),
                )
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("queue")))
                .on_scroll(|v| Message::Scrolled("queue", v.absolute_offset().y))
                .id(iced::widget::Id::new(QUEUE_SCROLL_ID))
                .height(Fill)
                .into(),
            );
            column(parts).spacing(4).into()
        };
        let pin_btn: iced::Element<'_, Message> = match self.layout() {
            LayoutMode::Narrow => button(btn_icon("x", 16.0, tk().text_muted))
                .style(icon_hover)
                .on_press(Message::CloseDrawers)
                .padding(6)
                .into(),
            LayoutMode::Medium => container(icon("list-music", 16.0, tk().accent))
                .padding(6)
                .into(),
            _ => button(btn_icon(
                "panel-right",
                16.0,
                if self.right_pinned {
                    tk().accent
                } else {
                    tk().text_muted
                },
            ))
            .style(icon_hover)
            .on_press(Message::ToggleRightPin)
            .padding(6)
            .into(),
        };
        if w < QUEUE_RAIL_CUTOFF_W && self.layout() != LayoutMode::Narrow {
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
            .width(iced::Length::Fixed(w))
            .height(Fill)
            .into();
        }
        container(column![row![pin_btn, queue_title].spacing(4), now, list,].spacing(8))
            .style(sidebar_bg)
            .padding(12)
            .width(iced::Length::Fixed(w))
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
            tk().text_muted,
        )))
        .style(icon_hover)
        .on_press(Message::ToggleMute)
        .padding(6)
        .into()
    }

    /// Mute button + volume slider, right-aligned in whatever width the
    /// parent row leaves, the slider capped at `slider_w`. The cap lives
    /// on an inner container laid out with loose limits: iced hands a
    /// `Fill` row child min = max = the leftover space, and
    /// `Limits::max_width` never drops below that min, so a cap set
    /// directly on the row child is silently ignored.
    fn volume_group(&self, slider_w: f32, spacing: f32, tip: bool) -> iced::Element<'_, Message> {
        use iced::widget::{container, row, text, tooltip};
        use iced::Fill;
        let slider = pointing_slider(0.0..=1.0, self.volume, Message::SetVolume)
            .step(0.01_f32)
            .style(themed_slider)
            .width(Fill);
        let slider: iced::Element<'_, Message> = if tip {
            tooltip(
                slider,
                text(if self.muted {
                    self.language.volume_muted(self.volume * 100.0)
                } else {
                    self.language.volume(self.volume * 100.0)
                })
                .size(12),
                tooltip::Position::Top,
            )
            .gap(8)
            .into()
        } else {
            slider.into()
        };
        container(
            container(
                row![self.mute_button(), slider]
                    .spacing(spacing)
                    .align_y(iced::alignment::Vertical::Center),
            )
            .width(Fill)
            .max_width(MUTE_BUTTON_W + spacing + slider_w),
        )
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    }

    pub(crate) fn player_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text, text::Wrapping};
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
        let volume_w = if tight { 80.0 } else { 120.0 };
        let main_gap = if tight { 8.0 } else { 12.0 };
        let like_button: iced::Element<'_, Message> = match &current {
            Some(t) => {
                let heart_color = if self.liked.contains(&t.id) {
                    tk().accent
                } else {
                    tk().text_muted
                };
                button(ctrl_box(heart_icon(
                    self.liked.contains(&t.id),
                    15.0,
                    heart_color,
                )))
                .style(icon_hover)
                .on_press(Message::Like(t.id.clone()))
                .padding(6)
                .into()
            }
            None => text("").size(1).into(),
        };
        let shuffle_button: iced::Element<'_, Message> = if self.queue.shuffle {
            button(ctrl_box(btn_icon("shuffle", 14.0, tk().on_accent)))
                .style(accent_button)
                .on_press(Message::ToggleShuffle)
                .padding(pad_side)
                .into()
        } else {
            button(ctrl_box(btn_icon("shuffle", 14.0, tk().text_muted)))
                .style(icon_hover)
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
                None => container(icon("music", 20.0, tk().text_muted))
                    .style(card_bg)
                    .padding(10)
                    .into(),
            };
            let info_like: iced::Element<'_, Message> = match &current {
                Some(t) => {
                    let heart_color = if self.liked.contains(&t.id) {
                        tk().accent
                    } else {
                        tk().text_muted
                    };
                    button(ctrl_box(heart_icon(
                        self.liked.contains(&t.id),
                        15.0,
                        heart_color,
                    )))
                    .style(icon_hover)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(6)
                    .into()
                }
                None => text("").size(1).into(),
            };
            let drawer_btn: iced::Element<'_, Message> = if self.drawer_open {
                button(ctrl_box(btn_icon("mic-vocal", 14.0, tk().on_accent)))
                    .style(accent_button)
                    .on_press(Message::ToggleDrawer)
                    .padding(7)
                    .into()
            } else {
                button(ctrl_box(btn_icon("mic-vocal", 14.0, tk().text_muted)))
                    .style(icon_hover)
                    .on_press(Message::ToggleDrawer)
                    .padding(7)
                    .into()
            };
            // One line each for title and artist (ellipsized to the
            // room beside the art and the two buttons) keeps the bar short.
            let budget = (((ww - MINI_INFO_CHROME) / 7.0) as usize).clamp(12, 80);
            let info = row![
                info_art,
                column![
                    text(truncate_title(&title, budget))
                        .size(13)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                    text(truncate_title(&artist, budget))
                        .size(12)
                        .color(tk().text_muted)
                        .wrapping(Wrapping::None),
                ]
                .spacing(1)
                .width(Fill),
                info_like,
                self.mini_player_button(),
                drawer_btn,
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center);
            let transport = row![
                {
                    let shuffle_btn: iced::Element<'_, Message> = if self.queue.shuffle {
                        button(ctrl_box(btn_icon("shuffle", 14.0, tk().on_accent)))
                            .style(accent_button)
                            .on_press(Message::ToggleShuffle)
                            .padding(6)
                            .into()
                    } else {
                        button(ctrl_box(btn_icon("shuffle", 14.0, tk().text_muted)))
                            .style(icon_hover)
                            .on_press(Message::ToggleShuffle)
                            .padding(6)
                            .into()
                    };
                    shuffle_btn
                },
                button(ctrl_box(btn_icon("skip-back", 18.0, tk().text)))
                    .style(icon_hover)
                    .on_press(Message::Prev)
                    .padding(6),
                button(ctrl_box(btn_icon(play_icon, 18.0, tk().on_accent)))
                    .style(accent_button)
                    .on_press(Message::TogglePause)
                    .padding(8),
                button(ctrl_box(btn_icon("skip-forward", 18.0, tk().text)))
                    .style(icon_hover)
                    .on_press(Message::Next)
                    .padding(6),
                repeat_button(repeat_icon, repeat_active, 6),
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center);
            // Transport on the left, volume on the right: one row
            // instead of two (the slider shrinks first, never the icons).
            let controls = row![transport, self.volume_group(MINI_VOLUME_W, 6.0, false),]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center);
            let mut bar_col = iced::widget::Column::new().spacing(6);
            if let Some(status) = self.status_row() {
                bar_col = bar_col.push(status);
            }
            return container(
                bar_col
                    .push(info)
                    // Fixed side slots keep the seek bar centered on the
                    // same column as the bar above it.
                    .push(
                        row![
                            container(text(fmt_time(shown_pos)).size(11).color(tk().text_muted))
                                .width(iced::Length::Fixed(MINI_SIDE_W))
                                .align_x(iced::alignment::Horizontal::Center),
                            seek_bar::seek_bar(shown_pos as f32, slider_max as f32),
                            container(text(fmt_time(duration)).size(11).color(tk().text_muted))
                                .width(iced::Length::Fixed(MINI_SIDE_W))
                                .align_x(iced::alignment::Horizontal::Center),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .push(controls),
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
                            None => container(icon("music", 20.0, tk().text_muted))
                                .style(card_bg)
                                .padding(10)
                                .into(),
                        };
                        let artist_el: iced::Element<'_, Message> = match &current {
                            Some(t) => button(btn_text(artist, 12).color(tk().text_muted))
                                .style(row_click)
                                .on_press(Message::OpenTrackArtist(t.id.clone()))
                                .padding(0)
                                .into(),
                            None => text(artist).size(12).color(tk().text_muted).into(),
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
                            button(ctrl_box(btn_icon("skip-back", 18.0, tk().text)))
                                .style(icon_hover)
                                .on_press(Message::Prev)
                                .padding(pad_side),
                            button(ctrl_box(btn_icon(play_icon, 18.0, tk().on_accent)))
                                .style(accent_button)
                                .on_press(Message::TogglePause)
                                .padding(pad_play),
                            button(ctrl_box(btn_icon("skip-forward", 18.0, tk().text)))
                                .style(icon_hover)
                                .on_press(Message::Next)
                                .padding(pad_side),
                            repeat_button(repeat_icon, repeat_active, pad_side),
                        ]
                        .spacing(transport_gap)
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            text(fmt_time(shown_pos)).size(11).color(tk().text_muted),
                            seek_bar::seek_bar(shown_pos as f32, slider_max as f32),
                            text(fmt_time(duration)).size(11).color(tk().text_muted),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    ]
                    .spacing(2)
                    .align_x(iced::alignment::Horizontal::Center)
                    .width(iced::Length::FillPortion(center_part)),
                    // Right-aligned group: on a tight bar only the volume
                    // slider gives up width (capped at its normal size),
                    // so the mute and Now Playing icons never shrink.
                    container(
                        row![
                            self.volume_group(volume_w, 8.0, true),
                            self.mini_player_button(),
                            {
                                if self.drawer_open {
                                    button(ctrl_box(btn_icon("mic-vocal", 14.0, tk().on_accent)))
                                        .style(accent_button)
                                        .on_press(Message::ToggleDrawer)
                                        .padding(7)
                                } else {
                                    button(ctrl_box(btn_icon("mic-vocal", 14.0, tk().text_muted)))
                                        .style(icon_hover)
                                        .on_press(Message::ToggleDrawer)
                                        .padding(7)
                                }
                            },
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                    )
                    .align_x(iced::alignment::Horizontal::Right)
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
    /// highlight, so both read through here. Only the live track has
    /// a cursor: another song's lyrics (opened from a context menu)
    /// must never follow the playing song's clock.
    pub(crate) fn lyric_cursor_at(&self, id: &str) -> Option<usize> {
        if !self.lyrics_is_live(id) {
            return None;
        }
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

    /// True when `id` is the track currently loaded in the player.
    pub(crate) fn lyrics_is_live(&self, id: &str) -> bool {
        self.queue.current().is_some_and(|t| t.id == id)
    }

    /// Height of one lyric row: its pre-broken lines (see
    /// [`Self::lyric_verse_lines`], the same for every distance) times
    /// the sized line height, plus button padding.
    /// `distance` = lines away from the current one (sizes follow
    /// [`crate::mini_player::verse_style`], default 1.3 line height).
    /// `romaji` = the romaji line under the verse (2px gap plus its own
    /// pre-broken lines at the smaller size).
    pub(crate) fn lyric_row_h(
        text: &str,
        romaji: Option<&str>,
        distance: usize,
        col_w: f32,
    ) -> f32 {
        let (size, _, _) = crate::mini_player::verse_style(distance as f32);
        let size = size * LYRICS_VIEW_SCALE;
        let line_h = |size: f32| {
            iced::widget::text::LineHeight::default()
                .to_absolute(iced::Pixels(size))
                .0
        };
        let rows = |s: String| s.lines().count().max(1) as f32;
        let verse = rows(Self::lyric_verse_lines(text, col_w)) * line_h(size);
        let roma = romaji.map_or(0.0, |r| {
            2.0 + rows(Self::lyric_romaji_lines(r, col_w)) * line_h(romaji_size(size))
        });
        verse + roma + LYRIC_BTN_PAD
    }

    /// Text width of a lyric row in a column `col_w` wide, less the
    /// break slack.
    fn lyric_text_w(col_w: f32) -> f32 {
        (col_w - LYRIC_ROW_CHROME - VERSE_BREAK_SLACK).max(120.0)
    }

    /// Size of the current (largest) synced verse in the lyrics views.
    fn lyric_top_size() -> f32 {
        crate::mini_player::verse_style(0.0).0 * LYRICS_VIEW_SCALE
    }

    /// A synced verse broken once at the current line's style (bold,
    /// largest size): drawn this way at every distance, so its words
    /// never hop between lines while it grows or shrinks.
    pub(crate) fn lyric_verse_lines(text: &str, col_w: f32) -> String {
        verse_breaks(
            text,
            Self::lyric_text_w(col_w),
            Self::lyric_top_size(),
            bold_font(),
        )
    }

    /// Romaji counterpart of [`Self::lyric_verse_lines`].
    pub(crate) fn lyric_romaji_lines(romaji: &str, col_w: f32) -> String {
        verse_breaks(
            romaji,
            Self::lyric_text_w(col_w),
            romaji_size(Self::lyric_top_size()),
            body_font(),
        )
    }

    /// Absolute drawer-scroll y that centers verse `at` (index into
    /// the visible lines): hero estimate, exact pitch walk over the
    /// cached rows (stanza gaps included, current row at its larger
    /// size), viewport centering, floored at zero. Takes the cursor
    /// instead of recomputing it (one clone per tick, not two).
    pub(crate) fn lyric_follow_y(&self, id: &str, at: usize) -> Option<f32> {
        self.lyric_center_y(id, at).map(|t| t.max(0.0))
    }

    /// [`Self::lyric_follow_y`] before the floor at zero: differences
    /// between two verses stay exact near the top.
    pub(crate) fn lyric_center_y(&self, id: &str, at: usize) -> Option<f32> {
        let (_, cached) = self.lyrics_display_entry(id)?;
        if cached.synced.is_empty() || cached.instrumental {
            return None;
        }
        // Same width formula as the drawer lyrics column.
        let col_w = self.lyrics_col_w();
        // The scrollable's measured height (it shrinks under the sticky
        // header and in short windows); before the first report, window
        // minus header/player chrome (approximate: only shifts the
        // center, never loses the verse).
        let viewport = self.drawer_view_h.unwrap_or_else(|| {
            let wh = self.window.map(|s| s.height).unwrap_or(800.0);
            (wh - LYRIC_VIEWPORT_PAD).max(120.0)
        });
        // Two-column Now Playing: only the provider stepper sits above
        // the lyrics inside the scroll (the hero lives beside it).
        let mut y = if self.np_two_col() {
            LYRIC_STEPPER_H + LYRIC_LIST_GAP
        } else {
            LYRIC_HERO_H + LYRIC_LIST_GAP
        };
        let mut vi = 0usize;
        let mut target = None;
        for l in &cached.synced {
            if l.text.trim().is_empty() {
                y += LYRIC_GAP_H + LYRIC_ROW_SPACING;
                continue;
            }
            let current = vi == at;
            let romaji = self.romaji_line(&l.text);
            let h = Self::lyric_row_h(&l.text, romaji, vi.abs_diff(at), col_w);
            if current {
                target = Some(y + h / 2.0 - viewport / 2.0);
            }
            y += h + LYRIC_ROW_SPACING;
            vi += 1;
        }
        target
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
            // Never while parked at the very top: the user scrolled up
            // to see the cover and title, and early verses sit close
            // enough to the top that resuming would drag them back down.
            if let Some(y) = self
                .lyric_follow_y(&id, at)
                .map(|y| self.clamp_follow(y))
                .filter(|_| self.drawer_scroll > LYRIC_TOP_HOLD_PX)
            {
                if (self.drawer_scroll - y).abs() <= LYRIC_RESUME_PX {
                    self.lyrics_follow = true;
                }
            }
            self.lyrics_follow_at = Some((id, at));
            return iced::Task::none();
        }
        // Every tick while following, measure the live row: the
        // estimate below is only a first guess (wrapping in narrow
        // columns and short viewports drift it), the probe corrects it.
        let probe = Self::lyric_probe_task(id.clone(), at);
        if !changed_track && self.lyrics_follow_at == Some((id.clone(), at)) {
            return probe;
        }
        let prev = self.lyrics_follow_at.replace((id.clone(), at));
        // Next verse of a settled follow: the measured target plus the
        // modeled distance between the two verses at their final sizes.
        // Lines are still resizing, so a measurement now would land
        // short and then creep (a bounce); the model's wrap error
        // cancels in the difference. Both ends stay unclamped: near the
        // top the settled target sits above zero's floor, and stepping
        // from the floored value overshot, then crept back (a bounce).
        let stepped = match (&prev, &self.lyrics_target_for) {
            (Some(p), Some(t)) if !changed_track && p == t && p.1.abs_diff(at) <= 3 => self
                .lyric_center_y(&id, p.1)
                .zip(self.lyric_center_y(&id, at))
                .map(|(a, b)| self.lyrics_target + b - a),
            _ => None,
        };
        let jump = match stepped.or_else(|| self.lyric_center_y(&id, at)) {
            Some(y) => {
                self.lyrics_target = y;
                self.lyrics_target_for = Some((id.clone(), at));
                if self.animations_enabled {
                    // The ticker glides there (smooth follow).
                    iced::Task::none()
                } else {
                    let y = self.clamp_follow(y);
                    self.lyrics_cmds = [y, f32::NAN, f32::NAN];
                    Self::lyrics_scroll_task(y)
                }
            }
            None => iced::Task::none(),
        };
        iced::Task::batch([jump, probe])
    }

    /// Measure visible lyric row `at` in the drawer scrollable; the
    /// result arrives as [`Message::LyricRowMeasured`] (nothing when
    /// the row or the scrollable is not on screen).
    pub(crate) fn lyric_probe_task(track: String, at: usize) -> iced::Task<Message> {
        iced::advanced::widget::operate(LyricRowProbe {
            track,
            at,
            row_id: lyric_row_id(at),
            scroll_id: iced::widget::Id::new(LYRICS_SCROLL_ID),
            scroll: None,
            row: None,
        })
        .map(Message::LyricRowMeasured)
    }

    /// Measurement of the followed verse on the first frame after the
    /// lines finish resizing (see [`Self::verse_gliding`]): confirms
    /// the stepped target on the settled layout right away instead of
    /// at the next tick. None when the drawer is not following.
    pub(crate) fn lyric_glide_probe(&mut self) -> Option<iced::Task<Message>> {
        let resizing = self.verse_gliding();
        let settle = std::mem::replace(&mut self.lyric_probe_settle, resizing);
        if resizing || !settle || !self.drawer_open || !self.lyrics_follow {
            return None;
        }
        let (track, at) = self.lyrics_follow_at.clone()?;
        Some(Self::lyric_probe_task(track, at))
    }

    /// Apply a measured row: retarget the follow to its exact center
    /// while it is still the followed verse (stale probes and manual
    /// takeovers are ignored). Snaps directly with animations off.
    /// Ignored while lines resize after a verse change: that layout is
    /// mid-way, the stepped target already points at the settled one
    /// and a probe right after the resize confirms it.
    pub(crate) fn apply_lyric_row(&mut self, g: LyricRowGeom) -> iced::Task<Message> {
        if !self.drawer_open
            || !self.lyrics_follow
            || self.lyrics_follow_at.as_ref() != Some(&(g.track.clone(), g.at))
        {
            return iced::Task::none();
        }
        self.drawer_view_h = Some(g.view_h);
        if self.verse_gliding() {
            return iced::Task::none();
        }
        self.lyrics_target_for = Some((g.track.clone(), g.at));
        let y = g.raw_center_y();
        if (y - self.lyrics_target).abs() <= 1.0 {
            return iced::Task::none();
        }
        self.lyrics_target = y;
        if self.animations_enabled {
            iced::Task::none()
        } else {
            let y = self.clamp_follow(y);
            self.lyrics_cmds = [y, f32::NAN, f32::NAN];
            Self::lyrics_scroll_task(y)
        }
    }

    /// Furthest the drawer lyrics can scroll (content minus viewport),
    /// once both are measured.
    pub(crate) fn drawer_max_y(&self) -> Option<f32> {
        let content = self.drawer_content_h?;
        let view = self.drawer_view_h?;
        Some((content - view).max(0.0))
    }

    /// Follow target bounded to where the drawer can actually scroll: a
    /// target past the end would glide forever against the clamp.
    pub(crate) fn clamp_follow(&self, y: f32) -> f32 {
        match self.drawer_max_y() {
            Some(max) => y.clamp(0.0, max),
            None => y.max(0.0),
        }
    }

    /// True when offset `y` is the echo of a recent follow command:
    /// exactly, or as the scrollable clamped it to its end.
    pub(crate) fn lyrics_echo(&self, y: f32) -> bool {
        let max = self.drawer_max_y();
        self.lyrics_cmds
            .iter()
            .any(|&c| c == y || max.is_some_and(|m| c > m && (y - m).abs() <= 0.5))
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

    /// On the frame the expanded queue list reappears (hover expand,
    /// pin, drawer open), scroll its fresh scrollable back to the last
    /// offset the user left it at instead of the top.
    pub(crate) fn queue_scroll_restore(&mut self) -> Option<iced::Task<Message>> {
        let shown = self.queue_list_visible();
        let appeared = shown && !self.queue_list_shown;
        self.queue_list_shown = shown;
        let y = self.scroll_offsets.get("queue").copied().unwrap_or(0.0);
        (appeared && y > 0.0).then(|| {
            iced::advanced::widget::operate(
                iced::advanced::widget::operation::scrollable::scroll_to::<Message>(
                    iced::widget::Id::new(QUEUE_SCROLL_ID),
                    iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                        x: None,
                        y: Some(y),
                    },
                ),
            )
        })
    }

    /// One follow-glide step: ease the drawer toward the verse target
    /// (ease-out: 25% of the remaining gap per tick, 2px floor) and
    /// command the scrollable there. During a verse change the scroll
    /// instead runs on the line resize's own clock (same easing, from
    /// where it started toward the live, re-measured target), so both
    /// finish together: catching up early and then backing off as the
    /// lines settle read as a bounce. `None` when settled, closed,
    /// disengaged, or unfollowed. The commanded offset is recorded so
    /// its echo never trips the manual-takeover check (snaps land
    /// exactly: only real user scrolls differ).
    pub(crate) fn lyrics_glide_task(&mut self) -> Option<iced::Task<Message>> {
        if !self.drawer_open || !self.lyrics_follow || self.lyrics_follow_at.is_none() {
            return None;
        }
        let target = self.clamp_follow(self.lyrics_target);
        // Keyed by the verse change (its start instant), so each change
        // starts from where the drawer was when it began, whichever of
        // the follow retarget and the verse change ran first.
        let running = self.verse_glide.filter(|g| g.running());
        if let Some(g) = running {
            if self.lyrics_glide_from.is_none_or(|(at, _)| at != g.started) {
                self.lyrics_glide_from = Some((g.started, self.drawer_scroll));
            }
        }
        let synced = running.zip(self.lyrics_glide_from.map(|(_, y)| y));
        let y = match synced {
            Some((g, from)) => {
                let y = from + (target - from) * g.progress();
                if (y - self.drawer_scroll).abs() <= 0.25 {
                    return None;
                }
                y
            }
            None => {
                self.lyrics_glide_from = None;
                let gap = target - self.drawer_scroll;
                if gap.abs() <= 1.0 {
                    return None;
                }
                let step = (gap.abs() * 0.25).max(2.0);
                ease_toward(self.drawer_scroll, target, step)
            }
        };
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
            && (self.drawer_scroll - self.clamp_follow(self.lyrics_target)).abs() > 1.0
    }

    /// Shared synced-lyrics body (lyrics page + Now Playing drawer):
    /// loading / instrumental / empty / plain states included.
    ///
    /// Drawer follow geometry (see [`lyric_follow_y`]) mirrors the row
    /// construction below exactly (gaps, pitches, current-row size):
    /// keep the two in sync when this layout changes.
    /// `col_w` = the lyrics column width (synced verses break to it).
    pub(crate) fn lyrics_content(&self, id: &str, col_w: f32) -> iced::Element<'_, Message> {
        use iced::widget::{column, container, text};
        use iced::Fill;
        // The mini player's look (centered lines, white text fading
        // with distance from the current line, which is bold and large),
        // as a full scrollable list with click-to-seek rows.
        let notice = |s: String| -> iced::Element<'_, Message> {
            text(s)
                .size(13)
                .color(tk().text_muted)
                .align_x(iced::alignment::Horizontal::Center)
                .width(Fill)
                .into()
        };
        match self.lyrics_display_entry(id) {
            Some((_, cached)) if cached.instrumental => notice(self.language.instrumental()),
            // Not the playing song: static text in the unsynced style
            // (stanza gaps kept), no highlight and no click-to-seek
            // (that would seek the other song).
            Some((_, cached)) if !cached.synced.is_empty() && !self.lyrics_is_live(id) => column(
                cached
                    .synced
                    .iter()
                    .map(|l| -> iced::Element<'_, Message> {
                        let line = l.text.trim();
                        if line.is_empty() {
                            iced::widget::Space::new()
                                .height(iced::Length::Fixed(10.0))
                                .width(Fill)
                                .into()
                        } else {
                            self.verse_with_romaji(
                                text(line.to_string())
                                    .size(13)
                                    .color(tk().text.scale_alpha(0.72))
                                    .align_x(iced::alignment::Horizontal::Center)
                                    .width(Fill),
                                line,
                                13.0,
                                0.72,
                            )
                        }
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(6)
            .into(),
            Some((_, cached)) if !cached.synced.is_empty() => {
                // Highlight reads through the shared cursor so the
                // follow geometry can never disagree with it.
                let at = self.lyric_cursor_at(id);
                let focus = self.verse_focus(id, at);
                let mut rows: Vec<iced::Element<'_, Message>> = Vec::new();
                let mut vi = 0usize;
                for l in &cached.synced {
                    if l.text.trim().is_empty() {
                        rows.push(
                            iced::widget::Space::new()
                                .height(iced::Length::Fixed(10.0))
                                .width(Fill)
                                .into(),
                        );
                        continue;
                    }
                    let (size, alpha, bold) = crate::mini_player::verse_style(vi as f32 - focus);
                    let size = size * LYRICS_VIEW_SCALE;
                    let row_id = lyric_row_id(vi);
                    vi += 1;
                    let ms = l.start_ms;
                    // Broken once at the highlighted style: the line
                    // keeps its words per row while it grows or shrinks.
                    let mut line = text(Self::lyric_verse_lines(&l.text, col_w))
                        .size(size)
                        .color(tk().text.scale_alpha(alpha))
                        .align_x(iced::alignment::Horizontal::Center)
                        .width(Fill);
                    if bold {
                        line = line.font(bold_font());
                    }
                    let verse = self.verse_with_romaji_broken(line, &l.text, size, alpha, col_w);
                    // Id'd wrapper: the drawer follow measures the live
                    // row through it (exact wrap, no estimate).
                    let row_el: iced::Element<'_, Message> = container(
                        row_button(verse)
                            .style(glass_row)
                            .on_press(Message::SeekToMs(ms))
                            .padding(4)
                            .width(Fill),
                    )
                    .id(row_id)
                    .width(Fill)
                    .into();
                    rows.push(row_el);
                }
                column(rows).spacing(6).into()
            }
            // Unsynced: the whole text, centered and quiet (no line is
            // current), as in the mini player.
            Some((_, cached)) => column(
                cached
                    .plain
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(|l| {
                        self.verse_with_romaji(
                            text(l.to_string())
                                .size(13)
                                .color(tk().text.scale_alpha(0.72))
                                .align_x(iced::alignment::Horizontal::Center)
                                .width(Fill),
                            l,
                            13.0,
                            0.72,
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .spacing(6)
            .into(),
            None if self.lyrics_fetch_pending(id) => {
                notice(self.language.loading_lyrics(self.lyrics_provider.label()))
            }
            None => notice(self.language.no_lyrics()),
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
        // Japanese lyrics add the romaji quick toggle on the right,
        // mirrored by a spacer so the provider label stays centered.
        let romaji = cfg!(feature = "romaji") && self.lyrics_shown_japanese(track_id);
        let mut items: Vec<iced::Element<'_, Message>> = Vec::new();
        if romaji {
            items.push(
                iced::widget::Space::new()
                    .width(iced::Length::Fixed(ROMAJI_TOGGLE_W))
                    .into(),
            );
        }
        items.extend::<[iced::Element<'_, Message>; 3]>([
            button(btn_icon("chevron-left", 14.0, tk().text_muted))
                .style(icon_hover)
                .on_press(Message::CycleLyricsProvider(-1))
                .padding(4)
                .into(),
            text(label.to_string())
                .size(12)
                .color(tk().text_muted)
                .width(iced::Length::Fixed(LYRICS_PROVIDER_LABEL_W))
                .align_x(iced::alignment::Horizontal::Center)
                .into(),
            button(btn_icon("chevron-right", 14.0, tk().text_muted))
                .style(icon_hover)
                .on_press(Message::CycleLyricsProvider(1))
                .padding(4)
                .into(),
        ]);
        if romaji {
            items.push(self.romaji_quick_toggle());
        }
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
            text(t.artist.clone()).size(15).color(tk().text_muted),
            text(t.album.clone().unwrap_or_else(|| "Single".into()))
                .size(12)
                .color(tk().text_muted),
            row![button(heart_icon(
                self.liked.contains(&t.id),
                16.0,
                if self.liked.contains(&t.id) {
                    tk().accent
                } else {
                    tk().text_muted
                }
            ))
            .style(icon_hover)
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
                        text(t.artist.clone()).size(12).color(tk().text_muted),
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
        // Wide content: art and titles stay put on the left while only
        // the lyrics column scrolls (no sticky header needed).
        if self.np_two_col() {
            let lyrics_col = scrollable(
                column![
                    container(stepper)
                        .width(Fill)
                        .align_x(iced::alignment::Horizontal::Center),
                    container(self.lyrics_content(&t.id, self.lyrics_col_w()))
                        .width(iced::Length::Fixed(self.lyrics_col_w())),
                ]
                .spacing(LYRIC_LIST_GAP)
                .width(iced::Length::Fixed(self.lyrics_col_w()))
                .padding(iced::Padding {
                    bottom: 40.0,
                    ..iced::Padding::ZERO
                }),
            )
            .height(Fill)
            .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
            .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("drawer")))
            .id(iced::widget::Id::new(LYRICS_SCROLL_ID))
            .on_scroll(|v| {
                Message::DrawerScrolled(
                    v.absolute_offset().y,
                    v.bounds().height,
                    v.content_bounds().height,
                )
            });
            let body: iced::Element<'_, Message> = container(
                row![
                    container(hero).width(iced::Length::Fixed(NP_HERO_COL_W)),
                    lyrics_col,
                ]
                .spacing(NP_COL_GAP)
                .height(Fill),
            )
            .width(Fill)
            .height(Fill)
            .padding(iced::Padding {
                top: 24.0,
                ..iced::Padding::ZERO
            })
            .align_x(iced::alignment::Horizontal::Center)
            .into();
            let track_id = t.id.clone();
            return mouse_area(body)
                .on_right_press(Message::OpenContext(track_id))
                .into();
        }
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
                    container(self.lyrics_content(&t.id, self.lyrics_col_w()))
                        .width(iced::Length::Fixed(self.lyrics_col_w())),
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
            .on_scroll(|v| {
                Message::DrawerScrolled(
                    v.absolute_offset().y,
                    v.bounds().height,
                    v.content_bounds().height,
                )
            }),
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

    /// Back/forward arrows show only when the center has room for
    /// them next to a usable search field (mouse buttons and the
    /// keyboard still navigate without them).
    pub(crate) fn header_nav_arrows(&self) -> bool {
        self.center_width() >= HEADER_ARROWS_MIN_CENTER
    }

    /// Header items left of the search pill (each plus its 4px gap):
    /// the Narrow menu button, then the nav arrows.
    pub(crate) fn header_lead_w(&self) -> f32 {
        let menu = if self.layout() == LayoutMode::Narrow {
            Self::NAV_BTN_W + 4.0
        } else {
            0.0
        };
        let arrows = if self.header_nav_arrows() {
            2.0 * (Self::NAV_BTN_W + 4.0)
        } else {
            0.0
        };
        menu + arrows
    }

    /// Header items right of the pill: the Narrow queue button or the
    /// settings gear (plus the gap before it).
    pub(crate) fn header_trail_w(&self) -> f32 {
        if self.layout() == LayoutMode::Narrow {
            Self::NAV_BTN_W + 4.0
        } else {
            Self::SETTINGS_W + 4.0
        }
    }

    /// Pill left edge in window coords (base pad + left slot + gap +
    /// leading header buttons).
    pub(crate) fn search_pill_left(&self) -> f32 {
        let slot = self.sidebar_slot_w();
        let gap = if slot > 0.0 { PANEL_GAP } else { 0.0 };
        WINDOW_PAD_X / 2.0 + slot + gap + self.header_lead_w()
    }

    /// Pill geometry shared by the header pill, the suggest popup and
    /// the scrim hole (same left + width everywhere, so popup edges
    /// match the input exactly and only the input stays clickable).
    /// Expanded search width shared by both search fields: the header
    /// pill target, shrunk to the room the header center leaves.
    pub(crate) fn search_full_width(&self) -> f32 {
        let search_w = self.search_width();
        let ww = self.window_w();
        // Push modes size against the right panel's target, not its
        // animated width, so the pill never jitters while it glides.
        let right_glide = if self.layout().panels_overlay() {
            0.0
        } else {
            self.right_target() - self.right_slot_w()
        };
        let chrome = self.header_lead_w() + self.header_trail_w();
        let center_avail = self.center_width() - right_glide - chrome - 16.0;
        let full = if center_avail < search_w {
            center_avail.max(140.0)
        } else {
            search_w
        };
        full.min(ww).max(0.0)
    }

    /// History search width: the playlist search's rule (placeholder
    /// sized at rest, eased up to the header pill's width while
    /// focused) within the page's content width.
    pub(crate) fn history_search_width(&self) -> f32 {
        let full = self
            .search_full_width()
            .min(self.content_width())
            .max(160.0);
        let collapsed = (text_line_width(self.language.history_filter_placeholder())
            + PL_SEARCH_CHROME)
            .min(full);
        collapsed + (full - collapsed) * ease_in_out(self.pl_search_expand)
    }

    /// Playlist search width: placeholder-sized at rest, eased up to
    /// the header pill's expanded width while focused (same max).
    /// Narrow windows (the smallest layout) give the playlist "…" menu
    /// its own line under play / Shuffle / search; wider ones keep the
    /// whole action row on one line.
    pub(crate) fn pl_menu_own_line(&self) -> bool {
        self.layout() == LayoutMode::Narrow
    }

    pub(crate) fn pl_search_width(&self) -> f32 {
        // Grows only into the room its action row has left (play,
        // Shuffle, menu and gaps beside it), so focusing it never
        // wraps the menu onto a second line. Narrow windows put the
        // menu on its own line instead (see the header), so the search
        // also gets the menu's slot there.
        let info_w = if self.hero_stacked() {
            self.content_width()
        } else {
            self.content_width() - PLAYLIST_COVER_PX - 20.0
        };
        let shuffle_w = icon_pill_width(&self.language.shuffle_label());
        // 4px spare for subpixel rounding: a row that fits exactly can
        // still wrap by a fraction of a pixel.
        let menu_w = if self.pl_menu_own_line() {
            0.0
        } else {
            PILL_H + 8.0
        };
        let room = info_w - PILL_H - shuffle_w - 2.0 * 8.0 - menu_w - 4.0;
        let full = self.search_full_width().min(room.max(0.0));
        let collapsed = (text_line_width(self.language.playlist_filter_placeholder())
            + PL_SEARCH_CHROME)
            .min(full);
        collapsed + (full - collapsed) * ease_in_out(self.pl_search_expand)
    }

    pub(crate) fn search_pill_geom(&self) -> (f32, f32) {
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let full = self.search_full_width();
        // Collapsed at rest (just wide enough for the whole
        // placeholder), grows to full while focused (eased).
        let collapsed =
            (text_line_width(self.language.search_placeholder()) + SEARCH_PILL_CHROME).min(full);
        let w = collapsed + (full - collapsed) * ease_in_out(self.search_expand);
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
            button(btn_icon("chevron-left", 22.0, tk().text))
                .style(nav_arrow)
                .on_press(Message::NavBack)
                .padding(10)
                .into()
        } else {
            button(btn_icon("chevron-left", 22.0, tk().text_muted))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let fwd_btn: iced::Element<'_, Message> = if can_forward {
            button(btn_icon("chevron-right", 22.0, tk().text))
                .style(nav_arrow)
                .on_press(Message::NavForward)
                .padding(10)
                .into()
        } else {
            button(btn_icon("chevron-right", 22.0, tk().text_muted))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let clear_btn: iced::Element<'_, Message> = if self.search_query.is_empty() {
            text("").size(1).into()
        } else {
            button(btn_icon("x", 13.0, tk().text_muted))
                .style(icon_hover)
                .on_press(Message::HomeSearchChanged(String::new()))
                .padding(6)
                .into()
        };
        // Submit magnifier: same message as Enter in the input, so
        // clicking it and pressing Enter always do the same thing.
        let go_btn: iced::Element<'_, Message> = button(btn_icon("search", 14.0, tk().text))
            .style(icon_hover)
            .on_press(Message::SuggestPick)
            .padding(6)
            .into();
        // Pill width comes from the shared geometry (same number the
        // popup and scrim use, so edges always match).
        let (_, pill_w) = self.search_pill_geom();
        let pill_w = iced::Length::Fixed(pill_w);
        let pill: iced::Element<'_, Message> = container(
            row![
                icon("search", 14.0, tk().text_muted),
                self.edit_menu(
                    text_input(self.language.search_placeholder(), &self.search_query)
                        .style(search_inner_input)
                        .id(SEARCH_INPUT_ID)
                        .on_input(Message::HomeSearchChanged)
                        .on_submit(Message::SuggestPick)
                        .width(Fill)
                        .padding(10),
                    &self.search_query,
                    Some(Message::HomeSearchChanged),
                    false,
                ),
                clear_btn,
                go_btn,
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(search_pill(self.search_focused))
        .padding(iced::Padding {
            left: 8.0,
            right: 4.0,
            ..iced::Padding::ZERO
        })
        .width(pill_w)
        .into();
        let gear: iced::Element<'_, Message> = button(btn_icon("settings", 14.0, tk().text))
            .style(icon_hover)
            .on_press(Message::Navigate(Route::Settings))
            .padding(6)
            .into();
        // Fixed pill always (shared geometry with the popup): Fill
        // space docks the gear to the header right edge. Narrow swaps
        // the gear for the queue drawer button (Settings moves into the
        // library drawer) and leads with the library menu button. The
        // items here must match `header_lead_w` / `header_trail_w`.
        let narrow = self.layout() == LayoutMode::Narrow;
        let mut h = iced::widget::Row::new();
        if narrow {
            h = h.push(
                button(btn_icon("menu", 18.0, tk().text))
                    .style(icon_hover)
                    .on_press(Message::ToggleLibraryDrawer)
                    .padding(12),
            );
        }
        if self.header_nav_arrows() {
            h = h.push(back_btn).push(fwd_btn);
        }
        h = h.push(pill);
        let spacer: iced::Element<'_, Message> = iced::widget::Space::new().width(Fill).into();
        h = h.push(spacer);
        if narrow {
            h = h.push(
                button(btn_icon(
                    "list-music",
                    18.0,
                    if self.right_latched {
                        tk().accent
                    } else {
                        tk().text
                    },
                ))
                .style(icon_hover)
                .on_press(Message::ToggleQueueDrawer)
                .padding(12),
            );
        } else {
            h = h.push(gear);
        }
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
    pub(crate) fn home_card<'a>(
        &'a self,
        art: iced::Element<'a, Message>,
        title: String,
        sub: String,
        press: Message,
        right: Message,
    ) -> iced::Element<'a, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, mouse_area};
        use iced::Fill;
        mouse_area(
            row_button(
                column![
                    art,
                    column![
                        btn_text(truncate_title(&title, 20), 13)
                            .width(Fill)
                            .wrapping(Wrapping::None),
                        btn_text(truncate_title(&sub, 22), 11)
                            .color(tk().text_muted)
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
    /// click, extra actions live in the right-click menu. The hover
    /// key is scoped per shelf (`scope`), so the same track in Quick
    /// picks and More-from highlights only under the cursor.
    pub(crate) fn home_song_row(&self, scope: &str, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hover_id = track_hover_key(scope, &t.id);
        let hovered = self.hovered_track.as_deref() == Some(hover_id.as_str());
        let menu_id = t.id.clone();
        mouse_area(
            container(
                row![
                    self.art_play_button(t, hovered, Message::PlayNow(t.id.clone())),
                    row_button(Self::btn_track_text_single(
                        &t.title,
                        &t.artist,
                        14,
                        tk().text
                    ))
                    .style(row_click)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(3)
                    .width(Fill),
                    text(fmt_len(self.track_secs(t)))
                        .size(11)
                        .color(tk().text_muted),
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

    /// Album/artist page header: art beside the title block, or
    /// stacked (smaller art on top) when the content is too narrow for
    /// both side by side.
    pub(crate) fn hero_header<'a>(
        &'a self,
        art_url: Option<&str>,
        size: f32,
        info: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, row};
        if self.hero_stacked() {
            column![self.art_thumb(art_url, HERO_STACKED_ART), info]
                .spacing(12)
                .into()
        } else {
            row![self.art_thumb(art_url, size), info]
                .spacing(16)
                .align_y(iced::alignment::Vertical::Center)
                .into()
        }
    }

    pub(crate) fn center_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text, toggler};
        use iced::Fill;
        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let picks: Vec<Track> = self.home_pick_tracks();
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    column![
                        text(self.language.home_empty_hint()).color(tk().text_muted),
                        text(self.language.home_empty_hint2())
                            .size(12)
                            .color(tk().text_muted),
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
                                                .color(tk().text_muted)
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
                    // Two interleaved columns, or one on narrow content.
                    let cols = self.quick_picks_cols();
                    let left: Vec<iced::Element<'_, Message>> = quick
                        .iter()
                        .step_by(cols)
                        .map(|t| self.home_song_row("quick", t))
                        .collect();
                    let right: Vec<iced::Element<'_, Message>> = if cols == 2 {
                        quick
                            .iter()
                            .skip(1)
                            .step_by(2)
                            .map(|t| self.home_song_row("quick", t))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    column![
                        row![
                            text(self.language.quick_picks()).size(19).width(Fill),
                            Self::play_pill(Message::PlayQuickPicks),
                        ]
                        .align_y(iced::alignment::Vertical::Center),
                        if right.is_empty() {
                            row![column(left).spacing(2).width(Fill)]
                        } else {
                            row![
                                column(left).spacing(2).width(Fill),
                                column(right).spacing(2).width(Fill),
                            ]
                        }
                        .spacing(8),
                    ]
                    .spacing(10)
                    .into()
                };
                let lib_cards: Vec<iced::Element<'_, Message>> = self
                    .home_strip_playlists()
                    .into_iter()
                    .map(|p| {
                        let art = if p.id == Self::FAVORITES_ID {
                            self.favorites_art(HOME_PICK_ART)
                        } else {
                            let t = p
                                .tracks
                                .first()
                                .and_then(|t| self.find_track(&t.id))
                                .unwrap_or_else(|| p.tracks[0].clone());
                            self.thumb_hero(&t, HOME_PICK_ART)
                        };
                        let sub = self.language.playlist_sub(p.tracks.len());
                        self.home_card(
                            art,
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
                // Memoized shelf (refresh_home_shelves): re-ranking the
                // catalog here ran on every frame.
                let album_cards: Vec<iced::Element<'_, Message>> = self
                    .home_shelves
                    .albums
                    .iter()
                    .map(|g| {
                        self.home_card(
                            self.thumb_hero(&g.tracks[0], HOME_PICK_ART),
                            g.title.clone(),
                            g.artist.clone(),
                            Message::OpenAlbum(Self::album_route(&g.title, &g.artist)),
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
                                let rows: Vec<iced::Element<'_, Message>> = tracks
                                    .iter()
                                    .map(|t| self.home_song_row("more", t))
                                    .collect();
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
                                container(btn_icon("chevron-right", 12.0, tk().text,)).padding(
                                    iced::Padding {
                                        bottom: 2.0,
                                        ..Default::default()
                                    }
                                ),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(ghost_button)
                        .on_press(Message::Navigate(Route::Discovery))
                        .padding([12, 16])
                        .height(iced::Length::Fixed(PILL_H)),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    row![
                        row_button(
                            column![
                                btn_text(self.language.weekly_mix(), 15),
                                btn_text(self.language.track_count(weekly_n), 12)
                                    .color(tk().text_muted),
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
                                btn_text(self.language.track_count(forgotten_n), 12)
                                    .color(tk().text_muted),
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
                    column![text(self.language.search_hint_empty()).color(tk().text_muted),]
                        .spacing(10)
                        .into()
                } else if self.searching
                    && results_empty(&self.results)
                    && all_empty(&self.all_results)
                {
                    self.search_loading()
                } else if results_empty(&self.results) && all_empty(&self.all_results) {
                    column![text(self.language.no_results()).color(tk().text_muted),]
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
                                .map(|(i, t)| {
                                    self.numbered_track_row("search", t, i + 1, &RowPlay::Now)
                                })
                                .collect();
                            let list: iced::Element<'_, Message> = if songs.is_empty() {
                                text(self.language.no_songs_for(&query))
                                    .color(tk().text_muted)
                                    .into()
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
                                .map(|a| self.album_card(a))
                                .collect();
                            let list: iced::Element<'_, Message> = if cards.is_empty() {
                                text(self.language.no_albums_for(&query))
                                    .color(tk().text_muted)
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
                                    .color(tk().text_muted)
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
                                    .color(tk().text_muted)
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
                    .map(|t| self.track_row("weekly", t))
                    .collect();
                let forgotten: Vec<iced::Element<'_, Message>> = self
                    .home_shelves
                    .forgotten
                    .iter()
                    .map(|t| self.track_row("forgotten", t))
                    .collect();
                let weekly_section: iced::Element<'_, Message> = if weekly.is_empty() {
                    text(self.language.discovery_hint_play())
                        .color(tk().text_muted)
                        .into()
                } else {
                    column(weekly).spacing(2).into()
                };
                let forgotten_section: iced::Element<'_, Message> = if forgotten.is_empty() {
                    text(self.language.discovery_hint_like())
                        .color(tk().text_muted)
                        .into()
                } else {
                    column(forgotten).spacing(2).into()
                };
                column![
                    text(self.language.made_for_you()).size(22),
                    row![
                        text(self.language.weekly_mix()).size(17).width(Fill),
                        Self::play_pill(Message::PlayWeeklyMix),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    weekly_section,
                    row![
                        text(self.language.forgotten()).size(17).width(Fill),
                        Self::play_pill(Message::PlayForgotten),
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
                let body_w = self.content_width();
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
                        text(note).size(12).color(tk().text_muted).into()
                    }
                };
                // Graphic EQ panel: bypass toggle top-right, canvas
                // sliders + curve below. Presets above are predefined
                // curves for the same gains.
                let eq_panel: iced::Element<'_, Message> = container(
                    column![
                        row![
                            iced::widget::Space::new().width(Fill),
                            text(self.language.enable_custom_eq())
                                .size(12)
                                .color(tk().text_muted),
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
                            .color(tk().text_muted)
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
                            .padding([12, 16])
                            .height(iced::Length::Fixed(PILL_H))
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
                                btn_icon("chevron-down", 13.0, tk().text_muted),
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
                                btn_icon("chevron-down", 13.0, tk().text_muted),
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
                    self.romaji_settings(),
                    text(self.language.appearance_section()).size(17),
                    self.theme_settings(),
                    text(self.language.display_font()).size(13),
                    settings_input({
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_text(self.font_family.label(), 13).width(Fill),
                                btn_icon("chevron-down", 13.0, tk().text_muted),
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
                        text(self.language.scale_hint())
                            .size(12)
                            .color(tk().text_muted),
                    ]
                    .spacing(4),
                    text(self.language.animations_section()).size(17),
                    toggler(self.animations_enabled)
                        .label(self.language.enable_animations())
                        .on_toggle(Message::AnimationsToggled)
                        .style(red_toggle),
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
                        .padding([12, 16])
                        .height(iced::Length::Fixed(PILL_H)),
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
                        self.update_section(),
                        text(self.language.about_license())
                            .size(12)
                            .color(tk().text_muted),
                        button(btn_text(self.language.about_source(), 12))
                            .style(ghost_button)
                            .on_press(Message::OpenUrl(GITHUB_URL.to_string()))
                            .padding([12, 16])
                            .height(iced::Length::Fixed(PILL_H)),
                        text(self.language.about_notices())
                            .size(12)
                            .color(tk().text_muted),
                        text(self.language.about_backend())
                            .size(12)
                            .color(tk().text_muted),
                    ]
                    .spacing(4),
                    text(self.language.shortcuts_hint())
                        .size(12)
                        .color(tk().text_muted),
                ]
                .spacing(10)
                .into()
            }
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let tracks = self.artist_tracks(&name);
                let albums = self.artist_albums(&name);
                let total: u32 = tracks.iter().map(|t| self.track_secs(t)).sum();
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
                        vec![text(self.language.loading_top_songs())
                            .color(tk().text_muted)
                            .into()],
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
                            .padding([12, 16])
                            .height(iced::Length::Fixed(PILL_H))
                            .into()
                    } else if fetching_full {
                        row![
                            button(btn_text(self.language.show_more(), 12))
                                .style(ghost_button)
                                .on_press(Message::ArtistTopExpand)
                                .padding([12, 16])
                                .height(iced::Length::Fixed(PILL_H)),
                            Self::spinner_frame(self.spin_frame, 12.0, tk().text_muted),
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
                            .map(|(i, t)| self.numbered_track_row("artist", t, i + 1, &play))
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
                    Some(seed) => Self::mix_pill(
                        self.language.mix_pill_label(),
                        Message::StartRadio(seed.id.clone()),
                    ),
                };
                let photo = self.artist_photo(&name);
                column![
                    self.hero_header(
                        photo.as_deref(),
                        160.0,
                        column![
                            text(self.language.artist_label())
                                .size(12)
                                .color(tk().text_muted),
                            text(name.clone()).size(34).font(bold_font()),
                            text(self.language.artist_songs_albums(
                                tracks.len(),
                                albums.len(),
                                &fmt_time(total as f64),
                            ))
                            .size(12)
                            .color(tk().text_muted),
                            row![
                                Self::play_pill(Message::PlayAll),
                                Self::shuffle_pill(
                                    self.language.shuffle_label(),
                                    Message::PlayAllShuffled
                                ),
                                radio_btn,
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .wrap()
                            .vertical_spacing(8),
                        ]
                        .spacing(6)
                        .width(Fill)
                        .into(),
                    ),
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
                let total: u32 = tracks.iter().map(|t| self.track_secs(t)).sum();
                let artist = tracks.first().map(|t| t.artist.clone()).unwrap_or_default();
                let year = self
                    .album_search_hit(id)
                    .and_then(|a| a.year)
                    .map(|y| y.to_string())
                    .unwrap_or_default();
                // One policy (see `album_header_cover`): the header and
                // its warming share the key, so it never placeholders.
                let cover = self.album_header_cover(id);
                let play = RowPlay::AlbumAt(id.to_string());
                let rows: Vec<iced::Element<'_, Message>> = if pending {
                    vec![text(self.language.loading_album_songs())
                        .color(tk().text_muted)
                        .into()]
                } else {
                    tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| self.album_track_row(t, i + 1, &play))
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
                    self.hero_header(
                        cover.as_deref(),
                        180.0,
                        column![
                            text(self.language.album_label())
                                .size(12)
                                .color(tk().text_muted),
                            text(title.clone()).size(34).font(bold_font()),
                            text(meta).size(12).color(tk().text_muted),
                            row![
                                Self::play_pill(Message::PlayAll),
                                Self::shuffle_pill(
                                    self.language.shuffle_label(),
                                    Message::PlayAllShuffled
                                ),
                                iced::widget::button(ctrl_box(btn_icon(
                                    "ellipsis",
                                    18.0,
                                    tk().text
                                )))
                                .style(icon_hover)
                                .on_press(Message::OpenAlbumMenu(id.clone()))
                                .padding(11),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .wrap()
                            .vertical_spacing(8),
                        ]
                        .spacing(6)
                        .width(Fill)
                        .into(),
                    ),
                    column(rows).spacing(2),
                ]
                .spacing(12)
                .into()
            }
            Route::RemotePlaylist(id) => {
                let hit = self.remote_playlist_hit(id);
                let title = hit
                    .as_ref()
                    .map(|p| p.title.clone())
                    .unwrap_or_else(|| id.clone());
                let tracks = self.remote_playlist_tracks(id);
                let pending = tracks.is_empty()
                    && self.remote_playlist_loading.as_deref() == Some(id.as_str());
                let total: u32 = tracks.iter().map(|t| self.track_secs(t)).sum();
                let sub = hit
                    .as_ref()
                    .map(|p| self.language.playlist_sub_full(&p.author, &p.detail))
                    .unwrap_or_default();
                let meta = if tracks.is_empty() {
                    sub
                } else {
                    let counts = self
                        .language
                        .playlist_meta(tracks.len(), &fmt_time(total as f64));
                    match hit.as_ref().map(|p| p.author.trim()) {
                        Some(author) if !author.is_empty() => format!("{author} · {counts}"),
                        _ => counts,
                    }
                };
                let cover = self.remote_playlist_cover(id);
                let play = RowPlay::RemotePlaylistAt(id.to_string());
                let rows: Vec<iced::Element<'_, Message>> = if pending {
                    vec![text(self.language.loading_playlist_songs())
                        .color(tk().text_muted)
                        .into()]
                } else {
                    tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| self.numbered_track_row("remote-pl", t, i + 1, &play))
                        .collect()
                };
                column![
                    self.hero_header(
                        cover.as_deref(),
                        180.0,
                        column![
                            text(self.language.playlist_label())
                                .size(12)
                                .color(tk().text_muted),
                            text(title).size(34).font(bold_font()),
                            text(meta).size(12).color(tk().text_muted),
                            row![
                                Self::play_pill(Message::PlaySearchPlaylist(id.clone())),
                                Self::shuffle_pill(
                                    self.language.shuffle_label(),
                                    Message::PlaySearchPlaylistShuffled(id.clone())
                                ),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .wrap()
                            .vertical_spacing(8),
                        ]
                        .spacing(6)
                        .width(Fill)
                        .into(),
                    ),
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
                    let sort = self.playlist_sort_cached(&pid);
                    let ordered = self.playlist_ordered_tracks(&p);
                    // Sort control (Favorites too, with its own picks):
                    // sort icon + current sort in the quiet button family.
                    // The open card floats from this button (dropdown
                    // overlay), never from the cursor. Fixed width
                    // (longest pick) so the `...` trigger never shifts
                    // when the pick changes.
                    let sort_button: iced::Element<'_, Message> = {
                        // Same pill as Shuffle and the search field.
                        let face: iced::Element<'_, Message> = button(
                            row![
                                btn_icon("arrow-up-down", 13.0, tk().text_muted),
                                btn_text(sort.label(), 13),
                            ]
                            .spacing(6)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill)
                            .height(Fill),
                        )
                        .style(ghost_button)
                        .on_press(Message::OpenSelectMenu(SelectMenu::Sort(pid.clone())))
                        .padding([0, 16])
                        .width(iced::Length::Fixed(SORT_BUTTON_W))
                        .height(iced::Length::Fixed(PILL_H))
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
                    // Search: display-only filter on title/artist. Rows
                    // keep their full-list index, so a click queues the
                    // whole playlist from that song on exactly like the
                    // unfiltered view. Drag reorder pauses while filtered
                    // (a partial list has no honest drop slots).
                    let q = self.playlist_filter.trim().to_lowercase();
                    let filtering = !q.is_empty();
                    let list: iced::Element<'_, Message> = if filtering {
                        let rows: Vec<iced::Element<'_, Message>> = ordered
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| {
                                t.title.to_lowercase().contains(&q)
                                    || t.artist.to_lowercase().contains(&q)
                            })
                            .map(|(i, t)| self.playlist_track_row(&pid, t, i))
                            .collect();
                        if rows.is_empty() {
                            text(self.language.no_songs_for(self.playlist_filter.trim()))
                                .size(12)
                                .color(tk().text_muted)
                                .into()
                        } else {
                            column(rows).spacing(2).into()
                        }
                    } else if sort == PlaylistSort::Manual && ordered.len() >= 2 {
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
                    // Header (YouTube Music style): cover beside a small
                    // label, bold title, count + length, then a round play
                    // button, Shuffle and the playlist search as pills
                    // (the search grows while focused) and the menu. The
                    // sort picker (own lists only) sits right-aligned in
                    // a bar above the songs, so the action row fits.
                    let total: u64 = p.tracks.iter().map(|t| u64::from(self.track_secs(t))).sum();
                    // Every control is PILL_H tall: a true circle for play
                    // and the menu, matching pills for the rest.
                    let mut actions: Vec<iced::Element<'_, Message>> = vec![
                        round_button(
                            btn_icon("play", 18.0, tk().on_accent),
                            accent_button,
                            Message::PlayAll,
                        ),
                        icon_pill_button(
                            "shuffle",
                            self.language.shuffle_label(),
                            Message::PlayAllShuffled,
                        ),
                        self.edit_menu(
                            pill_search_input(
                                self.language.playlist_filter_placeholder(),
                                &self.playlist_filter,
                                Message::PlaylistFilterChanged,
                                self.pl_search_width(),
                            ),
                            &self.playlist_filter,
                            Some(Message::PlaylistFilterChanged),
                            false,
                        ),
                    ];
                    let menu_btn: iced::Element<'_, Message> = round_button(
                        btn_icon("ellipsis", 16.0, tk().text),
                        ghost_button,
                        Message::OpenPlaylistMenu(p.id.clone()),
                    );
                    // Narrow: the menu gets its own line below the row
                    // (the search keeps its width); wider windows keep
                    // it at the end of the one-line action row.
                    let menu_line = if self.pl_menu_own_line() {
                        Some(menu_btn)
                    } else {
                        actions.push(menu_btn);
                        None
                    };
                    let cover = self.playlist_cover_url(&p.id);
                    // Favorites shows its generated heart cover.
                    let cover_art = |size: f32| -> iced::Element<'_, Message> {
                        if p.id == Self::FAVORITES_ID {
                            self.favorites_art(size)
                        } else {
                            self.art_thumb(cover.as_deref(), size)
                        }
                    };
                    let info: iced::Element<'_, Message> = column![
                        text(self.language.playlist_label())
                            .size(12)
                            .color(tk().text_muted),
                        text(p.title.clone()).size(34).font(bold_font()),
                        text(
                            self.language
                                .playlist_meta(p.tracks.len(), &fmt_total(total))
                        )
                        .size(12)
                        .color(tk().text_muted),
                        iced::widget::Space::new().height(iced::Length::Fixed(6.0)),
                        // Wraps onto a second line on narrow windows
                        // instead of pushing the menu button off-screen.
                        iced::widget::Row::with_children(actions)
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .wrap()
                            .vertical_spacing(8),
                    ]
                    .push(menu_line.map(|m| {
                        // Same 8px as the row gaps, under the play button.
                        column![
                            iced::widget::Space::new().height(iced::Length::Fixed(4.0)),
                            m
                        ]
                    }))
                    .spacing(4)
                    .width(Fill)
                    .into();
                    let header: iced::Element<'_, Message> = if self.hero_stacked() {
                        column![cover_art(HERO_STACKED_ART), info]
                            .spacing(12)
                            .into()
                    } else {
                        // Title block sits on the cover's bottom edge.
                        row![cover_art(PLAYLIST_COVER_PX), info]
                            .spacing(20)
                            .align_y(iced::alignment::Vertical::Bottom)
                            .into()
                    };
                    let list_bar: iced::Element<'_, Message> =
                        row![iced::widget::Space::new().width(Fill), sort_button]
                            .align_y(iced::alignment::Vertical::Center)
                            .into();
                    column![header, list_bar, list,].spacing(10).into()
                }
            },
            Route::Lyrics(id) => {
                let title = self
                    .find_track(id)
                    .map(|t| format!("{} · {}", t.title, t.artist))
                    .unwrap_or_else(|| id.clone());
                let stepper = self.lyrics_provider_stepper(id);
                let content = self.lyrics_content(id, self.content_width());
                column![
                    text(title).size(22),
                    row![
                        Self::play_pill(Message::PlayNow(id.clone())),
                        Self::mix_pill(
                            self.language.mix_pill_label(),
                            Message::StartRadio(id.clone())
                        ),
                    ]
                    .spacing(8),
                    stepper,
                    content,
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
        let cap = self.content_cap();
        let scrolled: iced::Element<'_, Message> = if self.center_width() > cap {
            scrollable(
                container(container(body).padding(20).width(Fill).max_width(cap))
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
    /// Menu row that opens the playlist flyout to the side (track and
    /// album menus): hover opens it (no click needed), the Tick grace
    /// closes it once both the toggle and the panel are cold. `key`
    /// names the owning menu (track id or album flyout key).
    pub(crate) fn playlist_flyout_toggle(
        &self,
        icon_name: &'static str,
        key: String,
        label: String,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row};
        use iced::Fill;
        let open = self.submenu_playlists_for.as_deref() == Some(key.as_str());
        mouse_area(
            row_button(
                row![
                    btn_icon(icon_name, 14.0, tk().text),
                    btn_text(label, 14).width(Fill),
                    container(btn_icon("chevron-right", 14.0, tk().text_muted)).padding(
                        iced::Padding {
                            bottom: 2.0,
                            ..Default::default()
                        }
                    ),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .width(Fill)
                .height(Fill),
            )
            .style(if open { selected_row } else { hover_row })
            .on_press(Message::TogglePlaylistSubmenu(key.clone()))
            .padding(8)
            .width(Fill)
            .height(iced::Length::Fixed(MENU_ROW_H)),
        )
        .on_enter(Message::SubmenuTriggerHot(Some(key)))
        .on_exit(Message::SubmenuTriggerHot(None))
        .into()
    }

    /// One playlist row in the flyout: cover, title, and a hint line
    /// (accent when `marked`, i.e. the song or album is already in it).
    pub(crate) fn flyout_row(
        &self,
        pl_id: &str,
        pl_title: &str,
        sub: String,
        marked: bool,
        msg: Message,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{column, row, text};
        use iced::Fill;
        let mut row_items: Vec<iced::Element<'_, Message>> = Vec::new();
        if let Some(pl) = self.find_playlist(pl_id) {
            row_items.push(self.playlist_cover(&pl, 28.0));
        }
        row_items.push(
            column![
                text(truncate_title(pl_title, 20)).size(13),
                text(sub)
                    .size(11)
                    .color(if marked { tk().accent } else { tk().text_muted }),
            ]
            .spacing(1)
            .width(Fill)
            .into(),
        );
        row_button(
            row(row_items)
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
        )
        .style(hover_row)
        .on_press(msg)
        .padding(6)
        .width(Fill)
        .into()
    }

    /// Playlist flyout panel placed beside a menu anchored at
    /// (`mx`, `my`): docks right of the 300px menu, flipping left when
    /// there is no room, top-aligned with the toggle row at
    /// `toggle_idx` (menu top padding 14 + header thumb 56 + gap 8,
    /// then MENU_ROW_H per row above it, so the anchor cannot drift).
    pub(crate) fn playlist_flyout_layer<'a>(
        &'a self,
        mut fly_rows: Vec<iced::Element<'a, Message>>,
        mx: f32,
        my: f32,
        toggle_idx: usize,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, container, mouse_area, row, text};
        use iced::Fill;
        if fly_rows.is_empty() {
            fly_rows.push(
                text(self.language.no_playlists_menu())
                    .size(12)
                    .color(tk().text_muted)
                    .into(),
            );
        }
        let fly_h = ((fly_rows.len() as f32) * 44.0).min(340.0);
        let flyout: iced::Element<'_, Message> = mouse_area(
            container(
                iced::widget::scrollable(column(fly_rows).spacing(2).width(Fill))
                    .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                    .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("flyout")))
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
        let mx2 = if mx + 300.0 + 8.0 + 280.0 <= ww {
            mx + 300.0 + 8.0
        } else {
            (mx - 280.0 - 8.0).max(0.0)
        };
        let header_h = 78.0;
        let my2 = (my + header_h + toggle_idx as f32 * MENU_ROW_H)
            .min((wh - fly_h - 16.0).max(0.0))
            .max(0.0);
        container(column![
            iced::widget::Space::new().height(iced::Length::Fixed(my2)),
            row![
                iced::widget::Space::new().width(iced::Length::Fixed(mx2)),
                flyout,
            ],
        ])
        .width(Fill)
        .height(Fill)
        .into()
    }

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
                            btn_icon("search", 13.0, tk().text_muted),
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
                    .color(tk().accent)
                    .into()
            } else {
                text(self.language.suggest_hint())
                    .size(11)
                    .color(tk().text_muted)
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
        set_row_title_chars(row_title_budget(self.content_width()));
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
        // Panel slots: push modes reserve the animated panel (the
        // collapsed rail always keeps its strip, even when empty),
        // Medium reserves the rail and floats the expanded panel above,
        // Narrow reserves nothing (drawers only).
        let mut main_children: Vec<iced::Element<'_, Message>> = Vec::with_capacity(3);
        let left_slot = self.sidebar_slot_w();
        if left_slot > 0.0 {
            main_children.push(self.sidebar_view_at(left_slot));
        }
        main_children.push(center);
        let right_slot = self.right_slot_w();
        if right_slot > 0.0 {
            main_children.push(self.right_panel_view_at(right_slot));
        }
        let row_layer = Row::with_children(main_children)
            .spacing(PANEL_GAP)
            .height(Fill);
        let main = self.with_panel_overlays(row_layer.into());
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
        base_children.push(main);
        base_children.push(self.player_view());
        let base: iced::Element<'_, Message> = container(
            iced::widget::Column::with_children(base_children)
                .spacing(8)
                .height(Fill),
        )
        // Roomier top padding so the header never hugs the window
        // edge (sides/bottom stay tight).
        .padding(iced::Padding {
            top: 16.0,
            left: 8.0,
            right: 8.0,
            bottom: 8.0,
        })
        .into();
        // Background first, then the collection page cover glow (behind
        // every panel: sidebars and the player draw over it), then the
        // app itself.
        let mut layers: Vec<iced::Element<'_, Message>> =
            vec![container(iced::widget::Space::new())
                .style(app_bg)
                .width(Fill)
                .height(Fill)
                .into()];
        // Always one slot (empty when there is no glow): a changing layer
        // count would shift `base` and reset every scroll position in it.
        layers.push(
            self.page_backdrop_layer()
                .unwrap_or_else(|| iced::widget::Space::new().into()),
        );
        layers.push(base);
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
                    .interaction(iced::mouse::Interaction::Idle)
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
                    (tk().accent, self.language.remove_from_favorites())
                } else {
                    (tk().text_muted, self.language.add_to_favorites())
                };
                let mut items: Vec<iced::Element<'_, Message>> = vec![
                    row_button(
                        row![
                            btn_icon("play", 14.0, tk().text),
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
                            btn_icon("list-plus", 14.0, tk().text),
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
                            btn_icon("plus", 14.0, tk().text),
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
                            btn_icon("radio", 14.0, tk().text),
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
                            btn_icon("mic", 14.0, tk().text),
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
                            (tk().accent, self.language.recommend_again())
                        } else {
                            (tk().text_muted, self.language.dont_recommend())
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
                            btn_icon("disc", 14.0, tk().text),
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
                            btn_icon("user-round", 14.0, tk().text),
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
                                btn_icon("trash", 14.0, tk().text),
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
                                btn_icon("trash", 14.0, tk().text),
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
                // lead via playlist_heads.
                let membership: Vec<(String, String, bool)> = self
                    .playlist_heads()
                    .iter()
                    .map(|pl| {
                        let has = self.playlist_has(pl.id, &t.id);
                        (pl.id.to_string(), pl.title.to_string(), has)
                    })
                    .collect();
                // Flyout toggle: the playlist list opens to the side so
                // transport rows stay put and multi-add never closes.
                // Hover opens it (no click needed); the Tick grace
                // closes it once both the toggle and the panel are
                // cold. Its index below anchors the flyout height.
                let toggle_idx = items.len();
                items.push(self.playlist_flyout_toggle(
                    "queue",
                    t.id.clone(),
                    self.language.playlists_toggle(membership.len()),
                ));
                // Playlist adds leave the menu open (multi-add); everything else
                // closes it via its own handler.
                let menu = container(
                    column![
                        row![
                            self.thumb_hero(&t, 56.0),
                            column![
                                text(t.title.clone()).size(16),
                                text(t.artist.clone()).size(12).color(tk().text_muted),
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
                // another song moves the menu in one step. The arrow
                // vote still shadows background buttons (cursor only,
                // clicks and right-clicks behave as before).
                let scrim: iced::Element<'_, Message> =
                    mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                        .on_press(Message::CloseContext)
                        .on_enter(Message::ClearHovers)
                        .on_exit(Message::ClearHovers)
                        .interaction(iced::mouse::Interaction::Idle)
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
                    let fly_rows: Vec<iced::Element<'_, Message>> = membership
                        .iter()
                        .map(|(pl_id, pl_title, in_pl)| {
                            let (sub, msg) = if *in_pl {
                                (
                                    self.language.flyout_tap_remove(),
                                    Message::RemoveFromPlaylist {
                                        playlist_id: pl_id.clone(),
                                        track_id: t.id.clone(),
                                    },
                                )
                            } else {
                                (
                                    self.language.flyout_tap_add(),
                                    Message::AddToPlaylist {
                                        playlist_id: pl_id.clone(),
                                        track_id: t.id.clone(),
                                    },
                                )
                            };
                            self.flyout_row(pl_id, pl_title, sub, *in_pl, msg)
                        })
                        .collect();
                    layers.push(self.playlist_flyout_layer(fly_rows, mx, my, toggle_idx));
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
                            btn_icon("play", 14.0, tk().text),
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
                            btn_icon("plus", 14.0, tk().text),
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
                            btn_icon("shuffle", 14.0, tk().text),
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
                // Export works for every list, Favorites included (the
                // save dialog ships with the online build only).
                #[cfg(feature = "innertube")]
                pl_items.push(
                    row_button(
                        row![
                            btn_icon("file-down", 14.0, tk().text),
                            btn_text(self.language.export_playlist_csv(), 14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::ExportPlaylist(pid3.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                );
                if pid3 != Self::FAVORITES_ID {
                    let pid_rename = pid3.clone();
                    pl_items.push(
                        row_button(
                            row![
                                btn_icon("pencil", 14.0, tk().text),
                                btn_text(self.language.rename_playlist(), 14),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RequestRenamePlaylist(pid_rename))
                        .padding(8)
                        .width(Fill)
                        .into(),
                    );
                    let pid4 = pid3.clone();
                    pl_items.push(
                        row_button(
                            row![
                                btn_icon("trash", 14.0, tk().text),
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
                let pl_art: iced::Element<'_, Message> = if p.id == Self::FAVORITES_ID {
                    self.favorites_art(56.0)
                } else {
                    match p.tracks.first().and_then(|t| self.find_track(&t.id)) {
                        Some(t) => self.thumb_hero(&t, 56.0),
                        None => self.playlist_cover(&p, 56.0),
                    }
                };
                let pl_menu = container(
                    column![
                        row![
                            pl_art,
                            column![
                                text(p.title.clone()).size(16),
                                text(self.language.playlist_sub(p.tracks.len()))
                                    .size(12)
                                    .color(tk().text_muted),
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
                        .interaction(iced::mouse::Interaction::Idle)
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
                    .interaction(iced::mouse::Interaction::Idle)
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
                    let (title, artist, year) = self.album_menu_info(id);
                    let sub = match year {
                        Some(y) => self.language.album_sub_year(&artist, y),
                        None => self.language.album_sub(&artist),
                    };
                    Some((
                        self.art_thumb(self.album_menu_cover(id).as_deref(), 56.0),
                        title,
                        sub,
                    ))
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
            // Album menus act on the same list the album page shows.
            let album_menu_tracks: Vec<Track> = match &menu {
                CollectionMenu::Album(id) => self.album_display_tracks(id),
                _ => Vec::new(),
            };
            let mut album_toggle_idx: Option<usize> = None;
            if let Some((art, title, sub)) = hit {
                let mut items: Vec<iced::Element<'_, Message>> = Vec::new();
                match menu.clone() {
                    CollectionMenu::Album(id) => {
                        let (_, artist, _) = self.album_menu_info(&id);
                        let tracks = album_menu_tracks.clone();
                        if let Some(seed) = tracks.first().map(|t| t.id.clone()) {
                            items.push(menu_item(
                                "play",
                                self.language.play_label(),
                                Message::PlayAlbumAt {
                                    album_id: id.clone(),
                                    index: 0,
                                    track_id: seed.clone(),
                                },
                            ));
                            items.push(menu_item(
                                "shuffle",
                                self.language.shuffle_play(),
                                Message::PlayAlbumShuffled(id.clone()),
                            ));
                            items.push(menu_item(
                                "plus",
                                self.language.add_to_queue(),
                                Message::QueueAlbum(id.clone()),
                            ));
                            album_toggle_idx = Some(items.len());
                            items.push(self.playlist_flyout_toggle(
                                "list-plus",
                                Self::album_submenu_key(&id),
                                self.language.add_to_playlist(),
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
                            column![
                                text(title).size(16),
                                text(sub).size(12).color(tk().text_muted),
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
                let coll_scrim: iced::Element<'_, Message> =
                    mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                        .on_press(Message::CloseContext)
                        .on_enter(Message::ClearHovers)
                        .on_exit(Message::ClearHovers)
                        .interaction(iced::mouse::Interaction::Idle)
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
                // Album playlist flyout: same hover/grace contract as
                // the track menu's; a press adds the whole album.
                if let (CollectionMenu::Album(album_id), Some(toggle_idx)) =
                    (&menu, album_toggle_idx)
                {
                    let key = Self::album_submenu_key(album_id);
                    if self.submenu_playlists_for.as_deref() == Some(key.as_str()) {
                        let fly_rows: Vec<iced::Element<'_, Message>> = self
                            .playlist_heads()
                            .into_iter()
                            .map(|pl| {
                                let done = self.album_in_playlist(pl.id, &album_menu_tracks);
                                let sub = if done {
                                    self.language.flyout_all_added()
                                } else {
                                    self.language.flyout_tap_add()
                                };
                                self.flyout_row(
                                    pl.id,
                                    pl.title,
                                    sub,
                                    done,
                                    Message::AddAlbumToPlaylist {
                                        playlist_id: pl.id.to_string(),
                                        album_id: album_id.clone(),
                                    },
                                )
                            })
                            .collect();
                        layers.push(self.playlist_flyout_layer(fly_rows, mx, my, toggle_idx));
                    }
                }
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
        // Playlist-rename dialog above everything else: same dimmed
        // frame, scrim/Esc keeps the old name (safe default).
        if let Some(dialog) = self.playlist_rename_dialog() {
            layers.push(dialog);
        }
        // New-playlist dialog: same dimmed frame, scrim/Esc cancels.
        if let Some(dialog) = self.new_playlist_dialog() {
            layers.push(dialog);
        }
        // Self-update modal above everything else: same dimmed frame,
        // scrim/Esc dismisses for this session (safe default).
        if let Some(dialog) = self.update_dialog() {
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

    /// Collection page cover glow at window level: a band across the
    /// whole window top (sidebars and header draw over it) that scrolls
    /// away with the page body, fading into the app background. `None`
    /// off collection pages, before the backdrop is decoded, or once it
    /// has scrolled out of view.
    pub(crate) fn page_backdrop_layer(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{container, image, pin, Space, Stack};
        use iced::Fill;
        let handle = self.page_backdrop_handle()?;
        let scroll = self.scroll_offsets.get("body").copied().unwrap_or(0.0);
        if scroll >= PAGE_BACKDROP_H {
            return None;
        }
        let band: iced::Element<'_, Message> = Stack::with_children(vec![
            image(handle)
                .content_fit(iced::ContentFit::Cover)
                .filter_method(image::FilterMethod::Linear)
                .opacity(PAGE_BACKDROP_OPACITY)
                .width(Fill)
                .height(Fill)
                .into(),
            container(Space::new())
                .style(page_backdrop_veil({
                    let p = &self.active_theme().palette;
                    backdrop_veil_floor(
                        self.page_backdrop_tone(),
                        p.text,
                        p.window,
                        PAGE_BACKDROP_OPACITY,
                    )
                }))
                .width(Fill)
                .height(Fill)
                .into(),
        ])
        .width(Fill)
        .height(iced::Length::Fixed(PAGE_BACKDROP_H))
        .into();
        Some(pin(band).y(-scroll).width(Fill).height(Fill).into())
    }

    /// Now Playing sheet (shared by the sliding and fully open layers):
    /// the cover's colors stretched into soft blobs behind the content,
    /// darkened toward the bottom so lyrics stay readable. Falls back to
    /// the plain card surface until the backdrop is ready.
    pub(crate) fn drawer_sheet(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, image, row, text, Space, Stack};
        use iced::Fill;
        let content = container(
            column![
                row![
                    button(ctrl_box(btn_icon("chevron-down", 14.0, tk().text)))
                        .style(icon_hover)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    text(self.language.now_playing())
                        .size(13)
                        .color(tk().text_muted)
                        .width(Fill),
                ]
                .align_y(iced::alignment::Vertical::Center),
                self.drawer_view(),
            ]
            .spacing(8),
        )
        .padding(16)
        .width(Fill)
        .height(Fill);
        let blur = self.queue.current().and_then(|t| self.track_blur(t));
        // Fixed four slots (surface, glow, veil, content): a changing
        // layer count would rebuild the content and reset the lyrics
        // scroll on every song change.
        let (glow, veil): (iced::Element<'_, Message>, iced::Element<'_, Message>) = match blur {
            Some(handle) => (
                image(handle)
                    .content_fit(iced::ContentFit::Cover)
                    .filter_method(image::FilterMethod::Linear)
                    .opacity(NP_BACKDROP_OPACITY)
                    .border_radius(8.0)
                    .width(Fill)
                    .height(Fill)
                    .into(),
                container(Space::new())
                    .style(np_backdrop_veil({
                        let p = &self.active_theme().palette;
                        let tones = self.queue.current().and_then(|t| self.track_blur_tone(t));
                        backdrop_veil_floor(tones, p.text, p.surface, NP_BACKDROP_OPACITY)
                    }))
                    .width(Fill)
                    .height(Fill)
                    .into(),
            ),
            None => (Space::new().into(), Space::new().into()),
        };
        let layers: Vec<iced::Element<'_, Message>> = vec![
            container(Space::new())
                .style(card_bg)
                .width(Fill)
                .height(Fill)
                .into(),
            glow,
            veil,
            content.into(),
        ];
        Stack::with_children(layers).width(Fill).height(Fill).into()
    }

    /// Now Playing sheet that overlays the center column (drawer slide).
    /// Gated on `drawer_anim` by the caller.
    pub(crate) fn drawer_layer(&self) -> iced::Element<'_, Message> {
        use iced::widget::{column, container};
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
        let sheet = self.drawer_sheet();
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
        use iced::widget::{column, container};
        use iced::Fill;
        let win_h = self.window.map(|s| s.height).unwrap_or(800.0);
        let center_h = (win_h - 148.0).max(300.0);
        let sheet_h = (center_h - DRAWER_TOP_RESERVE).max(0.0);
        let offset = (1.0 - self.drawer_anim).max(0.0) * sheet_h;
        let sheet = self.drawer_sheet();
        container(column![
            iced::widget::Space::new().height(iced::Length::Fixed(offset)),
            sheet,
        ])
        .width(Fill)
        .height(Fill)
        .into()
    }
}
