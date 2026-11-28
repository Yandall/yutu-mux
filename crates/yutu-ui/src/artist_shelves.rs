//! yutu-ui artist_shelves: the artist page's server shelves (Albums,
//! Singles & EPs, Videos, Featured on, Fans might also like, ...) as
//! Home-style card strips, and the full grid page behind a shelf's
//! "More" (the discography).

#[allow(unused_imports)]
use super::*;
use yutu_core::{ShelfItem, ShelfItemKind};

/// Scroll key of the horizontal strips (fade only: the body scroll
/// keeps its own key).
const STRIP_KEY: &str = "artist-strip";

impl AppState {
    /// One shelf card. Same press rules as every card: the whole card
    /// opens (album, artist, playlist) or plays (video, fresh queue);
    /// right click opens that kind's menu.
    pub(crate) fn shelf_card<'a>(
        &'a self,
        item: &ShelfItem,
        owner: &str,
    ) -> iced::Element<'a, Message> {
        let art = item
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        let (press, right) = match item.kind {
            ShelfItemKind::Album => (
                Message::OpenAlbum(item.id.clone()),
                Message::OpenAlbumMenu(item.id.clone()),
            ),
            ShelfItemKind::Artist => (
                Message::OpenArtist(item.id.clone()),
                Message::OpenArtistMenu(item.id.clone()),
            ),
            ShelfItemKind::Playlist => (
                Message::OpenSearchPlaylist(item.id.clone()),
                Message::OpenSearchPlaylistMenu(item.id.clone()),
            ),
            ShelfItemKind::Video => {
                let track = Self::shelf_video_track(item, owner);
                (
                    Message::ShelfVideo {
                        track: track.clone(),
                        menu: false,
                    },
                    Message::ShelfVideo { track, menu: true },
                )
            }
        };
        self.home_card(
            self.art_thumb(art.as_deref(), HOME_PICK_ART),
            item.title.clone(),
            item.subtitle.clone(),
            press,
            right,
        )
    }

    /// Shelf title row: the title, plus the "More" pill when the shelf
    /// has a full page the app can open.
    fn shelf_header<'a>(&self, title: &str, more: Option<Message>) -> iced::Element<'a, Message> {
        use iced::widget::{button, row, text};
        use iced::Fill;
        let title = text(title.to_string()).size(19).width(Fill);
        match more {
            Some(msg) => row![
                title,
                button(btn_text(self.language.shelf_more(), 12))
                    .style(ghost_button)
                    .on_press(msg)
                    .padding([6, 14]),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
            None => title.into(),
        }
    }

    /// Every server shelf of the artist page, in server order, each a
    /// horizontal card strip (like Home). `None` until the page lands,
    /// so the caller keeps its local fallback.
    pub(crate) fn artist_shelves_view(&self, route_id: &str) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{column, row, scrollable};
        use iced::Fill;
        let channel = self.artist_channel_id(route_id)?;
        let page = self.artist_pages.get(&channel)?;
        let sections: Vec<iced::Element<'_, Message>> = page
            .shelves
            .iter()
            .enumerate()
            .map(|(i, shelf)| {
                let cards: Vec<iced::Element<'_, Message>> = shelf
                    .items
                    .iter()
                    .map(|item| self.shelf_card(item, &page.name))
                    .collect();
                let more = shelf.more.as_ref().map(|_| Message::OpenArtistShelfMore {
                    channel: channel.clone(),
                    shelf: i,
                });
                column![
                    self.shelf_header(&shelf.title, more),
                    scrollable(row(cards).spacing(10))
                        .direction(scrollable::Direction::Horizontal(slim_bar()))
                        .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity(STRIP_KEY)))
                        .on_scroll(|v| Message::Scrolled(STRIP_KEY, v.absolute_offset().x))
                        .width(Fill)
                        .height(iced::Length::Fixed(HOME_CARD_H + HOME_STRIP_GAP)),
                ]
                .spacing(10)
                .into()
            })
            .collect();
        Some(column(sections).spacing(18).into())
    }

    /// Full page behind a shelf's "More": the artist, the shelf title,
    /// then every card in a wrapping grid (the shelf's own cards stand
    /// in while the full grid loads).
    pub(crate) fn artist_shelf_view(
        &self,
        channel: &str,
        shelf: usize,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{column, row, text};
        let Some(page) = self.artist_pages.get(channel) else {
            return text(self.language.section_empty())
                .size(12)
                .color(MUTED)
                .into();
        };
        let title = page
            .shelves
            .get(shelf)
            .map(|s| s.title.clone())
            .unwrap_or_default();
        let cards: Vec<iced::Element<'_, Message>> = self
            .artist_shelf_cards(channel, shelf)
            .iter()
            .map(|item| self.shelf_card(item, &page.name))
            .collect();
        let loading = self.artist_grid_loading.is_some()
            && self.artist_shelf_grid_key() == self.artist_grid_loading;
        let heading: iced::Element<'_, Message> = if loading {
            row![
                text(title).size(28).font(bold_font()),
                Self::spinner_frame(self.spin_frame, 14.0, MUTED),
            ]
            .spacing(10)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        } else {
            text(title).size(28).font(bold_font()).into()
        };
        column![
            iced::widget::button(btn_text(page.name.clone(), 13).color(MUTED))
                .style(row_click)
                .on_press(Message::OpenArtist(channel.to_string()))
                .padding(0),
            heading,
            Self::card_strip(cards),
        ]
        .spacing(10)
        .into()
    }

    /// Card art slots for the shelf strips (artwork warming): every
    /// card of a strip sits on that strip's row.
    pub(crate) fn artist_shelf_art_slots(&self, route_id: &str, y0: f32) -> Vec<(String, f32)> {
        let Some(page) = self.artist_page(route_id) else {
            return Vec::new();
        };
        let mut y = y0;
        let mut slots = Vec::new();
        for shelf in &page.shelves {
            y += Self::text_h(19.0) + 10.0;
            slots.extend(shelf.items.iter().filter_map(|i| {
                i.thumbnail_url
                    .as_deref()
                    .map(|u| (yutu_core::hires_thumb_url(u, 544), y))
            }));
            y += HOME_CARD_H + HOME_STRIP_GAP + 18.0;
        }
        slots
    }
}
