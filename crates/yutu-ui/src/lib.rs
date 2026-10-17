//! yutu-ui: native Iced app: real YouTube Music data only, no mock fixtures.

use std::collections::{HashMap, HashSet};
use yutu_core::{
    forgotten_favorites, should_scrobble, weekly_mix, Album, Artist, HistoryEntry, LibraryStore,
    LyricsResult, Playlist, PlaylistHit, QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
use yutu_player::MpvBackend;
use yutu_player::{EqPreset, Queue, RodioBackend};

// ---------------------------------------------------------------------------
// YouTube-red theme (dark, red accent). Keeps every view on one visual
// language: near-black app, slightly lighter sidebar/cards, single red
// primary action, muted secondary text, text-buttons for row actions.
// ---------------------------------------------------------------------------

const SPOT_GREEN: iced::Color = iced::Color::from_rgb8(0xFF, 0x00, 0x33);
const SPOT_GREEN_HOVER: iced::Color = iced::Color::from_rgb8(0xFF, 0x33, 0x66);
const MUTED: iced::Color = iced::Color::from_rgb8(0xB3, 0xB3, 0xB3);
const CARD_BG: iced::Color = iced::Color::from_rgb8(0x18, 0x18, 0x18);
const CARD_BG_HOVER: iced::Color = iced::Color::from_rgb8(0x28, 0x28, 0x28);
const SIDEBAR_BG: iced::Color = iced::Color::from_rgb8(0x12, 0x12, 0x12);
const APP_BG: iced::Color = iced::Color::from_rgb8(0x0A, 0x0A, 0x0A);
const NAV_ACTIVE_BG: iced::Color = iced::Color::from_rgb8(0x2A, 0x2A, 0x2A);

fn app_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(APP_BG)),
        ..Default::default()
    }
}

fn sidebar_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

fn card_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(CARD_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Slim floating scrollbar shared by every scrollable (4px). Fully
/// transparent until the list is hovered, dragged, or recently
/// scrolled: Iced themes scrollbars by interaction status, and scroll
/// motion lights a per-list fade that the ticker eases out (see
/// `scroll_hint`), which is the available transition mechanism.
/// Floating scrollbars paint over content, so hiding never takes
/// layout space and revealing shifts nothing.
fn slim_bar() -> iced::widget::scrollable::Scrollbar {
    iced::widget::scrollable::Scrollbar::new()
        .width(4.0)
        .scroller_width(4.0)
        .margin(2.0)
}

/// Slim scrollbar style with a travelling scroll-fade opacity 0..1
/// from the ticking `scroll_hint` map: hover/drag always show, scroll
/// motion shows then eases back to invisible.
fn slim_scroll_alpha(
    theme: &iced::Theme,
    status: iced::widget::scrollable::Status,
    alpha: f32,
) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::Status;
    let mut base = iced::widget::scrollable::default(theme, status);
    let a = if matches!(status, Status::Active { .. }) {
        0.4 * alpha.clamp(0.0, 1.0)
    } else {
        0.4
    };
    let rail = iced::widget::scrollable::Rail {
        background: None,
        border: iced::Border::default(),
        scroller: iced::widget::scrollable::Scroller {
            background: iced::Background::Color(iced::Color::from_rgba(1.0, 1.0, 1.0, a)),
            border: iced::Border::default().rounded(2),
        },
    };
    base.vertical_rail = rail;
    base.horizontal_rail = rail;
    base
}

/// Search pill: the rounded-input look for the container that wraps the
/// icon + borderless input (the icon sits visually inside the field).
fn search_pill(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(
            0x1E, 0x1E, 0x1E,
        ))),
        border: iced::Border {
            color: iced::Color::from_rgb8(0x3A, 0x3A, 0x3A),
            width: 1.0,
            radius: 14.0.into(),
        },
        ..Default::default()
    }
}

/// Borderless input for inside the search pill (the pill owns the
/// background + border, so the field itself is transparent).
fn search_inner_input(
    _: &iced::Theme,
    _: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced::Color::TRANSPARENT),
        border: iced::Border::default(),
        icon: MUTED,
        placeholder: MUTED,
        value: iced::Color::WHITE,
        selection: SPOT_GREEN,
    }
}

fn player_bg(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SIDEBAR_BG)),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Green circular primary action (Spotify play button).
fn green_play(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => SPOT_GREEN_HOVER,
        _ => SPOT_GREEN,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(20),
        ..Default::default()
    }
}

/// Sidebar nav: active gets a pill background, inactive is transparent.
fn nav_style(
    active: bool,
) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_: &iced::Theme, status: iced::widget::button::Status| {
        let hovered = matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        );
        let background = if active || hovered {
            Some(iced::Background::Color(NAV_ACTIVE_BG))
        } else {
            None
        };
        let text_color = if active || hovered {
            iced::Color::WHITE
        } else {
            MUTED
        };
        iced::widget::button::Style {
            background,
            text_color,
            border: iced::Border::default().rounded(10),
            ..Default::default()
        }
    }
}

/// Lucide icon (ISC, pinned 0.469.0, `assets/icons/*.svg`), tinted to
/// `color`. Lucide strokes use `currentColor`, so single-color tinting
/// reproduces the design palette at any size.
fn icon(name: &'static str, size: f32, color: iced::Color) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    let bytes: &'static [u8] = match name {
        "play" => include_bytes!("../assets/icons/play.svg"),
        "pause" => include_bytes!("../assets/icons/pause.svg"),
        "skip-back" => include_bytes!("../assets/icons/skip-back.svg"),
        "skip-forward" => include_bytes!("../assets/icons/skip-forward.svg"),
        "shuffle" => include_bytes!("../assets/icons/shuffle.svg"),
        "repeat" => include_bytes!("../assets/icons/repeat.svg"),
        "repeat-1" => include_bytes!("../assets/icons/repeat-1.svg"),
        "heart" => include_bytes!("../assets/icons/heart.svg"),
        "search" => include_bytes!("../assets/icons/search.svg"),
        "house" => include_bytes!("../assets/icons/house.svg"),
        "library" => include_bytes!("../assets/icons/library.svg"),
        "plus" => include_bytes!("../assets/icons/plus.svg"),
        "x" => include_bytes!("../assets/icons/x.svg"),
        "volume" => include_bytes!("../assets/icons/volume-2.svg"),
        "mic" => include_bytes!("../assets/icons/mic.svg"),
        "user-round" => include_bytes!("../assets/icons/user-round.svg"),
        "radio" => include_bytes!("../assets/icons/radio.svg"),
        "settings" => include_bytes!("../assets/icons/settings.svg"),
        "queue" => include_bytes!("../assets/icons/list-music.svg"),
        "chevron-up" => include_bytes!("../assets/icons/chevron-up.svg"),
        "chevron-down" => include_bytes!("../assets/icons/chevron-down.svg"),
        "chevron-left" => include_bytes!("../assets/icons/chevron-left.svg"),
        "chevron-right" => include_bytes!("../assets/icons/chevron-right.svg"),
        "arrow-up" => include_bytes!("../assets/icons/arrow-up.svg"),
        "arrow-down" => include_bytes!("../assets/icons/arrow-down.svg"),
        "download" => include_bytes!("../assets/icons/download.svg"),
        "trash" => include_bytes!("../assets/icons/trash-2.svg"),
        "ellipsis" => include_bytes!("../assets/icons/ellipsis.svg"),
        "grip" => include_bytes!("../assets/icons/grip-vertical.svg"),
        "music" => include_bytes!("../assets/icons/music.svg"),
        "disc" => include_bytes!("../assets/icons/disc.svg"),
        "folder" => include_bytes!("../assets/icons/folder.svg"),
        "list-plus" => include_bytes!("../assets/icons/list-plus.svg"),
        "sparkles" => include_bytes!("../assets/icons/sparkles.svg"),
        "panel-left" => include_bytes!("../assets/icons/panel-left.svg"),
        "panel-right" => include_bytes!("../assets/icons/panel-right.svg"),
        "thumbs-down" => include_bytes!("../assets/icons/thumbs-down.svg"),
        _ => include_bytes!("../assets/icons/music.svg"),
    };
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Heart with hollow/filled states: outline when inactive, solid when
/// active. The filled glyph is the verbatim Lucide heart with its
/// `fill="none"` flipped at runtime (Lucide ships no filled heart), so
/// no custom drawing and no new asset file.
fn heart_icon(liked: bool, size: f32, color: iced::Color) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    if !liked {
        return icon("heart", size, color);
    }
    const HEART: &str = include_str!("../assets/icons/heart.svg");
    // Parsed once: every liked row shares the handle.
    static FILLED_HEART: std::sync::LazyLock<svg::Handle> = std::sync::LazyLock::new(|| {
        let filled: Vec<u8> = HEART
            .replace("fill=\"none\"", "fill=\"currentColor\"")
            .into_bytes();
        svg::Handle::from_memory(filled)
    });
    svg(FILLED_HEART.clone())
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Thumbs-down with hollow/filled states (same fill-flip as
/// [`heart_icon`]): outline when the track is not disliked, solid when
/// "don't recommend" is active.
fn thumbs_down_icon(
    disliked: bool,
    size: f32,
    color: iced::Color,
) -> iced::Element<'static, Message> {
    use iced::widget::svg;
    if !disliked {
        return icon("thumbs-down", size, color);
    }
    const THUMBS_DOWN: &str = include_str!("../assets/icons/thumbs-down.svg");
    static FILLED_THUMBS_DOWN: std::sync::LazyLock<svg::Handle> = std::sync::LazyLock::new(|| {
        let filled: Vec<u8> = THUMBS_DOWN
            .replace("fill=\"none\"", "fill=\"currentColor\"")
            .into_bytes();
        svg::Handle::from_memory(filled)
    });
    svg(FILLED_THUMBS_DOWN.clone())
        .width(size)
        .height(size)
        .style(move |_: &iced::Theme, _| svg::Style { color: Some(color) })
        .into()
}

/// Rounded text input (search bar pill, filter fields): dark fill, soft
/// border, comfortable radius. One style for every input in the app.
fn rounded_input(
    _: &iced::Theme,
    _: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced::Color::from_rgb8(0x1E, 0x1E, 0x1E)),
        border: iced::Border {
            color: iced::Color::from_rgb8(0x3A, 0x3A, 0x3A),
            width: 1.0,
            radius: 14.0.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: iced::Color::WHITE,
        selection: SPOT_GREEN,
    }
}

/// Slider in theme red: red filled rail, pale track, white round handle.
fn themed_slider(_: &iced::Theme, _: iced::widget::slider::Status) -> iced::widget::slider::Style {
    iced::widget::slider::Style {
        rail: iced::widget::slider::Rail {
            backgrounds: (
                iced::Background::Color(SPOT_GREEN),
                iced::Background::Color(iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)),
            ),
            width: 4.0,
            border: iced::Border::default().rounded(2),
        },
        handle: iced::widget::slider::Handle {
            shape: iced::widget::slider::HandleShape::Circle { radius: 8.0 },
            background: iced::Background::Color(iced::Color::WHITE),
            border_width: 0.0,
            border_color: iced::Color::TRANSPARENT,
        },
    }
}

/// Import progress bar in theme red: red fill on a dark track.
#[cfg(feature = "innertube")]
fn import_bar(_: &iced::Theme) -> iced::widget::progress_bar::Style {
    iced::widget::progress_bar::Style {
        background: iced::Background::Color(iced::Color::from_rgb8(0x3A, 0x3A, 0x3A)),
        bar: iced::Background::Color(SPOT_GREEN),
        border: iced::Border::default().rounded(4),
    }
}

/// Row action button: transparent at rest, soft card hover with rounded
/// corners (every clickable row/item gets hover feedback).
/// Browser arrows: always transparent, softly rounded; the icon (not a
/// background) carries the enabled state.
fn nav_arrow(_: &iced::Theme, _: iced::widget::button::Status) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: None,
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(12),
        ..Default::default()
    }
}

fn hover_row(_: &iced::Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let hovered = matches!(
        status,
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
    );
    iced::widget::button::Style {
        background: hovered.then_some(iced::Background::Color(CARD_BG_HOVER)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Latched-selected row (flyout toggle while its submenu is open).
fn selected_row(_: &iced::Theme, _: iced::widget::button::Status) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: Some(iced::Background::Color(CARD_BG_HOVER)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Card-as-button (playlist / mix tiles).
fn card_button(
    _: &iced::Theme,
    status: iced::widget::button::Status,
) -> iced::widget::button::Style {
    let bg = match status {
        iced::widget::button::Status::Hovered => CARD_BG_HOVER,
        _ => CARD_BG,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}

/// Ghost chip: the held block while dragging (card + accent border,
/// deliberately more compact than a full row).
/// Accent bar marking the armed insertion slot.
fn drop_bar(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(SPOT_GREEN)),
        border: iced::Border::default().rounded(2),
        ..Default::default()
    }
}

/// Hairline divider between queue rows (visible on dark and hover).
fn hairline(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(NAV_ACTIVE_BG)),
        ..Default::default()
    }
}

fn hairline_rule() -> iced::Element<'static, Message> {
    iced::widget::container(
        iced::widget::container(
            iced::widget::Space::new()
                .height(iced::Length::Fixed(1.0))
                .width(iced::Length::Fill),
        )
        .style(hairline),
    )
    // Total layout 1 plus 2 top plus 2 bottom (5px); only the inner
    // 1px paints, the rest is transparent breathing room.
    .padding(iced::Padding {
        top: 2.0,
        bottom: 2.0,
        left: 0.0,
        right: 0.0,
    })
    .into()
}

/// Interleave hairline dividers between rows (never trailing).
fn with_dividers<'a>(rows: Vec<iced::Element<'a, Message>>) -> Vec<iced::Element<'a, Message>> {
    let mut out = Vec::with_capacity(rows.len() * 2);
    let mut first = true;
    for row in rows {
        if !first {
            out.push(hairline_rule());
        }
        first = false;
        out.push(row);
    }
    out
}
/// Row click target: transparent in every state (the row container owns
/// the hover highlight, so fast moves never leave a title-only pill).
fn row_click(_: &iced::Theme, _: iced::widget::button::Status) -> iced::widget::button::Style {
    iced::widget::button::Style {
        background: None,
        text_color: iced::Color::WHITE,
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}
/// Full-row hover background: transparent at rest, soft card highlight when
/// the cursor is anywhere over the row (the row container owns this; the
/// inner buttons keep their own `hover_row` press targets).
fn row_bg(hovered: bool) -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    move |_: &iced::Theme| iced::widget::container::Style {
        background: Some(iced::Background::Color(if hovered {
            CARD_BG_HOVER
        } else {
            iced::Color::TRANSPARENT
        })),
        border: iced::Border::default().rounded(8),
        ..Default::default()
    }
}
/// One context-menu row: icon + label, full-width hover press target.
fn menu_item(
    icon_name: &'static str,
    label: String,
    msg: Message,
) -> iced::Element<'static, Message> {
    use iced::widget::{button, row, text};
    use iced::Fill;
    button(
        row![
            icon(icon_name, 14.0, iced::Color::WHITE),
            text(label).size(14),
        ]
        .spacing(8)
        .align_y(iced::alignment::Vertical::Center)
        .width(Fill),
    )
    .style(hover_row)
    .on_press(msg)
    .padding(8)
    .width(Fill)
    .into()
}

/// Dim backdrop over art for the hover play overlay: darkens the
/// image so the white play icon reads on top.
fn art_dim(_: &iced::Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(iced::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.45,
        })),
        ..Default::default()
    }
}

/// Row art size (matches `thumb`) + hidden action width (icon 15 +
/// padding 6 each side): spacers keep rows steady when actions hide.
const ROW_ART: f32 = 40.0;
const ROW_ACTION_W: f32 = 27.0;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Home,
    Search,
    Queue,
    Library,
    Discovery,
    Settings,
    Artist(String),
    Album(String),
    PlaylistDetail(String),
    Lyrics(String),
    Radio(String),
    NowPlaying,
}

/// In-progress right-panel drag: source index + insertion slot.
/// Slots live strictly *between* rows (1..=len-1): drops above the
/// first row or below the last are not representable, so the ghost
/// never floats over empty space. The origin row collapses to zero
/// height while dragging; siblings close ranks around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DragState {
    from: usize,
    slot: Option<usize>,
}

/// Press snapshot for click-vs-drag: a release with no committed move
/// over the same unmutated row counts as a click (plays it).
#[derive(Debug, Clone)]
struct PressSnap {
    from: usize,
    pos: Option<iced::Point>,
    id: String,
    len: usize,
}

/// Press within slop of release (px): hand tremor must not kill a click.
const CLICK_SLOP_PX: f32 = 6.0;

/// Press and release positions match for click purposes (exact when the
/// cursor never moved, slop-tolerant otherwise).
fn release_is_click_at(press: Option<iced::Point>, release: Option<iced::Point>) -> bool {
    match (press, release) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            dx * dx + dy * dy < CLICK_SLOP_PX * CLICK_SLOP_PX
        }
        _ => false,
    }
}

/// Right-panel tab: queue or recently played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Queue,
    Recent,
}

/// Search page type filter (chip row above the results). The backend
/// returns songs, albums, artists and community playlists, so those
/// are the only chips: no empty categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Albums,
    Artists,
    Playlists,
}

/// Which sign-in started an import (status wording only). `Sync` is a
/// manual or scheduled refresh (same pipeline, "Sync" wording).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "innertube")]
pub enum ImportKind {
    Cookie,
    Library,
    Sync,
}

/// One in-flight playlist fetch for the banner bar: cumulative arrived
/// songs plus the best-effort total (unknown when details disagrees).
#[derive(Debug, Clone, Default)]
#[cfg(feature = "innertube")]
struct PlaylistLive {
    arrived: usize,
    count: Option<usize>,
}

#[cfg(feature = "innertube")]
impl ImportKind {
    /// Short source name for failure lines ("Cookie import failed").
    fn label(self) -> &'static str {
        match self {
            ImportKind::Cookie => "Cookie",
            ImportKind::Library => "Library",
            ImportKind::Sync => "Sync",
        }
    }

    /// Completion headline ("Cookie import complete").
    fn done_label(self) -> &'static str {
        match self {
            ImportKind::Cookie => "Cookie import",
            ImportKind::Library => "Import",
            ImportKind::Sync => "Sync",
        }
    }
}

/// Base pull payload: the staged import plus the flags sync needs to
/// reconcile safely (see [`StagedImport`]).
#[derive(Debug, Clone, Default)]
#[cfg(feature = "innertube")]
struct BasePull {
    tracks: Vec<Track>,
    liked_ids: Vec<String>,
    disliked_ids: Vec<String>,
    headers: Vec<yutu_core::ServerPlaylist>,
    headers_clean: bool,
    library_ok: bool,
}

/// First import stage: songs + liked + playlist headers, plus the
/// connected backend so per-playlist fetches reuse it (no reconnect per
/// list). Playlists then stream in next, one task each, keeping the
/// window alive with progress instead of freezing on one long future.
/// `headers_clean` is false when header pages failed (sync must not
/// prune then: absence may be a failed page, not a web deletion);
/// `library_ok` is false when the library fetch failed open (sync must
/// not replace likes/dislikes then).
#[derive(Debug, Clone)]
#[cfg(feature = "innertube")]
pub struct StagedImport {
    pub backend: yutu_core::InnertubeBackend,
    pub tracks: Vec<Track>,
    pub liked_ids: Vec<String>,
    pub disliked_ids: Vec<String>,
    pub headers: Vec<yutu_core::ServerPlaylist>,
    pub headers_clean: bool,
    pub library_ok: bool,
}

/// Search collection menu target: which album/artist hit or
/// community playlist the open menu belongs to (track menus and user
/// playlist menus keep their own id-keyed state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionMenu {
    Album(String),
    Artist(String),
    Playlist(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Route),
    SearchChanged(String),
    Search,
    PlayTrack(usize),
    Pause,
    TogglePause,
    EnqueueTrack(String),
    /// Append to the end of the queue without touching playback.
    QueueTrack(String),
    NewPlaylistNameChanged(String),
    Stop,
    Next,
    Prev,
    Like(String),
    /// Dislike ("don't recommend"): toggles `like/dislike` server-side.
    Dislike(String),
    SetVolume(f32),
    QueueRemove(usize),
    QueueUp(usize),
    QueueDown(usize),
    QueueClear,
    OpenArtist(String),
    OpenAlbum(String),
    OpenPlaylist(String),
    OpenLyrics(String),
    StartRadio(String),
    PlayAll,
    CreatePlaylist(String),
    AddToPlaylist {
        playlist_id: String,
        track_id: String,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        track_id: String,
    },
    DeletePlaylist(String),
    SelectEqPreset(String),
    ScrobbleToggled(bool),
    ScrobbleUserChanged(String),
    ScrobbleLbTokenChanged(String),
    ScrobbleFmKeyChanged(String),
    ScrobbleFmSecretChanged(String),
    ScrobbleFmSkChanged(String),
    ScrobbleSubmitted {
        target: String,
        result: Result<(), String>,
    },
    PlayWeeklyMix,
    PlayForgotten,
    SearchCompleted {
        query: String,
        result: Result<Box<SearchBundle>, String>,
    },
    RadioReady {
        seed: String,
        result: Result<Vec<Track>, String>,
    },
    QueueJump(usize),
    ToggleShuffle,
    CycleRepeat,
    Tick(std::time::Instant),
    SeekTo(f32),
    /// Exact-ms seek (lyric verse clicks; no f32 truncation).
    SeekToMs(u64),
    NavBack,
    NavForward,
    SetRightTab(RightTab),
    SetSearchFilter(SearchFilter),
    SuggestCompleted {
        gen: u64,
        result: Result<Vec<String>, String>,
    },
    /// Click-to-play: replace the queue with this track + autoplay tail.
    PlayNow(String),
    AutoplayReady {
        seed: String,
        result: Result<Vec<Track>, String>,
    },
    /// Insert after the current item (context menu: play next).
    PlayNext(String),
    /// Play an autoplay-tail item now (it joins the manual queue).
    PlayAutoplay(usize),
    /// Play-all with shuffle turned on first (hero shuffle buttons).
    PlayAllShuffled,
    /// Play one playlist from the clicked row on: the song plus the
    /// rest of the playlist from that point replace the queue.
    PlayPlaylistAt {
        playlist_id: String,
        index: usize,
    },
    /// Shuffle-play one search hero artist (known catalog tracks).
    PlayArtistShuffled(String),
    /// Shuffle-play one search hero album (known catalog tracks).
    PlayAlbumShuffled(String),
    /// Play one search-result playlist now (tracks fetch live first).
    PlaySearchPlaylist(String),
    /// Shuffle-play one search-result playlist (tracks fetch live first).
    PlaySearchPlaylistShuffled(String),
    /// A search-result playlist arrived (or failed): queues on success,
    /// honest status on failure.
    SearchPlaylistReady {
        id: String,
        label: String,
        shuffle: bool,
        result: Result<Vec<Track>, String>,
    },
    /// Remove one occurrence from the manual queue (menu / hover remove).
    RemoveFromQueue(String),
    /// Drop an autoplay-tail item by index.
    RemoveAutoplay(usize),
    /// Queue-page hover tracking (reveals the row remove).
    QueueHover(Option<usize>),
    /// Id-keyed row hover (full-row highlight on non-index lists).
    TrackHover(Option<String>),
    /// Sidebar playlist hover (reveals the row play button).
    PlaylistHover(Option<String>),
    /// Autoplay hover tracking (reveals the row remove).
    AutoHover(Option<usize>),
    /// Search collection hover (reveals the row highlight + menu).
    AlbumHover(Option<String>),
    ArtistHover(Option<String>),
    SearchPlaylistHover(Option<String>),
    /// Right-panel drag reorder (row press, slot hover, release anywhere).
    DragStart(usize),
    /// Global left press: arms a drag when the cursor is over a queue
    /// row (inner buttons swallow the row area press, so the row alone
    /// cannot arm from text/art; this covers the whole row).
    GlobalPress,
    /// Insertion-slot hover while dragging (0..=len, edges included).
    DragSlot(usize),
    /// Animation frame (sidebar widths + drawer slide converge).
    Animate,
    /// Bottom drawer open/close.
    ToggleDrawer,
    /// Drawer lyrics scroll offset (sticky header past 200px).
    DrawerScrolled(f32),
    /// A scrollable moved: lights its scrollbar fade, which the ticker
    /// eases out after idle (the key matches a `scroll_hint` entry).
    Scrolled(&'static str),
    DragEnd,
    /// Right-panel row hover: highlight + drag-target tracking.
    PanelHover(usize),
    /// Remove-button hover (presses there never arm a drag).
    RemoveHover(Option<usize>),
    /// Context menu open/close: right-click (per-row `mouse_area`) or the
    /// per-row `…` button opens it at the last cursor position.
    OpenContext(String),
    CloseContext,
    /// Dismiss the search popup only (scrim outside the input).
    CloseSuggest,
    /// Clear every row hover (scrims swallow hovers so nothing behind
    /// a popup or menu highlights).
    ClearHovers,
    /// Playlist context menu open/close (sidebar rows, detail header).
    OpenPlaylistMenu(String),
    /// Search collection menu open (album/artist/community-playlist
    /// rows): right-click or the hover `…` button, same frozen anchor
    /// as the track and playlist menus.
    OpenAlbumMenu(String),
    OpenArtistMenu(String),
    OpenSearchPlaylistMenu(String),
    /// Play a playlist from the top (clears the queue first).
    PlayPlaylist(String),
    /// Append a whole playlist to the queue (never interrupts).
    QueuePlaylist(String),
    /// Shuffle a playlist then play from the top.
    ShufflePlaylist(String),
    /// Toggle the playlist flyout for a track (menu stays open).
    TogglePlaylistSubmenu(String),
    CursorMoved(iced::Point),
    WindowResized(iced::Size),
    /// Context menu: jump to this track's album/artist detail.
    OpenTrackAlbum(String),
    OpenTrackArtist(String),
    /// Cache the resolved audio bytes for offline listening.
    DownloadTrack(String),
    Downloaded {
        track_id: String,
        result: Result<(), String>,
    },
    ToggleSidebarPin,
    ToggleRightPin,
    /// Animation master toggle (Settings).
    AnimationsToggled(bool),
    /// Animation duration in ms 20..=400 (Settings slider).
    AnimMsChanged(f32),
    SidebarFilterChanged(String),
    CookiePathChanged(String),
    CookieImport,
    /// First import stage done (songs + liked + playlist headers).
    #[cfg(feature = "innertube")]
    ImportStageReady {
        kind: ImportKind,
        result: Result<StagedImport, String>,
    },
    /// One playlist's tracks arrived (or failed); import continues.
    /// `gen` quarantines stragglers from an older import run.
    #[cfg(feature = "innertube")]
    PlaylistImported {
        kind: ImportKind,
        gen: u64,
        title: String,
        index: usize,
        total: usize,
        count: Option<usize>,
        result: Result<Playlist, String>,
    },
    /// One playlist page mapped (cumulative song count plus the
    /// best-effort total): moves the banner bar song by song. Data
    /// still merges only on [`Message::PlaylistImported`].
    #[cfg(feature = "innertube")]
    PlaylistTick {
        gen: u64,
        index: usize,
        arrived: usize,
        count: Option<usize>,
    },
    HomeSearchChanged(String),
    SearchStay,
    SearchDebounced {
        gen: u64,
        query: String,
    },
    UrlResolved {
        track_id: String,
        result: Result<String, String>,
    },
    UrlPrefetched {
        track_id: String,
        result: Result<String, String>,
    },
    LyricsReady {
        track_id: String,
        result: Result<LyricsResult, String>,
    },
    ArtworkReady {
        url: String,
        result: Result<Vec<u8>, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLoginStart,
    #[cfg(feature = "innertube")]
    GoogleLoginBegan {
        result: Result<yutu_core::GoogleLogin, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLoginPoll,
    #[cfg(feature = "innertube")]
    GoogleLoginPolled {
        result: Result<Option<yutu_core::OAuthPersist>, String>,
    },
    #[cfg(feature = "innertube")]
    GoogleLogout,
    #[cfg(feature = "innertube")]
    LibraryImport,
    /// Re-import via the saved cookie session (no file picker needed).
    #[cfg(feature = "innertube")]
    CookieSessionImport,
    /// Delete the saved cookie session (stays out of the account).
    #[cfg(feature = "innertube")]
    CookieLogout,
    /// Manual refresh from Settings: replays pending pushes, then pulls
    /// web changes (likes, playlist songs/titles, deletions) via the
    /// saved session. Same pipeline as imports, "Sync" wording.
    #[cfg(feature = "innertube")]
    SyncNow,
    /// Scheduled-sync master toggle (Settings).
    #[cfg(feature = "innertube")]
    AutoSyncToggled(bool),
    /// Best-effort server like/unlike result (local state is already
    /// applied: only failures report, so the row never lies).
    #[cfg(feature = "innertube")]
    LikeSynced {
        track_id: String,
        liked: bool,
        result: Result<(), String>,
    },
    /// Best-effort server dislike/clear result (same local-first contract
    /// as [`Message::Like`]).
    #[cfg(feature = "innertube")]
    DislikeSynced {
        track_id: String,
        disliked: bool,
        result: Result<(), String>,
    },
    /// Server playlist creation result: carries the local shell id so the
    /// handler can migrate it to the real server id.
    #[cfg(feature = "innertube")]
    ServerPlaylistCreated {
        local_id: String,
        title: String,
        result: Result<String, String>,
    },
    /// Best-effort server playlist add/remove/delete result (local state
    /// is already applied: only failures report). `op` identifies the
    /// push so the handler can clear its pending row on success.
    #[cfg(feature = "innertube")]
    ServerPlaylistSynced {
        detail: String,
        op: PlaylistSyncOp,
        result: Result<(), String>,
    },
    StreamResolved {
        video_id: String,
        result: Result<Vec<u8>, String>,
    },
    /// Background hydration for ids the catalog lacks (liked/history
    /// rows from older installs, or playlist rows missing art): the
    /// payload registers into the catalog + track cache + artwork.
    TracksHydrated {
        requested: Vec<String>,
        tracks: Vec<Track>,
    },
}

/// Which playlist push a [`Message::ServerPlaylistSynced`] reports, so
/// the handler clears exactly that pending row (a stale in-flight task
/// must never clear a newer intent).
#[derive(Debug, Clone)]
#[cfg(feature = "innertube")]
pub enum PlaylistSyncOp {
    Add {
        playlist_id: String,
        track_id: String,
    },
    Remove {
        playlist_id: String,
        track_id: String,
    },
    Delete {
        playlist_id: String,
    },
}

/// Playback sink: mpv when the `mpv` feature is on and libmpv initializes
/// (real seek + position + audible EQ), otherwise rodio (pure Rust).
/// The UI only talks to this enum: never to a concrete backend.
enum AudioOut {
    Rodio(RodioBackend),
    #[cfg(feature = "mpv")]
    Mpv(MpvBackend),
}

impl AudioOut {
    fn try_new() -> Option<Self> {
        #[cfg(feature = "mpv")]
        if let Ok(mpv) = MpvBackend::new() {
            return Some(Self::Mpv(mpv));
        }
        RodioBackend::try_new().ok().map(Self::Rodio)
    }

    /// True when this backend plays a URL progressively (mpv). Rodio needs
    /// full bytes, so the UI downloads first on that path.
    fn wants_stream(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    /// Hand mpv a stream URL (no download). Rodio cannot do this.
    fn play_stream_url(&self, url: &str) -> Result<(), String> {
        let _ = url;
        match self {
            Self::Rodio(_) => Err("progressive streaming needs the mpv backend".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_url(url)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    fn backend_name(&self) -> &'static str {
        match self {
            Self::Rodio(_) => "rodio",
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => "mpv",
        }
    }

    /// Play fully-downloaded bytes (resolve + fetch run in Iced tasks).
    fn play_track_bytes(&self, bytes: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.play_bytes(bytes).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a
                .load_bytes(&bytes)
                .and_then(|()| a.play())
                .map_err(|e| e.to_string()),
        }
    }

    fn pause(&self) {
        match self {
            Self::Rodio(a) => a.pause(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.pause();
            }
        }
    }

    fn resume(&self) {
        match self {
            Self::Rodio(a) => a.resume(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.play();
            }
        }
    }

    fn stop(&self) {
        match self {
            Self::Rodio(a) => a.stop(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.stop();
            }
        }
    }

    fn set_volume(&self, volume: f32) {
        match self {
            Self::Rodio(a) => a.set_volume(volume),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => {
                let _ = a.set_volume(volume);
            }
        }
    }

    fn position_secs(&self) -> f64 {
        match self {
            Self::Rodio(a) => a.position_secs(),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.position_secs(),
        }
    }

    /// Backend duration when it knows better (mpv), else `fallback`.
    fn duration_secs(&self, fallback: f64) -> f64 {
        #[cfg(feature = "mpv")]
        if let Self::Mpv(a) = self {
            let d = a.duration_secs();
            if d > 1.0 {
                return d;
            }
        }
        fallback
    }

    /// Draggable seek: real on mpv, unsupported on rodio.
    fn seek_secs(&self, secs: f64) -> Result<(), String> {
        match self {
            Self::Rodio(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.seek_secs(secs).map_err(|e| e.to_string()),
        }
    }

    /// Rodio decodes to raw PCM with no filter stage: EQ is mpv-only.
    /// Callers report this honestly instead of pretending it applied.
    fn supports_eq(&self) -> bool {
        match self {
            Self::Rodio(_) => false,
            #[cfg(feature = "mpv")]
            Self::Mpv(_) => true,
        }
    }

    fn apply_eq(&self, preset: &EqPreset) -> Result<(), String> {
        #[cfg(not(feature = "mpv"))]
        let _ = preset;
        match self {
            Self::Rodio(_) => Err("EQ needs the mpv backend (libmpv)".into()),
            #[cfg(feature = "mpv")]
            Self::Mpv(a) => a.apply_eq_preset(preset).map_err(|e| e.to_string()),
        }
    }
}

/// Parse a googlevideo `expire` query param (`…&expire=1791411186&…`) into a
/// unix timestamp. Unknown shapes → None (caller re-resolves every time).
fn url_expiry(url: &str) -> Option<u64> {
    url.split(['?', '&'])
        .find_map(|p| p.strip_prefix("expire=").and_then(|v| v.parse().ok()))
}

/// Scheduled sync cadence + startup grace (see [`sync_due`]): pulls
/// are cheap (staged, fail-open) but still network, so 15 minutes with
/// a 90s grace after launch.
#[cfg(feature = "innertube")]
const AUTO_SYNC_INTERVAL_SECS: u64 = 15 * 60;
#[cfg(feature = "innertube")]
const AUTO_SYNC_STARTUP_GRACE_SECS: u64 = 90;

/// True when a scheduled pull is due (pure time math, unit-tested; the
/// caller additionally requires a session and an idle pipeline).
#[cfg(feature = "innertube")]
fn sync_due(
    auto_sync: bool,
    last_sync: Option<std::time::Instant>,
    started_at: std::time::Instant,
    now: std::time::Instant,
) -> bool {
    if !auto_sync {
        return false;
    }
    match last_sync {
        Some(t) => now.duration_since(t).as_secs() >= AUTO_SYNC_INTERVAL_SECS,
        None => now.duration_since(started_at).as_secs() >= AUTO_SYNC_STARTUP_GRACE_SECS,
    }
}

/// Relative "Last synced" label for Settings.
#[cfg(feature = "innertube")]
fn fmt_sync_ago(last_sync_unix: Option<i64>, now_unix: i64) -> String {
    match last_sync_unix {
        None => "never".into(),
        Some(t) => {
            let ago = now_unix.saturating_sub(t);
            if ago < 60 {
                "just now".into()
            } else if ago < 3_600 {
                format!("{}m ago", ago / 60)
            } else if ago < 86_400 {
                format!("{}h ago", ago / 3_600)
            } else {
                format!("{}d ago", ago / 86_400)
            }
        }
    }
}

/// Context-menu origin from the last cursor position, clamped so the
/// 300px menu never runs off screen. Falls back to a sane default when
/// the cursor/window are still unknown (e.g. keyboard-opened).
fn menu_pos(cursor: Option<iced::Point>, window: Option<iced::Size>) -> (f32, f32) {
    const MENU_W: f32 = 300.0;
    const MENU_H: f32 = 510.0;
    let (cx, cy) = cursor.map(|p| (p.x, p.y)).unwrap_or((200.0, 150.0));
    let (ww, hh) = window
        .map(|s| (s.width, s.height))
        .unwrap_or((1280.0, 800.0));
    (
        cx.min((ww - MENU_W).max(0.0)).max(0.0),
        cy.min((hh - MENU_H).max(0.0)).max(0.0),
    )
}

/// Commit mapping for an insertion slot: `slot == from` and `slot ==
/// from + 1` are the item's own spot (no-op); anything else maps to a
/// `move_to` index. `None` on no-op or out-of-range.
/// Insertion slots live between rows plus both edges (0 before the
/// first row, len after the last) so every song, including the middle
/// of 3, always has a committable move.
fn slot_commit(len: usize, from: usize, slot: usize) -> Option<usize> {
    if from >= len || slot > len {
        return None;
    }
    let to = if slot <= from { slot } else { slot - 1 };
    if to == from {
        None
    } else {
        Some(to)
    }
}

/// Preview order for an insertion slot (test helper for the commit map).
#[cfg(test)]
fn drag_slot_preview(len: usize, from: usize, slot: Option<usize>) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    if let Some(slot) = slot {
        if let Some(to) = slot_commit(len, from, slot) {
            let item = order.remove(from);
            order.insert(to, item);
        }
    }
    order
}

/// Search cache TTL (s) and cap (queries, evict-all when full).
const SEARCH_CACHE_TTL_SECS: u64 = 300;
const SEARCH_CACHE_CAP: usize = 30;

/// Retry cooldown (s) for failed artwork/metadata fetches: completion
/// chaining re-scans the visible screenful on every arrival, so without
/// this a permanently failing URL or video would refetch in a loop.
const FAILED_COOLDOWN_SECS: u64 = 120;

/// Normalize a query for cache keys (case, edges, inner runs).
fn search_cache_key(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Truncate a title to `max` chars with an ellipsis (menu rows).
fn truncate_title(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        title.to_owned()
    } else {
        format!("{}…", title.chars().take(max).collect::<String>())
    }
}

/// Fixed row/card heights: every card and row keeps one height and long
/// names truncate with an ellipsis instead of stretching the container.
/// Containers pair these with `.clip(true)` as a backstop, so even an
/// untruncated overflow can never move its neighbours.
const HOME_CARD_W: f32 = 156.0;
const HOME_PICK_ART: f32 = 140.0;
const HOME_CARD_H: f32 = 200.0;
/// Unified search list cap (songs + albums + artists mixed): the page
/// stays fast while every type keeps its head results visible.
const MIXED_SEARCH_CAP: usize = 30;
/// Breathing room below the home pick strip: the floating horizontal
/// scrollbar sits on the strip's bottom edge, clear of the cards above
/// and the next section below.
const HOME_STRIP_GAP: f32 = 14.0;
const TRACK_ROW_H: f32 = 52.0;
const SUGGEST_ROW_H: f32 = 44.0;
const QUEUE_ROW_H: f32 = 58.0;
const AUTO_ROW_H: f32 = 44.0;
/// Scrollbar fade: idle delay before the fade starts plus the per-tick
/// step (~8 frames on the Animate tick, matching panel glides).
const SCROLL_FADE_DELAY_MS: u64 = 700;
const SCROLL_FADE_STEP: f32 = 0.12;

/// Hover hysteresis grace (px): how far outside the expanded panel the
/// cursor may roam before an unpinned sidebar collapses.
const SIDEBAR_GRACE: f32 = 16.0;
/// Same contract for the right panel (measured from the window edge).
const RIGHT_GRACE: f32 = 16.0;
/// Collapsed right rail width; the expand zone starts just outside it.
const RIGHT_RAIL_W: f32 = 56.0;
/// How far outside the collapsed rail the expand zone reaches.
const RIGHT_EXPAND_GRACE: f32 = 8.0;
/// Bottom strip reserved for the player bar: sidebar hover expand never
/// fires from inside this zone. Measured: status line (~16) + control
/// row (~50) + player padding (2x10) + spacings (~16) with margin.
/// The stacked minimal player is taller (see player_zone_h).
const PLAYER_ZONE_H: f32 = 105.0;
/// Minimal stacked player reserve (4 rows + status + padding).
const PLAYER_ZONE_MINIMAL_H: f32 = 210.0;
/// Player breakpoints on window width: single row down to 700,
/// tight single row down to 500, stacked below that.
const PLAYER_COMPACT_BELOW: f32 = 700.0;
const PLAYER_MINIMAL_BELOW: f32 = 500.0;

/// Player bar layout mode (responsive narrow/medium/wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayerLayout {
    Wide,
    Compact,
    Minimal,
}

/// Window width to player layout (pure helper, unit-tested).
fn player_layout_mode(window_width: f32) -> PlayerLayout {
    if window_width < PLAYER_MINIMAL_BELOW {
        PlayerLayout::Minimal
    } else if window_width < PLAYER_COMPACT_BELOW {
        PlayerLayout::Compact
    } else {
        PlayerLayout::Wide
    }
}

/// Bottom-strip reserve for the current player height (the stacked
/// minimal player is taller than the single row).
impl AppState {
    fn player_zone_h(&self) -> f32 {
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        if player_layout_mode(ww) == PlayerLayout::Minimal {
            PLAYER_ZONE_MINIMAL_H
        } else {
            PLAYER_ZONE_H
        }
    }
}
/// Animation tick and duration slider bounds (Settings slider, ms).
const ANIM_TICK_MS: u64 = 16;
const ANIM_MIN_MS: u64 = 20;
const ANIM_MAX_MS: u64 = 400;
const ANIM_DEFAULT_MS: u64 = 100;
/// Total travels used to derive per-tick steps for a target duration.
const SIDEBAR_TRAVEL: f32 = 179.0;
/// Right worst case (285 to 0 when the rail is empty) so every right
/// glide finishes within the window; shorter glides snap early.
const RIGHT_TRAVEL: f32 = 285.0;
/// Per-tick steps for a target duration in ms at the 16ms tick:
/// (sidebar px, right px, unit 0..1). Widths floor at 1px.
fn anim_steps(anim_ms: u64) -> (f32, f32, f32) {
    let frames = ((anim_ms as f32 / ANIM_TICK_MS as f32).round() as u32).max(1) as f32;
    (
        (SIDEBAR_TRAVEL / frames).max(1.0),
        (RIGHT_TRAVEL / frames).max(1.0),
        1.0 / frames,
    )
}

/// Header-height reserve above the Now Playing sheet: measured header
/// row (22px icon + 2x10 padding = 42) plus the 4px column spacing.
/// Tied to the header control heights; the fully-open body layer drops
/// it (real header above) so both states meet with no snap.
const DRAWER_TOP_RESERVE: f32 = 46.0;
/// Center content cap on very wide windows (body only, header stays
/// full bleed so search math never drifts).
const CENTER_MAX_W: f32 = 1150.0;
/// Window width past which the center body centers with margins.
const CENTER_MARGIN_BREAK: f32 = 1600.0;
/// Top strip the search-popup scrim leaves clickable (header + input).
const SUGGEST_SCRIM_TOP: f32 = 110.0;

/// Ease `current` toward `target` by `step` (frame tick for drawer and
/// sidebar transitions). Snaps when closer than one step.
fn ease_toward(current: f32, target: f32, step: f32) -> f32 {
    if (target - current).abs() <= step {
        target
    } else {
        current + (target - current).signum() * step
    }
}

/// Format seconds as `m:ss` for the player bar.
fn fmt_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Duration text for search rows: raw All results carry no durations,
/// so a zero one renders blank, never "0:00".
fn duration_text(secs: u32) -> iced::Element<'static, Message> {
    use iced::widget::text;
    if secs > 0 {
        text(fmt_time(secs as f64)).size(12).color(MUTED).into()
    } else {
        text("").size(12).into()
    }
}

pub struct AppState {
    pub route: Route,
    pub search_query: String,
    pub results: SearchResults,
    /// Server-ordered All response for the current query (keystroke
    /// narrowed like `results`; the All tab renders it as-is).
    pub all_results: SearchAll,
    pub queue: Queue,
    pub liked: HashSet<String>,
    /// Liked ids in add order (membership stays in `liked`).
    pub liked_order: Vec<String>,
    /// Disliked ("don't recommend") ids: membership plus add order below.
    pub disliked: HashSet<String>,
    /// Disliked ids in add order (membership stays in `disliked`).
    pub disliked_order: Vec<String>,
    /// Downloaded ids (offline audio on disk) in add order.
    pub downloaded: HashSet<String>,
    /// Downloaded ids in add order (membership stays in `downloaded`).
    pub downloaded_order: Vec<String>,
    pub history: Vec<String>,
    pub volume: f32,
    pub status: String,
    pub user_playlists: Vec<Playlist>,
    pub new_playlist_name: String,
    pub playlist_seq: u64,
    /// Selected EQ preset name (in-memory only; never persisted).
    pub eq_preset: String,
    /// Scrobble toggle (in-memory only; submission needs credentials below).
    pub scrobble_enabled: bool,
    /// Scrobble username (in-memory only; never persisted).
    pub scrobble_user: String,
    /// ListenBrainz user token (in-memory only; never persisted or logged).
    pub scrobble_lb_token: String,
    /// Last.fm API key / shared secret / session key (in-memory only).
    pub scrobble_fm_key: String,
    pub scrobble_fm_secret: String,
    pub scrobble_fm_sk: String,
    paused: bool,
    audio: Option<AudioOut>,
    /// Last Tick-observed playback position (drives the progress bar).
    position_secs: f64,
    /// Track id currently being fetched (stale arrivals are dropped).
    loading_track: Option<String>,
    /// Resolved stream URLs by track id: (url, expire_unix). Fresh hits
    /// play instantly: back/prev/replay never re-resolve.
    url_cache: HashMap<String, (String, u64)>,
    /// Next song's stream, resolved early (played instantly on advance).
    prefetched: Option<(String, String)>,
    /// Lyrics by track id (LRCLIB, no login) + in-flight fetch.
    lyrics_cache: HashMap<String, LyricsResult>,
    lyrics_loading: Option<String>,
    /// Browser-style back/forward stacks (Route history, capped).
    nav_back: Vec<Route>,
    nav_forward: Vec<Route>,
    /// Right-panel tab (view-only state).
    right_tab: RightTab,
    /// Queue id order before shuffle was turned on. Toggling off restores
    /// it (ids added while shuffled append at the end); the playing song
    /// never skips: [`Queue::reorder`] retargets its index by identity.
    pre_shuffle: Option<Vec<String>>,
    /// Current track when shuffle turned on (id + occurrence rank): if it
    /// is still current at restore time, this exact occurrence is
    /// re-seated (`reorder` alone retargets duplicates by first match).
    pre_shuffle_cur: Option<(String, usize)>,
    /// Suggestion popup: texts + generation + visibility.
    suggest: Vec<String>,
    suggest_gen: u64,
    show_suggest: bool,
    /// Last persisted queue snapshot (changed-identity check for the Tick
    /// saver: queue writes land within one tick, positions within 30s).
    queue_saved: Option<QueueSnapshot>,
    /// Last queue save time (positions refresh at most every 30s).
    queue_saved_at: Option<std::time::Instant>,
    /// Saved position to seek after a restored queue starts playing
    /// (dropped on any other load or failure).
    pending_resume: Option<f64>,
    /// A session restore is waiting for its first play (the play button
    /// loads the saved track instead of resuming an empty backend).
    restored_pending: bool,
    /// Context menu open for this track id (None = closed).
    context_menu: Option<String>,
    /// Playlist menu open for this playlist id (None = closed).
    pl_menu: Option<String>,
    /// Search collection menu open (None = closed).
    coll_menu: Option<CollectionMenu>,
    /// Playlist flyout open for this track id (side panel, menu stays).
    submenu_playlists_for: Option<String>,
    /// Cursor position frozen when the menu opened (the menu must not
    /// chase the mouse while the user aims at it).
    menu_at: Option<iced::Point>,
    /// Last cursor position (window coords: anchors the context menu).
    cursor: Option<iced::Point>,
    /// Last window size (clamps the context menu on screen).
    window: Option<iced::Size>,
    /// Pinned sidebars stay open; unpinned ones reveal on edge hover.
    sidebar_pinned: bool,
    right_pinned: bool,
    /// Latched hover states (updated in `CursorMoved`; see `sidebar_open`).
    sidebar_latched: bool,
    right_latched: bool,
    /// Animated sidebar widths (ease toward the open/closed targets).
    sidebar_w: f32,
    right_w: f32,
    /// Bottom drawer (Now Playing + lyrics): open flag + slide 0..1.
    drawer_open: bool,
    drawer_anim: f32,
    /// Animation prefs (in-memory; never persisted): master toggle plus
    /// a duration in ms for panel glides.
    animations_enabled: bool,
    anim_ms: u64,
    /// Drawer lyrics scroll offset (sticky compact header past 200px).
    drawer_scroll: f32,
    /// Scrollbar fade per scrollable key (last motion + opacity 0..1):
    /// scroll motion lights the bar, the ticker fades it after idle.
    /// Never persisted; missing keys read as invisible.
    scroll_hint: HashMap<&'static str, (std::time::Instant, f32)>,
    /// Playlist filter text in the sidebar.
    sidebar_filter: String,
    /// Autoplay tail belongs to this seed (stale arrivals drop).
    autoplay_for: Option<String>,
    /// Spotify-style split: manual queue (user picks) plays first, then
    /// this radio-picked tail ("Next up"). Never persisted.
    autoplay: Vec<Track>,
    /// Queue-page row under the cursor (hover reveals its remove).
    hovered_queue: Option<usize>,
    /// Cursor over a row remove button (presses there must never arm a
    /// drag: slipping off delete must not become a reorder).
    hover_remove: Option<usize>,
    /// Track-row hover by id (reveals full-row highlight where lists are
    /// not index-stable: search/library/discovery rows + recent).
    hovered_track: Option<String>,
    /// Search collection hover by id (album/artist/playlist rows).
    hovered_album: Option<String>,
    hovered_artist: Option<String>,
    hovered_search_playlist: Option<String>,
    /// Autoplay row under the cursor (same reveal, separate index space).
    hovered_auto: Option<usize>,
    /// Right-panel drag in progress (row press to slot hover to release).
    drag: Option<DragState>,
    /// Press snapshot for click detection (cleared on every release).
    press_snap: Option<PressSnap>,
    /// Sidebar playlist under the cursor (hover play affordance).
    hovered_playlist: Option<String>,
    /// Last committing reorder time: a shared press/release with the
    /// drop eats the row click that follows it (150ms window).
    just_reordered_at: Option<std::time::Instant>,
    /// Eased drop-slot gap 0..1 (opens room for the ghost chip).
    drag_gap: f32,
    /// Recent playback/fetch failures (capped, surfaced in Settings).
    error_log: Vec<String>,
    /// Manual position clock deadline after a user seek (anti-flicker).
    seek_hold_until: Option<std::time::Instant>,
    /// A seek landed while paused (backend clock stays stale until resume).
    paused_seek: bool,
    /// Last Tick instant (drives the manual clock during a seek hold).
    last_tick: Option<std::time::Instant>,
    /// Cookie-file path for import (Settings).
    cookie_path: String,
    /// Where the queue is playing from (playlist/artist/radio label).
    playing_from: String,
    /// Artwork by thumbnail URL (iced image handles) + in-flight set.
    artwork: HashMap<String, iced::widget::image::Handle>,
    artwork_loading: HashSet<String>,
    /// Failed artwork URLs with the failure instant: sketchy or removed
    /// covers are not refetched for a cooldown (stops retry loops when
    /// completion chaining re-scans the visible screenful).
    artwork_failed: HashMap<String, std::time::Instant>,
    /// Ids whose metadata probe came back empty, with the instant:
    /// unresolvable videos are not re-probed for a cooldown.
    hydrate_cooldown: HashMap<String, std::time::Instant>,
    /// Ids with a metadata probe in flight (dedups lazy batches
    /// across navigations and ticks: no repeat fetch while pending).
    hydrating: HashSet<String>,
    /// First Tick after launch runs the Home lazy batch (startup
    /// itself does zero network so the window appears instantly).
    lazy_init_done: bool,
    /// Track id a prefetch was already attempted for (no Tick spam).
    prefetch_attempted: Option<String>,
    /// Google device-flow login in progress (cloned into poll tasks).
    #[cfg(feature = "innertube")]
    google_login: Option<yutu_core::GoogleLogin>,
    /// A saved OAuth token exists on disk (signed in).
    #[cfg(feature = "innertube")]
    logged_in: bool,
    /// A saved cookie session exists on disk (stay signed in).
    #[cfg(feature = "innertube")]
    cookie_logged_in: bool,
    /// A login/library network task is in flight.
    #[cfg(feature = "innertube")]
    login_busy: bool,
    /// Staged import progress: songs (liked) in the base stage, playlists
    /// done/total, plus the run generation (stale playlist arrivals from
    /// an older run are ignored). Powers the per-playlist status line.
    #[cfg(feature = "innertube")]
    import_songs: usize,
    #[cfg(feature = "innertube")]
    import_likes: usize,
    #[cfg(feature = "innertube")]
    import_done: usize,
    #[cfg(feature = "innertube")]
    import_total: usize,
    #[cfg(feature = "innertube")]
    import_gen: u64,
    /// An import is in flight: the top banner shows live progress.
    #[cfg(feature = "innertube")]
    import_active: bool,
    /// Expected import songs (base songs plus known playlist totals) and
    /// playlist songs already finished: banner bar math. Per-list live
    /// counts arrive via ticks; finished indices quarantine late ticks.
    #[cfg(feature = "innertube")]
    import_expected: usize,
    #[cfg(feature = "innertube")]
    import_live: HashMap<usize, PlaylistLive>,
    #[cfg(feature = "innertube")]
    import_finished: HashSet<usize>,
    /// Banner display count, eased toward [`Self::import_song_target`]
    /// every Tick (see [`Self::ease_import_shown`]).
    #[cfg(feature = "innertube")]
    import_shown: usize,
    /// Scheduled sync: pulls web changes every [`AUTO_SYNC_INTERVAL`]
    /// while signed in (plus a manual Sync button in Settings).
    #[cfg(feature = "innertube")]
    auto_sync: bool,
    /// Last completed pull (any kind): drives the scheduler and the
    /// Settings "Last synced" line.
    #[cfg(feature = "innertube")]
    last_sync: Option<std::time::Instant>,
    /// Same, as unix time, persisted so the label survives restarts.
    #[cfg(feature = "innertube")]
    last_sync_unix: Option<i64>,
    /// Launch instant: the first auto-sync waits for a grace period so
    /// startup stays instant.
    #[cfg(feature = "innertube")]
    started_at: std::time::Instant,
    /// End-of-track auto-advance already fired for the current song.
    end_advanced: bool,
    library_path: String,
    /// Real-track catalog: every track the app has seen (search, radio,
    /// playlists). ID lookups resolve here: never fixtures.
    catalog: HashMap<String, Track>,
    /// Last complete real search (local filtering base for `SearchChanged`).
    search_base: SearchResults,
    /// Last complete unfiltered search (same narrowing base for All).
    search_all_base: SearchAll,
    /// Query cache: normalized query to (arrival time, results). Repeat
    /// searches (backspace/retype, Enter-after-popup) skip the network.
    search_cache: HashMap<String, (std::time::Instant, SearchBundle)>,
    /// A real search is in flight (loading indicator).
    searching: bool,
    /// Keystroke generation: each input bumps it; only the latest
    /// debounced task may fire (no search spam while typing).
    search_gen: u64,
    /// Search page type filter (chip row): kept across queries.
    search_filter: SearchFilter,
    /// Current radio sequence (filled by `RadioReady`).
    radio_tracks: Vec<Track>,
    /// MPRIS handle for live desktop propagation (`None` in tests/headless).
    mpris: Option<yutu_desktop::SharedPlayer>,
}

/// Deterministic radio ordering: seed first, then same-artist tracks, then
/// the rest. Pure helper over caller-supplied (real) tracks.
pub fn order_radio(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    order_radio_impl(seed_id, tracks)
}

fn order_radio_impl(seed_id: &str, tracks: &[Track]) -> Vec<Track> {
    let start = tracks.iter().position(|t| t.id == seed_id).unwrap_or(0);
    let seed_artist = tracks
        .get(start)
        .map(|t| t.artist.clone())
        .unwrap_or_default();
    let mut seq = Vec::with_capacity(tracks.len());
    let seed_actual = tracks.get(start).map(|t| t.id.clone()).unwrap_or_default();
    if let Some(t) = tracks.get(start) {
        seq.push(t.clone());
    }
    for t in tracks {
        if t.id != seed_actual && t.artist == seed_artist {
            seq.push(t.clone());
        }
    }
    for t in tracks {
        if t.id != seed_actual && t.artist != seed_artist {
            seq.push(t.clone());
        }
    }
    seq
}

/// Fixture-backed radio for unit tests (never user-visible).
#[cfg(test)]
fn radio_for(seed_id: &str) -> Vec<Track> {
    order_radio(seed_id, &yutu_core::MockBackend::fixture_tracks())
}

impl AppState {
    /// Resolve a track id from the real catalog (search/radio/playlist data).
    fn find_track(&self, id: &str) -> Option<Track> {
        self.catalog.get(id).cloned()
    }

    /// True when `track_id` is already in the playlist (drives the
    /// Add-to/Remove-from label in the context menu).
    fn playlist_has(&self, playlist_id: &str, track_id: &str) -> bool {
        if playlist_id == Self::FAVORITES_ID {
            return self.liked.contains(track_id);
        }
        if playlist_id == Self::DOWNLOADED_ID {
            return self.downloaded.contains(track_id);
        }
        self.user_playlists
            .iter()
            .find(|p| p.id == playlist_id)
            .is_some_and(|p| p.tracks.iter().any(|t| t.id == track_id))
    }

    /// Instant local matches from the known catalog: paints in the popup
    /// on the same keystroke, then the live payload replaces it.
    fn catalog_hits(&self, query: &str) -> Vec<Track> {
        let q = query.to_lowercase();
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
            .take(12)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Cached results for a normalized query (miss on absent or stale).
    fn search_cache_get(&self, query: &str) -> Option<SearchBundle> {
        let (at, bundle) = self.search_cache.get(&search_cache_key(query))?;
        if at.elapsed().as_secs() > SEARCH_CACHE_TTL_SECS {
            return None;
        }
        Some(bundle.clone())
    }

    /// Store results under the normalized query (evict the oldest when
    /// full, never the whole cache).
    fn search_cache_put(&mut self, query: &str, bundle: SearchBundle) {
        if self.search_cache.len() >= SEARCH_CACHE_CAP {
            if let Some(oldest) = self
                .search_cache
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                self.search_cache.remove(&oldest);
            }
        }
        self.search_cache
            .insert(search_cache_key(query), (std::time::Instant::now(), bundle));
    }

    /// Cache-first search: paint cached results with no network task,
    /// else None so the caller fires live (or reports offline).
    fn search_cache_hit_task(&mut self, q: &str) -> Option<iced::Task<Message>> {
        let hit = self.search_cache_get(q)?;
        self.searching = false;
        Some(self.apply_search_hit(q, hit))
    }

    /// Register tracks and queue artwork (no painting). Shared by live
    /// completions (fresh or stale) and cache hits. Both halves register:
    /// track menus resolve through the catalog off either list. Raw goes
    /// first so filtered durations win (flat items carry none).
    fn stage_search(&mut self, bundle: SearchBundle) -> (SearchBundle, Vec<iced::Task<Message>>) {
        let all_tracks = self.register_tracks(bundle.all.tracks);
        let filtered_tracks = self.register_tracks(bundle.filtered.tracks);
        let bundle = SearchBundle {
            all: SearchAll {
                tracks: all_tracks,
                ..bundle.all
            },
            filtered: SearchResults {
                tracks: filtered_tracks,
                ..bundle.filtered
            },
        };
        let art_urls: Vec<String> = bundle
            .filtered
            .albums
            .iter()
            .filter_map(|a| a.thumbnail_url.clone())
            .chain(
                bundle
                    .filtered
                    .artists
                    .iter()
                    .filter_map(|a| a.thumbnail_url.clone()),
            )
            .chain(
                bundle
                    .filtered
                    .playlists
                    .iter()
                    .filter_map(|p| p.thumbnail_url.clone()),
            )
            .chain(
                bundle
                    .all
                    .albums
                    .iter()
                    .filter_map(|a| a.thumbnail_url.clone()),
            )
            .chain(
                bundle
                    .all
                    .artists
                    .iter()
                    .filter_map(|a| a.thumbnail_url.clone()),
            )
            .chain(
                bundle
                    .all
                    .playlists
                    .iter()
                    .filter_map(|p| p.thumbnail_url.clone()),
            )
            // Hi-res keys: display sites show the 544px rewrite and
            // art_thumb is an exact-key lookup, so warm what is shown.
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
            .collect();
        let mut tasks = self.artwork_tasks(&bundle.filtered.tracks);
        tasks.extend(self.artwork_tasks(&bundle.all.tracks));
        tasks.extend(self.artwork_tasks_for_urls(&art_urls));
        (bundle, tasks)
    }

    /// Paint results into results/base/status/artwork (shared by live
    /// completions and cache hits). Returns the artwork batch.
    fn apply_search_hit(&mut self, query: &str, bundle: SearchBundle) -> iced::Task<Message> {
        let n = bundle.filtered.tracks.len();
        let m = bundle.filtered.playlists.len();
        let (bundle, tasks) = self.stage_search(bundle);
        self.search_base = bundle.filtered.clone();
        self.search_all_base = bundle.all.clone();
        self.results = filter_results(&bundle.filtered, &self.search_query);
        if self.results.tracks.is_empty() && !bundle.filtered.tracks.is_empty() {
            self.results = bundle.filtered.clone();
        }
        self.all_results = filter_all(&bundle.all, &self.search_query);
        if self.all_results.tracks.is_empty() && !bundle.all.tracks.is_empty() {
            self.all_results = bundle.all;
        }
        self.prune_coll_menu();
        self.status = if m == 0 {
            format!("{n} songs for “{query}”.")
        } else {
            format!("{n} songs + {m} playlists for “{query}”.")
        };
        self.publish_mpris();
        iced::Task::batch(tasks)
    }

    /// Merge a stored row with its catalog entry: catalog wins, but
    /// art the catalog lacks is filled from the stored row (and healed
    /// back into the catalog). Queues then own full Tracks even for
    /// pre-migration rows.
    fn resolve_for_play(&mut self, t: &Track) -> Track {
        let out = self.resolve_for_play_mem(t);
        // Write-through so the healed row survives restarts.
        self.cache_tracks(std::slice::from_ref(&out));
        out
    }

    /// In-memory half of [`Self::resolve_for_play`]: catalog merge with
    /// no SQLite traffic. Bulk paths resolve everything first, then
    /// persist once, so long playlists never fsync per track on the GUI
    /// thread.
    fn resolve_for_play_mem(&mut self, t: &Track) -> Track {
        match self.find_track(&t.id) {
            Some(mut full) => {
                let mut healed = false;
                if full.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                    full.thumbnail_url = t.thumbnail_url.clone();
                    healed = true;
                }
                if full.thumbnail_large_url.is_none() && t.thumbnail_large_url.is_some() {
                    full.thumbnail_large_url = t.thumbnail_large_url.clone();
                    healed = true;
                }
                if full.plays.is_none() && t.plays.is_some() {
                    full.plays = t.plays.clone();
                    healed = true;
                }
                if healed {
                    self.catalog.insert(full.id.clone(), full.clone());
                }
                full
            }
            None => {
                self.catalog.insert(t.id.clone(), t.clone());
                t.clone()
            }
        }
    }

    /// Register freshly fetched tracks; returns them for chaining.
    /// Also upserts into the SQLite track cache so restarts rebuild the
    /// catalog (Favorites/history) with no search-first step.
    fn register_tracks(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        self.cache_tracks(&tracks);
        tracks
    }

    /// Memory-only catalog seed (startup path): inserts without touching
    /// SQLite, so opening the app never blocks on writes. Fresh network
    /// data still goes through [`Self::register_tracks`] (small batches).
    fn seed_catalog_mem(&mut self, tracks: Vec<Track>) {
        for t in tracks {
            self.catalog.insert(t.id.clone(), t);
        }
    }

    /// First screenful of track ids for the current route (lazy scope).
    /// Home shows the recent grid (or Favorites fallback); Library shows
    /// the head of Favorites; detail routes show the head of their list;
    /// the sidebar covers come from the head of each playlist's first
    /// track. Everything is capped: never a whole-library sweep.
    fn visible_ids(&self) -> Vec<String> {
        const CAP: usize = 12;
        match &self.route {
            Route::Home => {
                let mut ids: Vec<String> = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for id in self.history.iter().rev() {
                    if ids.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        ids.push(id.clone());
                    }
                }
                if ids.is_empty() {
                    ids.extend(self.liked_order.iter().take(8).cloned());
                }
                ids
            }
            Route::Library => self.liked_order.iter().take(CAP).cloned().collect(),
            Route::PlaylistDetail(id) => self
                .find_playlist(id)
                .map(|p| p.tracks.iter().take(15).map(|t| t.id.clone()).collect())
                .unwrap_or_default(),
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                self.artist_tracks(&name)
                    .into_iter()
                    .take(CAP)
                    .map(|t| t.id)
                    .collect()
            }
            Route::Album(id) => self
                .album_tracks(id)
                .into_iter()
                .take(CAP)
                .map(|t| t.id)
                .collect(),
            Route::Search => self
                .results
                .tracks
                .iter()
                .take(CAP)
                .map(|t| t.id.clone())
                .collect(),
            Route::Discovery => {
                let mut ids: Vec<String> = self.liked_order.iter().take(6).cloned().collect();
                for id in self.history.iter().rev().take(6) {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
                ids
            }
            Route::Queue => {
                let mut ids: Vec<String> = self
                    .queue
                    .items()
                    .iter()
                    .take(CAP)
                    .map(|t| t.id.clone())
                    .collect();
                ids.extend(self.autoplay.iter().take(4).map(|t| t.id.clone()));
                ids
            }
            Route::Radio(_) => self
                .radio_tracks
                .iter()
                .take(15)
                .map(|t| t.id.clone())
                .collect(),
            Route::Lyrics(id) => vec![id.clone()],
            Route::NowPlaying => self
                .queue
                .current()
                .map(|t| vec![t.id.clone()])
                .unwrap_or_default(),
            Route::Settings => Vec::new(),
        }
    }

    /// Visible tracks (resolved) for artwork warming: catalog rows first,
    /// falling back to the stored playlist row so covers show even before
    /// hydration fills the catalog.
    fn visible_tracks_for_art(&self) -> Vec<Track> {
        const CAP: usize = 12;
        match &self.route {
            Route::PlaylistDetail(id) => self
                .find_playlist(id)
                .map(|p| p.tracks.into_iter().take(15).collect())
                .unwrap_or_default(),
            Route::Home => {
                let mut out = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for id in self.history.iter().rev() {
                    if out.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            out.push(t);
                        }
                    }
                }
                if out.is_empty() {
                    out = self.favorite_tracks().into_iter().take(8).collect();
                }
                out
            }
            Route::Library => self.favorite_tracks().into_iter().take(CAP).collect(),
            _ => self
                .visible_ids()
                .into_iter()
                .filter_map(|id| self.find_track(&id))
                .take(CAP)
                .collect(),
        }
    }

    /// Sidebar playlist covers (first track of the first playlists):
    /// disk-cached art shows instantly via [`Self::art_handle`]; this
    /// warms only covers missing from both memory and disk.
    fn sidebar_cover_tracks(&self) -> Vec<Track> {
        self.all_playlists()
            .into_iter()
            .take(8)
            .filter_map(|p| {
                let first = p.tracks.first()?;
                // Catalog row wins (fresh art); stored row is the fallback.
                Some(self.find_track(&first.id).unwrap_or_else(|| first.clone()))
            })
            .collect()
    }

    /// Lazy batch for what is on screen right now: metadata probes for
    /// visible ids lacking a catalog row or art (deduped via `hydrating`,
    /// cooldown-skipped when a probe came back empty, capped small), plus
    /// artwork fetches for visible covers missing from both memory and
    /// disk (also capped). Cache hits render instantly; network fills the
    /// rest without ever sweeping the library.
    fn ensure_visible_tasks(&mut self) -> iced::Task<Message> {
        const META_CAP: usize = 8;
        let mut need_meta: Vec<String> = Vec::new();
        for id in self.visible_ids() {
            if need_meta.len() >= META_CAP {
                break;
            }
            if self.hydrating.contains(&id) || Self::cooled(&self.hydrate_cooldown, &id) {
                continue;
            }
            let needs = match self.catalog.get(&id) {
                None => true,
                Some(t) => t.thumbnail_url.is_none() && t.thumbnail_large_url.is_none(),
            };
            if needs {
                self.hydrating.insert(id.clone());
                need_meta.push(id);
            }
        }
        let mut tasks = Vec::new();
        if !need_meta.is_empty() {
            tasks.push(Self::hydrate_task(need_meta));
        }
        // Artwork: visible rows plus sidebar covers, disk-first. Both URL
        // keys are warmed (rows read the raw thumb, hero cards read the
        // 544px key): warming only one leaves the other placeholder.
        let mut art_tracks = self.visible_tracks_for_art();
        art_tracks.extend(self.sidebar_cover_tracks());
        tasks.extend(self.artwork_tasks(&art_tracks));
        iced::Task::batch(tasks)
    }

    /// True when `key` failed within the retry cooldown (stops refetch
    /// loops: completion chaining re-scans visible on every arrival).
    fn cooled(map: &HashMap<String, std::time::Instant>, key: &str) -> bool {
        map.get(key)
            .is_some_and(|at| at.elapsed().as_secs() < FAILED_COOLDOWN_SECS)
    }

    /// Record a failure with now (capped map: drop history past 500).
    fn note_failed(map: &mut HashMap<String, std::time::Instant>, key: String) {
        if map.len() >= 500 {
            map.clear();
        }
        map.insert(key, std::time::Instant::now());
    }

    /// Clear a failure on success (a later URL rotation refetches cleanly).
    fn note_success(map: &mut HashMap<String, std::time::Instant>, key: &str) {
        map.remove(key);
    }

    /// Best-effort write-through to the track metadata cache (never
    /// fails the caller: a missing DB just means next launch re-fetches).
    fn cache_tracks(&self, tracks: &[Track]) {
        if tracks.is_empty() {
            return;
        }
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.upsert_tracks(tracks);
        }
    }

    /// Full-library sweep (tests + one-off diagnostics). The live UI
    /// never calls this on navigation: [`Self::ensure_visible_tasks`]
    /// scopes every fetch to the visible screenful instead.
    #[allow(dead_code)]
    fn missing_track_ids(&self) -> Vec<String> {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let consider = |id: &str, seen: &mut HashSet<String>, out: &mut Vec<String>| {
            if !seen.insert(id.to_string()) {
                return;
            }
            let needs = match self.catalog.get(id) {
                None => true,
                Some(t) => t.thumbnail_url.is_none() && t.thumbnail_large_url.is_none(),
            };
            if needs {
                out.push(id.to_string());
            }
        };
        for id in &self.liked_order {
            consider(id, &mut seen, &mut out);
        }
        for id in &self.downloaded_order {
            consider(id, &mut seen, &mut out);
        }
        for id in &self.history {
            consider(id, &mut seen, &mut out);
        }
        for p in &self.user_playlists {
            for t in &p.tracks {
                consider(&t.id, &mut seen, &mut out);
            }
        }
        out
    }

    /// Background fetch for ids the catalog lacks. Small lazy batches
    /// only (callers cap to the visible screenful): never a library sweep.
    /// One concurrent probe per id (not one sequential task): covers on a
    /// screenful land together instead of one watch-playlist round trip at
    /// a time. Each completion registers independently.
    fn hydrate_task(ids: Vec<String>) -> iced::Task<Message> {
        if ids.is_empty() {
            return iced::Task::none();
        }
        // Cap: 8 concurrent probes per batch.
        let mut tasks = Vec::new();
        for id in ids.into_iter().take(8) {
            let requested = vec![id.clone()];
            tasks.push(iced::Task::perform(
                async move { hydrate_one(id).await },
                move |tracks| Message::TracksHydrated {
                    requested: requested.clone(),
                    tracks,
                },
            ));
        }
        iced::Task::batch(tasks)
    }

    /// On-demand fetch for one unknown id: reports "fetching" and fires
    /// the hydration probe so the song appears without a manual search.
    /// Skips when a probe for the id is already in flight (Tick or
    /// navigation fired it first): no duplicate network work.
    fn unknown_track_task(&mut self, id: &str) -> iced::Task<Message> {
        self.status = format!("Fetching info for {id}…");
        self.publish_mpris();
        if !self.hydrating.insert(id.to_string()) {
            return iced::Task::none();
        }
        Self::hydrate_task(vec![id.to_string()])
    }

    /// Merge an import into memory only (catalog + likes + playlists):
    /// returns artwork tasks for the new tracks. SQLite writes happen in
    /// the background tasks via [`Self::persist_imported`], never on the
    /// GUI thread, so big imports cannot stall the event loop (the OS
    /// flags the window frozen after seconds without event pumping).
    #[cfg(feature = "innertube")]
    fn merge_imported(
        &mut self,
        tracks: Vec<Track>,
        liked_ids: Vec<String>,
        playlists: Vec<Playlist>,
    ) -> Vec<iced::Task<Message>> {
        // Playlist rows widen the catalog: register them alongside the
        // library tracks so covers and titles survive restarts.
        let mut every = tracks;
        for p in &playlists {
            for t in &p.tracks {
                if !every.iter().any(|x| x.id == t.id) {
                    every.push(t.clone());
                }
            }
        }
        let tracks = self.register_tracks_mem(every);
        for id in &liked_ids {
            self.like_id(id);
        }
        // Re-import refreshes in place (same slot, new rows); new server
        // lists append after the local ones. Lists with unpushed edits
        // keep their local contents until the pushes land (a replace
        // would clobber intent the server hasn't seen).
        for p in playlists {
            if self.playlist_has_pending(&p.id) {
                continue;
            }
            match self.user_playlists.iter().position(|x| x.id == p.id) {
                Some(i) => self.user_playlists[i] = p,
                None => self.user_playlists.push(p),
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Memory-only track registration (no SQLite): the import path
    /// persists separately in background tasks.
    #[cfg(feature = "innertube")]
    fn register_tracks_mem(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        tracks
    }

    /// SQLite half of an import merge (runs inside background tasks, not
    /// in `update`): track cache upsert, likes with metadata when the
    /// payload carries it (bare ids otherwise), server playlists mirrored
    /// by id. Best effort throughout (an unopenable DB just means the
    /// next launch re-fetches).
    #[cfg(feature = "innertube")]
    fn persist_imported(
        library_path: &str,
        tracks: &[Track],
        liked_ids: &[String],
        playlists: &[Playlist],
    ) {
        let Ok(store) = LibraryStore::open(library_path) else {
            return;
        };
        let _ = store.upsert_tracks(tracks);
        let by_id: std::collections::HashMap<&str, &Track> =
            tracks.iter().map(|t| (t.id.as_str(), t)).collect();
        for id in liked_ids {
            if let Some(t) = by_id.get(id.as_str()) {
                let _ = store.like_track(t);
            } else {
                let _ = store.like(id);
            }
        }
        // Server playlists (real ids, not `u{seq}` shells): mirror by id
        // so re-imports refresh renames and removals instead of drifting
        // from what is on screen. Local `u{seq}` rows never collide with
        // server ids.
        for p in playlists {
            let _ = store.sync_server_playlist(&p.id, &p.title, &p.tracks);
        }
    }

    /// True for playlists that live on YouTube Music (real server ids from
    /// an import or a synced create). Local `u{seq}` shells, the virtual
    /// Favorites/Downloaded lists, and the `LM` liked-music pseudo-list
    /// (likes/dislikes sync via `rate_song`, never playlist edits) never
    /// sync as playlists.
    #[cfg_attr(not(feature = "innertube"), allow(dead_code))]
    fn is_server_playlist(id: &str) -> bool {
        if id == Self::FAVORITES_ID || id == Self::DOWNLOADED_ID || id == "LM" {
            return false;
        }
        if id.len() < 6 {
            return false;
        }
        if let Some(rest) = id.strip_prefix('u') {
            if rest.parse::<u64>().is_ok() {
                return false;
            }
        }
        true
    }

    /// Ease the banner's displayed song count toward the real arrived
    /// total (halving the gap per Tick): pages land in ~100-song bursts
    /// microseconds apart, so raw counts would teleport 0 to 100%. The
    /// display never exceeds the truth and converges exactly; the status
    /// line always shows real numbers.
    #[cfg(feature = "innertube")]
    fn ease_import_shown(&mut self) {
        if !self.import_active {
            return;
        }
        let target = self.import_song_target();
        if self.import_shown < target {
            let gap = target - self.import_shown;
            self.import_shown += gap.div_ceil(2).max(1);
        } else {
            self.import_shown = target;
        }
    }

    /// Real arrived songs for the banner: merged songs plus in-flight
    /// rows from lists with known totals (unknown-total rows join at
    /// merge, never mid-flight, so the bar never overshoots).
    #[cfg(feature = "innertube")]
    fn import_song_target(&self) -> usize {
        self.import_songs
            + self
                .import_live
                .values()
                .filter(|e| e.count.is_some())
                .map(|e| e.arrived)
                .sum::<usize>()
    }

    /// Everything the app has really seen (discovery candidate pool).
    fn catalog_tracks(&self) -> Vec<Track> {
        let mut v: Vec<Track> = self.catalog.values().cloned().collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Merge the import base stage (songs + liked) immediately so results
    /// show without waiting for playlists, then fan out one task per
    /// playlist header. Each arrival updates the status line, so a slow
    /// import reads as progress instead of a frozen window. Bumps the run
    /// generation so stragglers from an older run are ignored.
    ///
    /// Sync pulls reconcile server-wins (web likes/dislikes replace the
    /// local sets, absent server lists are pruned, still-pending pushes
    /// ride over both); headers with pending delete/add/remove rows are
    /// never fetched, so unpushed intent cannot be clobbered.
    #[cfg(feature = "innertube")]
    fn begin_playlist_stages(
        &mut self,
        kind: ImportKind,
        staged: StagedImport,
    ) -> iced::Task<Message> {
        if kind == ImportKind::Cookie {
            self.cookie_logged_in = cookie_session_exists();
        }
        // Never resurrect lists the user deleted locally but hasn't
        // pushed yet, and never refresh lists with unpushed edits.
        let headers: Vec<yutu_core::ServerPlaylist> = staged
            .headers
            .iter()
            .filter(|h| {
                !self.playlist_has_pending(&h.id)
                    && !self.has_pending_op("delete", Some(&h.id), None)
            })
            .cloned()
            .collect();
        let n = staged.tracks.len();
        let m = staged.liked_ids.len();
        let total = headers.len();
        self.import_songs = n;
        self.import_likes = m;
        self.import_done = 0;
        self.import_total = total;
        self.import_expected = n;
        self.import_shown = n;
        self.import_live.clear();
        self.import_finished.clear();
        self.import_gen += 1;
        let gen = self.import_gen;
        let mut tasks = self.reconcile_base(&staged, &headers);
        if total == 0 {
            self.login_busy = false;
            self.import_active = false;
            self.mark_synced();
            self.status = format!("{}: {n} library tracks ({m} liked).", kind.done_label());
            self.publish_mpris();
            return iced::Task::batch(tasks);
        }
        self.login_busy = true;
        self.import_active = true;
        self.status = format!(
            "{}: {n} songs ({m} liked), fetching {total} playlists…",
            kind.done_label(),
        );
        self.publish_mpris();
        let backend = staged.backend;
        let library_path = self.library_path.clone();
        tasks.extend(headers.into_iter().enumerate().map(|(i, h)| {
            Self::playlist_import_task(
                backend.clone(),
                library_path.clone(),
                kind,
                gen,
                h,
                i,
                total,
            )
        }));
        iced::Task::batch(tasks)
    }

    /// Base-stage reconcile (memory half; the disk half ran in
    /// [`Self::persist_base_pull`]): catalog register, server-wins
    /// like/dislike replace with still-pending intent re-applied, and
    /// pruning of server lists absent from clean headers. `headers` are
    /// the fetchable subset (pending-op lists filtered out).
    #[cfg(feature = "innertube")]
    fn reconcile_base(
        &mut self,
        staged: &StagedImport,
        headers: &[yutu_core::ServerPlaylist],
    ) -> Vec<iced::Task<Message>> {
        let tracks = self.register_tracks_mem(staged.tracks.clone());
        if staged.library_ok {
            self.liked = staged.liked_ids.iter().cloned().collect();
            self.liked_order = staged.liked_ids.clone();
            self.disliked = staged.disliked_ids.iter().cloned().collect();
            self.disliked_order = staged.disliked_ids.clone();
            // Still-pending pushes ride over the server sets (the disk
            // half in `apply_sync_pull` does the same).
            for (act, id) in self.pending_rate_ops() {
                match act.as_str() {
                    "like" => self.like_id(&id),
                    "unlike" => self.unlike_id(&id),
                    "dislike" => self.dislike_id(&id),
                    _ => self.undislike_id(&id),
                }
            }
        }
        if staged.headers_clean {
            let present: std::collections::HashSet<&str> =
                headers.iter().map(|h| h.id.as_str()).collect();
            let pruned: Vec<String> = self
                .user_playlists
                .iter()
                .filter(|p| {
                    Self::is_server_playlist(&p.id)
                        && !present.contains(p.id.as_str())
                        && !self.playlist_has_pending(&p.id)
                })
                .map(|p| p.id.clone())
                .collect();
            if !pruned.is_empty() {
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    for id in &pruned {
                        let _ = store.delete_playlist(id);
                    }
                }
                self.user_playlists.retain(|p| !pruned.contains(&p.id));
                if let Route::PlaylistDetail(id) = &self.route {
                    if pruned.contains(id) {
                        self.go(Route::Library);
                    }
                }
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Still-pending rate pushes as (op, track id) pairs (sync
    /// reconciliation re-applies them over server-wins sets).
    #[cfg(feature = "innertube")]
    fn pending_rate_ops(&self) -> Vec<(String, String)> {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return Vec::new();
        };
        store
            .pending_ops()
            .unwrap_or_default()
            .into_iter()
            .filter(|o| matches!(o.op.as_str(), "like" | "unlike" | "dislike" | "undislike"))
            .filter_map(|o| o.track_id.map(|id| (o.op, id)))
            .collect()
    }

    /// True when a matching pending row exists (sync skips such lists).
    #[cfg(feature = "innertube")]
    fn has_pending_op(&self, op: &str, playlist_id: Option<&str>, track_id: Option<&str>) -> bool {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return false;
        };
        store.pending_ops().unwrap_or_default().iter().any(|o| {
            o.op == op
                && o.playlist_id.as_deref() == playlist_id
                && o.track_id.as_deref() == track_id
        })
    }

    /// True when a playlist has pending add/remove rows (store-level
    /// check for background tasks, which have no `&self`).
    #[cfg(feature = "innertube")]
    fn playlist_has_pending_store(library_path: &str, playlist_id: &str) -> bool {
        let Ok(store) = LibraryStore::open(library_path) else {
            return false;
        };
        store.has_playlist_pending(playlist_id).unwrap_or(false)
    }

    /// True when a playlist has pending add/remove rows.
    #[cfg(feature = "innertube")]
    fn playlist_has_pending(&self, playlist_id: &str) -> bool {
        Self::playlist_has_pending_store(&self.library_path, playlist_id)
    }

    /// Record a not-yet-confirmed server write (see `PendingOp`).
    /// Offline builds: local state is the truth, nothing to record.
    #[cfg(feature = "innertube")]
    fn record_pending(
        &self,
        op: &str,
        playlist_id: Option<&str>,
        track_id: Option<&str>,
        title: Option<&str>,
    ) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.record_pending(op, playlist_id, track_id, title);
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn record_pending(
        &self,
        _op: &str,
        _playlist_id: Option<&str>,
        _t: Option<&str>,
        _ti: Option<&str>,
    ) {
    }

    /// Drop one confirmed rate push (exact-op match, so a stale
    /// in-flight task cannot clear a newer intent).
    #[cfg(feature = "innertube")]
    fn clear_pending_rate(&self, op: &str, track_id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.clear_pending(op, None, Some(track_id));
    }
}

/// One ranked search hit across result types. The unified search
/// list mixes songs, albums, artists and playlists, so each row carries
/// its own value (no per-type sections anymore).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchHit<'a> {
    Song(&'a Track),
    Album(&'a Album),
    Artist(&'a Artist),
    Playlist(&'a PlaylistHit),
}

/// One search round trip: the server-ordered All response plus the
/// filtered per-type depth behind the Songs/Albums/Artists/Playlists
/// tabs. The cache stores the bundle; keystroke narrowing filters both
/// halves (order-preserving, so server rank survives typing). Boxed at
/// the `SearchCompleted` edge: nine vecs would bloat the `Message` enum.
#[derive(Debug, Clone, Default)]
pub struct SearchBundle {
    pub all: SearchAll,
    pub filtered: SearchResults,
}

/// True when the unfiltered response holds nothing renderable.
fn all_empty(all: &SearchAll) -> bool {
    all.tracks.is_empty()
        && all.artists.is_empty()
        && all.albums.is_empty()
        && all.playlists.is_empty()
}

/// Lowercase with edge punctuation trimmed, so "Chop Suey!" matches
/// a "Chop Suey" top hit when resolving the hero play target.
fn norm_title(s: &str) -> String {
    s.to_lowercase()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// Hero for the unified search list: the server's top hit resolved by
/// id into the shelves (the card tap target is directly actionable),
/// falling back to a title match and then the first row. The shelves
/// below supply the play button.
fn top_hero<'a>(all: &'a SearchAll) -> Option<SearchHit<'a>> {
    let by_id = all.top.as_ref().and_then(|top| match top.kind {
        TopKind::Song | TopKind::Video => all
            .tracks
            .iter()
            .find(|t| t.id == top.id)
            .map(SearchHit::Song),
        TopKind::Artist => all
            .artists
            .iter()
            .find(|a| a.id == top.id)
            .map(SearchHit::Artist),
        TopKind::Album => all
            .albums
            .iter()
            .find(|a| a.id == top.id)
            .map(SearchHit::Album),
        TopKind::Playlist => all
            .playlists
            .iter()
            .find(|p| p.id == top.id)
            .map(SearchHit::Playlist),
        TopKind::Other => None,
    });
    if by_id.is_some() {
        return by_id;
    }
    let matched = all.top.as_ref().and_then(|top| {
        let title = norm_title(&top.title);
        let artist = norm_title(&top.artist);
        match top.kind {
            TopKind::Song | TopKind::Video => all
                .tracks
                .iter()
                .find(|t| {
                    norm_title(&t.title) == title
                        && (artist.is_empty() || norm_title(&t.artist) == artist)
                })
                .map(SearchHit::Song),
            TopKind::Artist => all
                .artists
                .iter()
                .find(|a| norm_title(&a.name) == title || norm_title(&a.name) == artist)
                .map(SearchHit::Artist),
            TopKind::Album => all
                .albums
                .iter()
                .find(|a| norm_title(&a.title) == title)
                .map(SearchHit::Album),
            TopKind::Playlist => all
                .playlists
                .iter()
                .find(|p| norm_title(&p.title) == title)
                .map(SearchHit::Playlist),
            TopKind::Other => None,
        }
    });
    matched.or_else(|| {
        all.tracks
            .first()
            .map(SearchHit::Song)
            .or_else(|| all.artists.first().map(SearchHit::Artist))
            .or_else(|| all.albums.first().map(SearchHit::Album))
            .or_else(|| all.playlists.first().map(SearchHit::Playlist))
    })
}

/// Row order for the unified search list: the exact web mixed order
/// (`mixed` refs into the typed vecs), hero excluded and capped for
/// page speed. Narrowed-away refs are skipped. No client re-sort, so
/// official hits stay where the server put them.
fn order_all_hits<'a>(all: &'a SearchAll) -> Vec<SearchHit<'a>> {
    let hero_id: Option<&str> = match top_hero(all) {
        Some(SearchHit::Song(t)) => Some(t.id.as_str()),
        Some(SearchHit::Album(a)) => Some(a.id.as_str()),
        Some(SearchHit::Artist(a)) => Some(a.id.as_str()),
        Some(SearchHit::Playlist(p)) => Some(p.id.as_str()),
        None => None,
    };
    let mut rows: Vec<SearchHit<'a>> = Vec::new();
    for m in &all.mixed {
        let hit = match m.kind {
            0 => all
                .tracks
                .iter()
                .find(|t| t.id == m.id)
                .map(SearchHit::Song),
            1 => all
                .artists
                .iter()
                .find(|a| a.id == m.id)
                .map(SearchHit::Artist),
            2 => all
                .albums
                .iter()
                .find(|a| a.id == m.id)
                .map(SearchHit::Album),
            3 => all
                .playlists
                .iter()
                .find(|p| p.id == m.id)
                .map(SearchHit::Playlist),
            _ => None,
        };
        if let Some(hit) = hit {
            let id = match hit {
                SearchHit::Song(t) => t.id.as_str(),
                SearchHit::Album(a) => a.id.as_str(),
                SearchHit::Artist(a) => a.id.as_str(),
                SearchHit::Playlist(p) => p.id.as_str(),
            };
            if hero_id != Some(id) {
                rows.push(hit);
            }
        }
        if rows.len() >= MIXED_SEARCH_CAP {
            break;
        }
    }
    rows
}

/// True when every result list is empty (drives the search page
/// loading/empty states, never per-type placeholders).
fn results_empty(results: &SearchResults) -> bool {
    results.tracks.is_empty()
        && results.albums.is_empty()
        && results.artists.is_empty()
        && results.playlists.is_empty()
}

/// Order-preserving substring narrow of an unfiltered response (the
/// keystroke path for the All tab): same predicates as
/// `filter_results`, server order untouched. The top hit survives only
/// while it still matches the narrower query.
fn filter_all(base: &SearchAll, query: &str) -> SearchAll {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return SearchAll::default();
    }
    SearchAll {
        top: base.top.clone().filter(|t| {
            t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q)
        }),
        mixed: base.mixed.clone(),
        tracks: base
            .tracks
            .iter()
            .filter(|t| t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        artists: base
            .artists
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        albums: base
            .albums
            .iter()
            .filter(|a| a.title.to_lowercase().contains(&q) || a.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        playlists: base
            .playlists
            .iter()
            .filter(|p| p.title.to_lowercase().contains(&q) || p.author.to_lowercase().contains(&q))
            .cloned()
            .collect(),
    }
}

/// Case-insensitive substring filter over a full search result set.
fn filter_results(base: &SearchResults, query: &str) -> SearchResults {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return SearchResults::default();
    }
    SearchResults {
        tracks: base
            .tracks
            .iter()
            .filter(|t| t.title.to_lowercase().contains(&q) || t.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        albums: base
            .albums
            .iter()
            .filter(|a| a.title.to_lowercase().contains(&q) || a.artist.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        artists: base
            .artists
            .iter()
            .filter(|a| a.name.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        playlists: base
            .playlists
            .iter()
            .filter(|p| p.title.to_lowercase().contains(&q) || p.author.to_lowercase().contains(&q))
            .cloned()
            .collect(),
    }
}

/// Offline builds: only the yt-dlp path exists; without the binary installed
/// there is nothing else to try.
#[cfg(not(feature = "innertube"))]
async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    Err(format!(
        "yt-dlp not found and this is an offline build ({video_id}). Install yt-dlp or rebuild with `--features innertube`."
    ))
}

/// yt-dlp `--get-url` only (no download): the fast path behind instant
/// playback. Failures name the track; the caller decides bytes-vs-stream.
async fn resolve_stream_url(video_id: &str) -> Result<String, String> {
    let bin = yutu_core::find_yt_dlp()
        .ok_or_else(|| "yt-dlp not installed: install it to play music.".to_string())?;
    yutu_core::resolve_audio_url(&bin, video_id).map_err(|e| e.to_string())
}

/// Real search over the live Innertube API: the web-style unfiltered
/// search (server relevance order for the All tab) plus one raw
/// filtered search per type (depth for the filter tabs, same `params`
/// the web client sends). Five round trips, each failing open; only a
/// total failure errors. Uses the signed-in session when one exists
/// (the server then ranks like the logged-in web client, from the
/// account's own activity), else anonymous.
#[cfg(feature = "innertube")]
async fn live_search(query: &str) -> Result<Box<SearchBundle>, String> {
    use yutu_core::{raw_search_all, raw_search_filtered, SearchFilterKind};
    let auth = AppState::session_search_auth();
    let (all, songs, albums, artists, playlists) = tokio::join!(
        raw_search_all(query, &auth),
        raw_search_filtered(query, &auth, SearchFilterKind::Songs),
        raw_search_filtered(query, &auth, SearchFilterKind::Albums),
        raw_search_filtered(query, &auth, SearchFilterKind::Artists),
        raw_search_filtered(query, &auth, SearchFilterKind::Playlists),
    );
    if let (Err(e), Err(_), Err(_), Err(_), Err(_)) = (&all, &songs, &albums, &artists, &playlists)
    {
        return Err(e.clone());
    }
    let all = all.unwrap_or_default();
    let filtered = SearchResults {
        tracks: songs.map(|s| s.tracks).unwrap_or_default(),
        albums: albums.map(|s| s.albums).unwrap_or_default(),
        artists: artists.map(|s| s.artists).unwrap_or_default(),
        playlists: playlists.map(|s| s.playlists).unwrap_or_default(),
    };
    // Either half backs the other: a failed raw response still leaves
    // server-ordered filtered shelves for All, and failed filtered
    // queries fall back to the raw shelves for the tabs.
    let all = if all_empty(&all) {
        all_from_filtered(&filtered)
    } else {
        all
    };
    let filtered = SearchResults {
        tracks: if filtered.tracks.is_empty() {
            all.tracks.clone()
        } else {
            filtered.tracks
        },
        albums: if filtered.albums.is_empty() {
            all.albums.clone()
        } else {
            filtered.albums
        },
        artists: if filtered.artists.is_empty() {
            all.artists.clone()
        } else {
            filtered.artists
        },
        playlists: if filtered.playlists.is_empty() {
            all.playlists.clone()
        } else {
            filtered.playlists
        },
    };
    Ok(Box::new(SearchBundle { all, filtered }))
}

/// All response derived from filtered shelves (raw failure fallback):
/// per-type server order concatenated, no top hit.
#[cfg(feature = "innertube")]
fn all_from_filtered(filtered: &SearchResults) -> SearchAll {
    use yutu_core::MixedRef;
    let mut mixed = Vec::new();
    for t in &filtered.tracks {
        mixed.push(MixedRef {
            kind: 0,
            id: t.id.clone(),
        });
    }
    for a in &filtered.artists {
        mixed.push(MixedRef {
            kind: 1,
            id: a.id.clone(),
        });
    }
    for a in &filtered.albums {
        mixed.push(MixedRef {
            kind: 2,
            id: a.id.clone(),
        });
    }
    for p in &filtered.playlists {
        mixed.push(MixedRef {
            kind: 3,
            id: p.id.clone(),
        });
    }
    SearchAll {
        top: None,
        tracks: filtered.tracks.clone(),
        artists: filtered.artists.clone(),
        albums: filtered.albums.clone(),
        playlists: filtered.playlists.clone(),
        mixed,
    }
}

/// Fetch one search-result playlist (community or featured) into playable
/// tracks: the click path behind playlist heroes and rows.
#[cfg(feature = "innertube")]
async fn live_playlist_tracks(id: &str) -> Result<Vec<Track>, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    backend.playlist_tracks(id).await
}

/// Radio sequence for a seed: the seed first, same-artist tracks next,
/// everything else after.
#[cfg(feature = "innertube")]
async fn live_radio(seed: &str, artist: &str) -> Result<Vec<Track>, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    let found = backend.search_tracks(artist).await?;
    let mut seq: Vec<Track> = Vec::with_capacity(found.len());
    if let Some(t) = found.iter().find(|t| t.id == seed).cloned() {
        seq.push(t);
    }
    for t in &found {
        if t.id != seed && t.artist == artist && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    for t in &found {
        if t.id != seed && !seq.iter().any(|x| x.id == t.id) {
            seq.push(t.clone());
        }
    }
    Ok(seq)
}

/// Hydrate one bare id (liked/history rows from installs that only
/// stored ids, or catalog rows missing art) into a full Track: a
/// watch-playlist probe whose panel contains the seed, keeping only the
/// exact-id match (neighbours stay out so a small lazy batch never
/// explodes into 100+ rows of DB writes + art).
#[cfg(feature = "innertube")]
async fn hydrate_one(id: String) -> Vec<Track> {
    use yutu_core::InnertubeBackend;
    let Ok(backend) = InnertubeBackend::connect_anonymous().await else {
        return Vec::new();
    };
    match backend.recommendations_for(&id).await {
        Ok(tracks) => tracks.into_iter().filter(|t| t.id == id).collect(),
        Err(e) => {
            eprintln!("[yutu-hydrate] {id}: {e}");
            Vec::new()
        }
    }
}

/// Offline builds cannot hydrate (no Innertube): the SQLite track cache
/// is the only source, so callers just get nothing new.
#[cfg(not(feature = "innertube"))]
async fn hydrate_one(_id: String) -> Vec<Track> {
    Vec::new()
}

/// Resolve a video ID to bytes. yt-dlp goes first (maintained decipher, no
/// feature gate, license-clean); then the raw WEB_REMIX probes with sidecar
/// mint + retry on PO-gating. A bare-URL 403 retries once with a freshly
/// resolved URL (Google edges transiently reject single URL instances),
/// then falls back to a direct sidecar download (`-o -`, whose own HTTP
/// stack handles the headers/retries a bare GET lacks).
async fn resolve_and_fetch(video_id: &str) -> Result<Vec<u8>, String> {
    if let Some(bin) = yutu_core::find_yt_dlp() {
        match yutu_core::resolve_audio_url(&bin, video_id) {
            Ok(url) => match yutu_player::fetch_bytes(&url).await {
                Ok(bytes) => return Ok(bytes),
                Err(first) => {
                    let msg = first.to_string();
                    if msg.contains("403") {
                        eprintln!("[yutu-stream] chunked fetch 403 ({msg}); re-resolving once");
                        if let Ok(fresh) = yutu_core::resolve_audio_url(&bin, video_id) {
                            if let Ok(bytes) = yutu_player::fetch_bytes(&fresh).await {
                                return Ok(bytes);
                            }
                        }
                        eprintln!("[yutu-stream] fresh URL also failed; yt-dlp direct download");
                        match yutu_core::download_audio_bytes(&bin, video_id) {
                            Ok(bytes) => return Ok(bytes),
                            Err(e) => {
                                eprintln!(
                                    "[yutu-stream] yt-dlp download failed ({e}); trying raw probes"
                                );
                            }
                        }
                    } else {
                        eprintln!("[yutu-stream] yt-dlp path failed ({msg}); trying raw probes");
                    }
                }
            },
            Err(e) => {
                eprintln!("[yutu-stream] yt-dlp path failed ({e}); trying raw probes");
            }
        }
    }
    resolve_via_raw(video_id).await
}

/// Raw-probe path (needs the `innertube` feature): anonymous connect,
/// fallback chain, sidecar mint + `poToken` retry on PO-gating.
#[cfg(feature = "innertube")]
async fn resolve_via_raw(video_id: &str) -> Result<Vec<u8>, String> {
    use yutu_core::{InnertubeBackend, StreamError, StreamRequest};
    let backend = InnertubeBackend::connect_anonymous().await?;
    let mut req = StreamRequest::new(video_id);
    match backend.resolve_stream(&mut req).await {
        Ok(url) => yutu_player::fetch_bytes(&url)
            .await
            .map_err(|e| e.to_string()),
        Err(StreamError::PoTokenRequired { .. }) => {
            let Some(bin) = yutu_core::find_botguard_bin() else {
                return Err("PO token required and rustypipe-botguard is not installed.".into());
            };
            let mut retry = StreamRequest::new(video_id);
            let resolved = backend
                .resolve_stream_with_sidecar(&mut retry, &bin)
                .await
                .map_err(|e| format!("{e}: {}", e.hint()))?;
            match yutu_player::fetch_bytes(&resolved.url).await {
                Ok(bytes) => Ok(bytes),
                Err(first) => match resolved.pot_token {
                    Some(pot) => {
                        let sep = if resolved.url.contains('?') { '&' } else { '?' };
                        let url = format!("{}{}pot={}", resolved.url, sep, pot);
                        yutu_player::fetch_bytes(&url)
                            .await
                            .map_err(|e| e.to_string())
                    }
                    None => Err(first.to_string()),
                },
            }
        }
        Err(e) => Err(format!("{e}: {}", e.hint())),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// XDG data dir without extra deps: $XDG_DATA_HOME or ~/.local/share.
pub fn data_dir() -> String {
    let base = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        format!("{home}/.local/share")
    });
    format!("{base}/yutu-mux")
}

pub fn default_library_path() -> String {
    format!("{}/library.db", data_dir())
}

/// Disk cache dir for offline audio bytes (`*.m4a` per track).
pub fn offline_cache_dir() -> String {
    format!("{}/offline_cache", data_dir())
}

/// Disk dir for downloaded lyrics (`{sanitized-id}.json` per track).
pub fn lyrics_dir() -> String {
    format!("{}/lyrics", data_dir())
}

/// Disk dir for downloaded artwork (`{url-hash}.bin` per image URL).
pub fn artwork_dir() -> String {
    format!("{}/artwork", data_dir())
}

/// Filename-safe stem: ASCII alphanumerics kept, all else `_`.
/// Same rule as the audio cache so ids map 1:1 with their `.m4a`.
fn sanitize_filename(s: &str) -> String {
    if s.is_empty() {
        return "unknown".to_string();
    }
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Lyrics file for `track_id` inside `dir` (caller passes [`lyrics_dir`]
/// in prod, a temp dir in tests).
fn lyrics_file_in(dir: &str, track_id: &str) -> String {
    format!("{}/{}.json", dir, sanitize_filename(track_id))
}

/// Persisted lyrics path for `track_id`.
pub fn lyrics_file_for(track_id: &str) -> String {
    lyrics_file_in(&lyrics_dir(), track_id)
}

/// Artwork stem for `url` inside `dir`: hex of the URL hash, so no URL
/// characters ever reach the filesystem. The extension is sniffed from
/// the bytes at save time (see [`artwork_ext`]).
fn artwork_stem_in(dir: &str, url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{}/{:016x}", dir, hasher.finish())
}

/// Extension sniffed from magic bytes. The image loader (`image::open`
/// behind iced file handles) keys the decoder off the file extension,
/// so extensionless files never render: every saved file carries one.
/// YTM thumbs are JPEG; unknown bytes default there too.
fn artwork_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.len() >= 12 && bytes[0..4] == *b"RIFF" && bytes[8..12] == *b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else {
        "jpg"
    }
}

/// Candidate extensions for a saved artwork stem, in lookup order.
const ARTWORK_EXTS: [&str; 4] = ["jpg", "png", "webp", "gif"];

/// Saved artwork path for `url`, if any: tries the sniffed-extension
/// names, then migrates a legacy extensionless `.bin` file (sniff +
/// rename, best-effort) so early downloads regain their covers.
fn artwork_path_in(dir: &str, url: &str) -> Option<String> {
    let stem = artwork_stem_in(dir, url);
    for ext in ARTWORK_EXTS {
        let path = format!("{stem}.{ext}");
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }
    let legacy = format!("{stem}.bin");
    if std::path::Path::new(&legacy).exists() {
        if let Ok(bytes) = std::fs::read(&legacy) {
            let migrated = format!("{stem}.{}", artwork_ext(&bytes));
            if std::fs::rename(&legacy, &migrated).is_ok() {
                return Some(migrated);
            }
        }
        // Rename failed (or bytes unreadable): leave the legacy file;
        // nothing decodes it, so report absence.
    }
    None
}

/// Persisted artwork path for `url`, if downloaded.
pub fn artwork_path_for(url: &str) -> Option<String> {
    artwork_path_in(&artwork_dir(), url)
}

/// True when the artwork bytes for `url` are already on disk.
fn artwork_file_exists(url: &str) -> bool {
    artwork_path_for(url).is_some()
}

/// Best-effort persist of fetched lyrics (missing dir is created;
/// failures are ignored: the song stays playable without them).
fn save_lyrics(track_id: &str, lyrics: &yutu_core::LyricsResult) {
    save_lyrics_in(&lyrics_dir(), track_id, lyrics);
}

/// [`save_lyrics`] into an explicit dir (tests pass a temp dir).
fn save_lyrics_in(dir: &str, track_id: &str, lyrics: &yutu_core::LyricsResult) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(lyrics) {
        let _ = std::fs::write(lyrics_file_in(dir, track_id), json);
    }
}

/// Lyrics previously saved by [`save_lyrics`] (or by a download bundle).
/// `None` when never fetched: the caller fires the network task.
#[cfg(feature = "lyrics")]
fn load_lyrics(track_id: &str) -> Option<yutu_core::LyricsResult> {
    load_lyrics_in(&lyrics_dir(), track_id)
}

/// [`load_lyrics`] from an explicit dir (tests pass a temp dir).
#[cfg(feature = "lyrics")]
fn load_lyrics_in(dir: &str, track_id: &str) -> Option<yutu_core::LyricsResult> {
    let bytes = std::fs::read(lyrics_file_in(dir, track_id)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Best-effort persist of fetched artwork bytes (failures ignored).
fn save_artwork(url: &str, bytes: &[u8]) {
    save_artwork_in(&artwork_dir(), url, bytes);
}

/// [`save_artwork`] into an explicit dir (tests pass a temp dir).
fn save_artwork_in(dir: &str, url: &str, bytes: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = format!("{}.{}", artwork_stem_in(dir, url), artwork_ext(bytes));
    let _ = std::fs::write(path, bytes);
}

/// Audio bytes previously downloaded for `track_id` inside `dir`.
/// `None` when the track was never downloaded (or the file is gone).
/// Reads the file directly (no tracker state): the in-memory tracker is
/// always fresh after a restart, but the files persist.
fn offline_bytes_in(dir: &str, track_id: &str) -> Option<Vec<u8>> {
    let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
    cache.set_dir(dir.to_string());
    let path = cache.file_path_for(track_id)?;
    std::fs::read(path).ok()
}

/// Saved Google OAuth token bundle (0600, written by the login flow).
#[cfg(feature = "innertube")]
pub fn oauth_path() -> String {
    format!("{}/youtube_oauth.json", data_dir())
}

/// Saved cookie session: the normalised `Cookie` header (0600, written
/// on every successful cookie import). Restarts reuse it, so the account
/// stays signed in without another export.
#[cfg(feature = "innertube")]
pub fn cookie_session_path() -> String {
    format!("{}/cookie_session.txt", data_dir())
}

#[cfg(feature = "innertube")]
fn cookie_session_exists() -> bool {
    std::path::Path::new(&cookie_session_path()).exists()
}

#[cfg(feature = "innertube")]
fn oauth_path_exists() -> bool {
    #[cfg(feature = "innertube")]
    {
        std::path::Path::new(&oauth_path()).exists()
    }
    #[cfg(not(feature = "innertube"))]
    {
        false
    }
}

/// Sync prefs from SQLite: auto-sync toggle (default on) plus the last
/// completed pull, so the Settings label survives restarts.
#[cfg(feature = "innertube")]
fn load_sync_prefs(path: &str) -> (bool, Option<i64>) {
    let Ok(store) = LibraryStore::open(path) else {
        return (true, None);
    };
    let auto_sync = store.get_pref("auto_sync").unwrap_or(None).as_deref() != Some("0");
    let last_sync_unix = store
        .get_pref("last_sync_unix")
        .unwrap_or(None)
        .and_then(|v| v.parse().ok());
    (auto_sync, last_sync_unix)
}

#[allow(clippy::type_complexity)]
fn load_persisted(
    path: &str,
) -> (
    HashSet<String>,
    Vec<String>,
    HashSet<String>,
    Vec<String>,
    Vec<String>,
    Vec<Playlist>,
    u64,
    Vec<Track>,
    HashSet<String>,
    Vec<String>,
) {
    let empty = (
        HashSet::new(),
        Vec::new(),
        HashSet::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        0,
        Vec::new(),
        HashSet::new(),
        Vec::new(),
    );
    let store = match LibraryStore::open(path) {
        Ok(s) => s,
        Err(_) => return empty,
    };
    // Likes: every id in SQLite in add order, no catalog probe.
    let order: Vec<String> = store.liked_ids().unwrap_or_default();
    let liked: HashSet<String> = order.iter().cloned().collect();
    // Dislikes ("don't recommend"): same contract as likes.
    let disliked_order: Vec<String> = store.disliked_ids().unwrap_or_default();
    let disliked: HashSet<String> = disliked_order.iter().cloned().collect();
    let history = store
        .history(50)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.track_id)
        .collect();
    // One-time migration: older imports stored browse (`VL...`) ids while
    // creates store edit (`PL...`) ids. Canonicalise to edit form so the
    // same list never appears twice and writes always send the edit form
    // (a `VL...` id to `browse/edit_playlist` 400s with invalid argument).
    // Runs only while stale browse-form rows exist.
    let mut user_playlists = store.user_playlists().unwrap_or_default();
    if user_playlists.iter().any(|p| p.id.starts_with("VL")) {
        let mut merged: Vec<Playlist> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        for p in user_playlists.drain(..) {
            if p.id.starts_with("VL") {
                stale.push(p.id.clone());
            }
            let id = yutu_core::canonical_playlist_id(&p.id);
            match merged.iter_mut().find(|m| m.id == id) {
                Some(m) => {
                    for t in p.tracks {
                        if !m.tracks.iter().any(|x| x.id == t.id) {
                            m.tracks.push(t);
                        }
                    }
                }
                None => merged.push(Playlist {
                    id,
                    title: p.title,
                    tracks: p.tracks,
                }),
            }
        }
        for m in &merged {
            let _ = store.sync_server_playlist(&m.id, &m.title, &m.tracks);
        }
        for old in stale {
            if old.starts_with("VL") {
                let _ = store.delete_playlist(&old);
            }
        }
        user_playlists = merged;
    }
    let seq = user_playlists
        .iter()
        .filter_map(|p| p.id.strip_prefix('u')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    // Track metadata cache: lets Favorites/history render on first
    // launch with no search (offline-friendly). Missing rows simply
    // mean an older install: the caller hydrates them from the network.
    let cached = store.all_cached_tracks().unwrap_or_default();
    // Downloaded ids in add order; prune entries whose audio file is
    // gone (manual delete, eviction, or a previous crash) so the
    // Downloaded list stays honest about what is really offline.
    let downloaded_order: Vec<String> = {
        let ids = store.downloaded_ids().unwrap_or_default();
        let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
        cache.set_dir(offline_cache_dir());
        let mut kept = Vec::with_capacity(ids.len());
        for id in ids {
            let present = cache
                .file_path_for(&id)
                .is_some_and(|p| std::path::Path::new(&p).exists());
            if present {
                kept.push(id);
            } else {
                let _ = store.unmark_downloaded(&id);
            }
        }
        kept
    };
    let downloaded: HashSet<String> = downloaded_order.iter().cloned().collect();
    (
        liked,
        order,
        disliked,
        disliked_order,
        history,
        user_playlists,
        seq,
        cached,
        downloaded,
        downloaded_order,
    )
}

impl AppState {
    pub fn new() -> (Self, iced::Task<Message>) {
        Self::with_library_path(default_library_path())
    }

    pub fn with_library_path(library_path: String) -> (Self, iced::Task<Message>) {
        let audio = match AudioOut::try_new() {
            Some(a) => {
                eprintln!("audio backend: {}", a.backend_name());
                Some(a)
            }
            None => {
                eprintln!("audio unavailable: no output device");
                None
            }
        };
        if let Some(parent) = std::path::Path::new(&library_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let (
            liked,
            liked_order,
            disliked,
            disliked_order,
            history,
            user_playlists,
            playlist_seq,
            cached,
            downloaded,
            downloaded_order,
        ) = load_persisted(&library_path);
        let mut state = Self {
            route: Route::Home,
            search_query: String::new(),
            results: SearchResults::default(),
            all_results: SearchAll::default(),
            queue: Queue::default(),
            liked,
            liked_order,
            disliked,
            disliked_order,
            downloaded,
            downloaded_order,
            history,
            volume: 0.8,
            status: "Ready: search for music.".into(),
            user_playlists,
            new_playlist_name: String::new(),
            playlist_seq,
            eq_preset: "Flat".into(),
            scrobble_enabled: false,
            scrobble_user: String::new(),
            scrobble_lb_token: String::new(),
            scrobble_fm_key: String::new(),
            scrobble_fm_secret: String::new(),
            scrobble_fm_sk: String::new(),
            paused: false,
            audio,
            position_secs: 0.0,
            loading_track: None,
            url_cache: HashMap::new(),
            prefetched: None,
            lyrics_cache: HashMap::new(),
            lyrics_loading: None,
            nav_back: Vec::new(),
            nav_forward: Vec::new(),
            right_tab: RightTab::Queue,
            pre_shuffle: None,
            pre_shuffle_cur: None,
            suggest: Vec::new(),
            suggest_gen: 0,
            show_suggest: false,
            queue_saved: None,
            queue_saved_at: None,
            pending_resume: None,
            restored_pending: false,
            context_menu: None,
            submenu_playlists_for: None,
            pl_menu: None,
            coll_menu: None,
            menu_at: None,
            cursor: None,
            window: None,
            sidebar_pinned: true,
            right_pinned: true,
            sidebar_latched: true,
            right_latched: true,
            sidebar_w: 235.0,
            right_w: 285.0,
            drawer_open: false,
            drawer_anim: 0.0,
            animations_enabled: true,
            anim_ms: ANIM_DEFAULT_MS,
            drawer_scroll: 0.0,
            scroll_hint: HashMap::new(),
            sidebar_filter: String::new(),
            autoplay_for: None,
            autoplay: Vec::new(),
            hovered_queue: None,
            hover_remove: None,
            hovered_track: None,
            hovered_album: None,
            hovered_artist: None,
            hovered_search_playlist: None,
            hovered_auto: None,
            drag: None,
            press_snap: None,
            hovered_playlist: None,
            just_reordered_at: None,
            drag_gap: 0.0,
            error_log: Vec::new(),
            seek_hold_until: None,
            paused_seek: false,
            last_tick: None,
            cookie_path: String::new(),
            playing_from: String::new(),
            artwork: HashMap::new(),
            artwork_loading: HashSet::new(),
            artwork_failed: HashMap::new(),
            hydrate_cooldown: HashMap::new(),
            // Ids with a metadata probe in flight (dedups lazy batches
            // across navigations and ticks: no repeat fetch while pending).
            hydrating: HashSet::new(),
            // First Tick after launch runs the Home lazy batch (startup
            // itself does zero network so the window appears instantly).
            lazy_init_done: false,
            prefetch_attempted: None,
            #[cfg(feature = "innertube")]
            google_login: None,
            #[cfg(feature = "innertube")]
            logged_in: oauth_path_exists(),
            #[cfg(feature = "innertube")]
            cookie_logged_in: cookie_session_exists(),
            #[cfg(feature = "innertube")]
            login_busy: false,
            #[cfg(feature = "innertube")]
            import_songs: 0,
            #[cfg(feature = "innertube")]
            import_likes: 0,
            #[cfg(feature = "innertube")]
            import_done: 0,
            #[cfg(feature = "innertube")]
            import_total: 0,
            #[cfg(feature = "innertube")]
            import_gen: 0,
            #[cfg(feature = "innertube")]
            import_active: false,
            #[cfg(feature = "innertube")]
            import_expected: 0,
            #[cfg(feature = "innertube")]
            import_live: HashMap::new(),
            #[cfg(feature = "innertube")]
            import_finished: HashSet::new(),
            #[cfg(feature = "innertube")]
            import_shown: 0,
            #[cfg(feature = "innertube")]
            auto_sync: true,
            #[cfg(feature = "innertube")]
            last_sync: None,
            #[cfg(feature = "innertube")]
            last_sync_unix: None,
            #[cfg(feature = "innertube")]
            started_at: std::time::Instant::now(),
            end_advanced: false,
            library_path,
            catalog: HashMap::new(),
            search_base: SearchResults::default(),
            search_all_base: SearchAll::default(),
            search_cache: HashMap::new(),
            searching: false,
            search_gen: 0,
            search_filter: SearchFilter::All,
            radio_tracks: Vec::new(),
            mpris: None,
        };
        // Sync prefs: auto-sync toggle + last-sync label survive restarts.
        #[cfg(feature = "innertube")]
        {
            let (auto_sync, last_sync_unix) = load_sync_prefs(&state.library_path);
            state.auto_sync = auto_sync;
            state.last_sync_unix = last_sync_unix;
        }
        // Seed the in-memory catalog from the track cache plus
        // persisted playlists so Favorites/history/playlist songs render
        // on first launch with no search. Memory-only (no SQLite writes,
        // no network, no artwork fetches): the window appears instantly
        // with cached titles plus disk-cached covers, then the first Tick
        // lazily hydrates only the Home screenful (see ensure_visible).
        let mut seed: Vec<Track> = cached;
        seed.extend(state.user_playlists.iter().flat_map(|p| p.tracks.clone()));
        // Merge duplicates: prefer rows with artwork so covers survive.
        {
            use std::collections::HashMap;
            let mut merged: HashMap<String, Track> = HashMap::new();
            for t in seed {
                merged
                    .entry(t.id.clone())
                    .and_modify(|e| {
                        if e.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                            e.thumbnail_url = t.thumbnail_url.clone();
                        }
                        if e.thumbnail_large_url.is_none() && t.thumbnail_large_url.is_some() {
                            e.thumbnail_large_url = t.thumbnail_large_url.clone();
                        }
                        if e.plays.is_none() && t.plays.is_some() {
                            e.plays = t.plays.clone();
                        }
                        if e.title.is_empty() && !t.title.is_empty() {
                            e.title = t.title.clone();
                            e.artist = t.artist.clone();
                            e.album = t.album.clone();
                            e.duration_secs = t.duration_secs;
                        }
                    })
                    .or_insert(t);
            }
            seed = merged.into_values().collect();
        }
        state.seed_catalog_mem(seed);
        // Session restore last: the catalog is warm, so saved ids
        // resolve to full rows with art.
        state.restore_queue();
        (state, iced::Task::none())
    }

    /// Browser-style navigation: records back history, clears forward.
    pub fn navigate(&mut self, route: Route) {
        self.go(route);
    }

    fn go(&mut self, route: Route) {
        // Navigation always drops the drawer (it glides shut) and the
        // search popup (results take the page).
        self.drawer_open = false;
        self.show_suggest = false;
        if route != self.route {
            self.nav_back.push(self.route.clone());
            if self.nav_back.len() > 50 {
                self.nav_back.remove(0);
            }
            self.nav_forward.clear();
        }
        self.route = route;
    }

    fn go_back(&mut self) {
        self.drawer_open = false;
        self.show_suggest = false;
        if let Some(prev) = self.nav_back.pop() {
            self.nav_forward.push(self.route.clone());
            self.route = prev;
        }
    }

    fn go_forward(&mut self) {
        self.drawer_open = false;
        self.show_suggest = false;
        if let Some(next) = self.nav_forward.pop() {
            self.nav_back.push(self.route.clone());
            self.route = next;
        }
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let keys = iced::keyboard::listen().filter_map(|event| match event {
            iced::keyboard::Event::KeyPressed { key, .. } => match key.as_ref() {
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Space) => {
                    Some(Message::TogglePause)
                }
                iced::keyboard::Key::Character(" ") => Some(Message::TogglePause),
                iced::keyboard::Key::Character("/") => Some(Message::Navigate(Route::Search)),
                iced::keyboard::Key::Character("n") => Some(Message::Next),
                iced::keyboard::Key::Character("p") => Some(Message::Prev),
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => {
                    Some(Message::CloseContext)
                }
                _ => None,
            },
            _ => None,
        });
        let tick = iced::time::every(std::time::Duration::from_millis(500)).map(Message::Tick);
        let anim = if self.animating() {
            iced::time::every(std::time::Duration::from_millis(ANIM_TICK_MS))
                .map(|_| Message::Animate)
        } else {
            iced::Subscription::none()
        };
        let events = iced::event::listen().filter_map(|event| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Window(iced::window::Event::Resized(size)) => {
                Some(Message::WindowResized(size))
            }
            // Mouse side buttons navigate like a browser (press, not
            // release, for browser feel and no drag-commit collision).
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Back)) => {
                Some(Message::NavBack)
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Forward)) => {
                Some(Message::NavForward)
            }
            // Left release only owns the drag: side/right releases must
            // never commit or abort a reorder in progress.
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::DragEnd)
            }
            // Left press anywhere: the handler arms a queue drag only
            // when the cursor sits on a queue row.
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                Some(Message::GlobalPress)
            }
            _ => None,
        });
        iced::Subscription::batch([keys, tick, events, anim])
    }

    /// Repeat mode as stored in the queue snapshot.
    fn repeat_label(mode: yutu_player::RepeatMode) -> String {
        match mode {
            yutu_player::RepeatMode::Off => "off".into(),
            yutu_player::RepeatMode::All => "all".into(),
            yutu_player::RepeatMode::One => "one".into(),
        }
    }

    /// Parse a stored repeat mode (unknown falls back to Off).
    fn repeat_parse(s: &str) -> yutu_player::RepeatMode {
        match s {
            "all" => yutu_player::RepeatMode::All,
            "one" => yutu_player::RepeatMode::One,
            _ => yutu_player::RepeatMode::Off,
        }
    }

    /// Current playback session as a snapshot (queue ids in order plus
    /// index, position, shuffle/repeat/volume).
    fn queue_snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            ids: self.queue.items().iter().map(|t| t.id.clone()).collect(),
            index: self.queue.position(),
            position_secs: self.position_secs,
            shuffle: self.queue.shuffle,
            repeat: Self::repeat_label(self.queue.repeat),
            volume: self.volume,
        }
    }

    /// Write the session snapshot (or clear it when the queue is empty).
    /// Ids only, one shot: restarts resolve rows via the track cache.
    fn persist_queue(&mut self) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if self.queue.is_empty() {
            let _ = store.clear_queue();
            self.queue_saved = None;
        } else {
            let snap = self.queue_snapshot();
            if store.save_queue(&snap).is_ok() {
                self.queue_saved = Some(snap);
                self.queue_saved_at = Some(std::time::Instant::now());
            }
        }
    }

    /// Tick saver: queue identity changes (ids, index, shuffle, repeat,
    /// volume) persist on the next tick; the playing position refreshes
    /// at most every 30s so Ticks stay write-free while paused.
    fn maybe_persist_queue(&mut self, now: std::time::Instant) {
        let snap = self.queue_snapshot();
        let identity_changed = self.queue_saved.as_ref().is_none_or(|s| {
            s.ids != snap.ids
                || s.index != snap.index
                || s.shuffle != snap.shuffle
                || s.repeat != snap.repeat
                || (s.volume - snap.volume).abs() > f32::EPSILON
        });
        if identity_changed {
            self.persist_queue();
            return;
        }
        let position_stale = (self
            .queue_saved
            .as_ref()
            .map(|s| s.position_secs)
            .unwrap_or(0.0)
            - snap.position_secs)
            .abs()
            > 5.0;
        let due = self
            .queue_saved_at
            .is_none_or(|at| now.duration_since(at).as_secs() >= 30);
        if !self.queue.is_empty() && !self.paused && position_stale && due {
            self.persist_queue();
        }
    }

    /// Rebuild the queue from the persisted snapshot (unknown ids drop:
    /// the track was never cached). Starts paused at the saved position
    /// with shuffle/repeat/volume restored.
    fn restore_queue(&mut self) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let snap = match store.load_queue() {
            Ok(Some(snap)) => snap,
            _ => return,
        };
        let items: Vec<Track> = snap
            .ids
            .iter()
            .filter_map(|id| self.find_track(id))
            .collect();
        if items.is_empty() {
            return;
        }
        let dropped = snap.ids.len() - items.len();
        self.queue.restore(items, snap.index);
        self.queue.shuffle = snap.shuffle;
        self.queue.repeat = Self::repeat_parse(&snap.repeat);
        self.volume = snap.volume;
        self.apply_volume();
        self.position_secs = snap.position_secs;
        self.paused = true;
        self.playing_from = "Saved queue".into();
        self.queue_saved = Some(snap);
        self.queue_saved_at = Some(std::time::Instant::now());
        // The next play loads the saved track (never resumes an empty
        // backend); any other load clears the flag instead.
        self.restored_pending = true;
        self.status = if dropped == 0 {
            format!("Restored queue ({} songs).", self.queue.len())
        } else {
            format!(
                "Restored queue ({} songs, {dropped} not cached).",
                self.queue.len()
            )
        };
    }

    fn persist_like(&self, id: &str, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if liked {
            // Cache the full row when we know it: Favorites then survives
            // restarts with title/art, not just a bare id that needs a
            // search to become visible again.
            if let Some(t) = self.catalog.get(id) {
                let _ = store.like_track(t);
            } else {
                let _ = store.like(id);
            }
        } else {
            let _ = store.unlike(id);
        }
    }

    /// Like with an explicit track (callers that already hold the full
    /// row): guarantees the metadata cache is warmed even if the catalog
    /// has not registered it yet.
    fn persist_like_track(&self, track: &Track, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = if liked {
            store.like_track(track)
        } else {
            store.unlike(&track.id)
        };
    }

    fn persist_dislike(&self, id: &str, disliked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if disliked {
            let _ = store.dislike(id);
        } else {
            let _ = store.undislike(id);
        }
    }

    fn persist_downloaded(&self, id: &str, downloaded: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        if downloaded {
            // Cache the full row when we know it: Downloaded then
            // survives restarts with title/art, not just a bare id.
            if let Some(t) = self.catalog.get(id) {
                let _ = store.mark_downloaded_track(t);
            } else {
                let _ = store.mark_downloaded(id);
            }
        } else {
            let _ = store.unmark_downloaded(id);
        }
    }

    /// Download with an explicit track (callers that already hold the
    /// full row): guarantees the metadata cache is warmed even if the
    /// catalog has not registered it yet.
    fn persist_downloaded_track(&self, track: &Track, downloaded: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = if downloaded {
            store.mark_downloaded_track(track)
        } else {
            store.unmark_downloaded(&track.id)
        };
    }

    /// Best-effort delete of the whole offline bundle for `track_id`:
    /// audio bytes plus the lyrics/art sidecars (missing files are fine:
    /// the DB row is already gone). Art needs the track's URLs, so callers
    /// pass the catalog row when they have it.
    fn remove_download_bundle(track_id: &str, track: Option<&Track>) {
        Self::remove_download_bundle_in(
            &offline_cache_dir(),
            &lyrics_dir(),
            &artwork_dir(),
            track_id,
            track,
        );
    }

    /// [`remove_download_bundle`] against explicit dirs (tests pass temps).
    fn remove_download_bundle_in(
        cache_dir: &str,
        lyrics_dir: &str,
        art_dir: &str,
        track_id: &str,
        track: Option<&Track>,
    ) {
        // Delete the audio file directly by path: the in-memory tracker
        // is always fresh here (restarts, background tasks), so a
        // tracker-based removal would no-op on its empty entry set.
        let mut probe = yutu_player::OfflineCache::new(500 * 1024 * 1024);
        probe.set_dir(cache_dir.to_string());
        if let Some(path) = probe.file_path_for(track_id) {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_file(lyrics_file_in(lyrics_dir, track_id));
        if let Some(t) = track {
            let mut urls: Vec<String> = t.thumbnail_url.iter().cloned().collect();
            urls.extend(
                t.thumbnail_large_url
                    .clone()
                    .map(|u| yutu_core::hires_thumb_url(&u, 544)),
            );
            for url in urls {
                if let Some(path) = artwork_path_in(art_dir, &url) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }

    fn persist_play(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = store.record_play(id, now);
    }

    fn persist_playlist_created(&self, id: &str, title: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.create_playlist(id, title);
    }

    fn persist_playlist_deleted(&self, id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.delete_playlist(id);
    }

    fn persist_playlist_added(&self, playlist_id: &str, track: &Track) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.add_to_playlist(playlist_id, track);
        // Keep the metadata cache in step so the same song in Favorites
        // or history still renders if its playlist is later deleted.
        let _ = store.upsert_track(track);
    }

    fn persist_playlist_removed(&self, playlist_id: &str, track_id: &str) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = store.remove_from_playlist(playlist_id, track_id);
    }

    /// Click-to-play: queue the track, jump to it, and fetch real audio.
    /// The fetch task completes in `StreamResolved`; the tone is gone.
    fn enqueue(&mut self, track: Track) -> iced::Task<Message> {
        let track = self.resolve_for_play(&track);
        self.absorb_autoplay(&track.id);
        self.playing_from = "Single".into();
        self.history.push(track.id.clone());
        self.persist_play(&track.id);
        self.queue.push(track);
        self.queue.jump_to(self.queue.len().saturating_sub(1));
        self.apply_volume();
        let play = self.play_current_task();
        let just_queued = self.queue.current().cloned();
        let scrobble = just_queued
            .map(|t| self.maybe_scrobble_task(&t))
            .unwrap_or_else(iced::Task::none);
        if self.audio.is_none() {
            self.status.push_str(" (no audio device)");
            return scrobble;
        }
        iced::Task::batch([scrobble, play])
    }

    /// Hero art URL: large preferred, rewritten to 544px server-side so
    /// the 300px drawer hero stays crisp. Shared by display and fetch so
    /// the cache key never drifts.
    fn hero_art_url(t: &Track) -> Option<String> {
        t.thumbnail_large_url
            .clone()
            .or(t.thumbnail_url.clone())
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Catalog-aware hero URL: stale queue rows fall back to catalog art,
    /// same as thumb_sized, so the drawer never stays empty while the
    /// player bar shows art.
    fn hero_art_for(&self, t: &Track) -> Option<String> {
        let merged = self.catalog.get(&t.id).unwrap_or(t);
        Self::hero_art_url(merged)
    }

    /// Fetch the hero artwork unless cached/in-flight (drawer hero): the
    /// large URL first, falling back to the small one like the hero does.
    /// Catalog-aware so stale rows warm the same key they display.
    fn large_art_task(&mut self, t: &Track) -> Option<iced::Task<Message>> {
        let url = self.hero_art_for(t)?;
        if self.artwork.contains_key(&url)
            || self.artwork_loading.contains(&url)
            || artwork_file_exists(&url)
        {
            return None;
        }
        self.artwork_loading.insert(url.clone());
        let url_for_msg = url.clone();
        Some(iced::Task::perform(
            async move { yutu_player::fetch_bytes(&url).await },
            move |result| Message::ArtworkReady {
                url: url_for_msg.clone(),
                result: result.map_err(|e| e.to_string()),
            },
        ))
    }

    /// Play whatever is current (no-op on an empty queue). Fast paths first:
    /// a prefetched URL or a fresh cached URL plays instantly with no
    /// download; otherwise a resolve task runs (mpv streams the URL, rodio
    /// downloads the bytes). Sets `loading_track` so stale arrivals drop.
    fn play_current_task(&mut self) -> iced::Task<Message> {
        let Some(t) = self.queue.current().cloned() else {
            return iced::Task::none();
        };
        // Immediate feedback: silence the old song the instant a new load
        // starts (resolving takes ~1s; the old track must not play over it).
        if let Some(audio) = &self.audio {
            audio.stop();
        }
        if self.prefetched.as_ref().is_some_and(|(id, _)| id != &t.id) {
            self.prefetched = None;
        }
        self.prefetch_attempted = None;
        self.loading_track = Some(t.id.clone());
        self.end_advanced = false;
        self.paused = false;
        self.paused_seek = false;
        self.seek_hold_until = None;
        self.position_secs = 0.0;
        // A new load cancels any pending resume seek (it belongs to a
        // previous track); the resume flow re-arms it afterwards.
        self.pending_resume = None;
        self.restored_pending = false;
        // Lyrics page follows playback: swap the id in place (no back-stack
        // spam) and fetch the new song's lyrics unless cached.
        let mut follow_tasks: Vec<iced::Task<Message>> = Vec::new();
        if matches!(self.route, Route::Lyrics(_)) && self.route != Route::Lyrics(t.id.clone()) {
            self.route = Route::Lyrics(t.id.clone());
        }
        // Lyrics + hero warming on every track start, whatever the route.
        follow_tasks.extend(self.track_start_tasks(&t));
        // Prefetched URL: instant.
        if let Some((_, url)) = self.prefetched.take() {
            if self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        // Fresh cached URL: instant (back/prev/replay never re-resolve).
        if let Some((url, expires)) = self.url_cache.get(&t.id).cloned() {
            if expires > now_unix() as u64 + 60 && self.try_play_url(&url, &t) {
                return iced::Task::batch(follow_tasks);
            }
        }
        // Downloaded bytes on disk play with no network (either
        // backend): offline songs start instantly, online or off. A
        // stale row (file evicted since) falls through to resolve.
        if self.downloaded.contains(&t.id) && self.try_play_offline(&t) {
            return iced::Task::batch(follow_tasks);
        }
        self.status = format!("Loading {} · {}…", t.title, t.artist);
        let audio_task = match &self.audio {
            Some(a) if a.wants_stream() => Self::resolve_url_task(t.id),
            _ => Self::resolve_task(t.id),
        };
        follow_tasks.push(audio_task);
        iced::Task::batch(follow_tasks)
    }

    /// Best-effort instant play of a stream URL. True on success.
    fn try_play_url(&mut self, url: &str, t: &Track) -> bool {
        match &self.audio {
            Some(audio) => match audio.play_stream_url(url) {
                Ok(()) => {
                    self.loading_track = None;
                    self.status = format!("Playing: {} · {}.", t.title, t.artist);
                    self.consume_resume();
                    true
                }
                Err(e) => {
                    self.pending_resume = None;
                    let msg = format!("Playback failed for {}: {e}", t.id);
                    self.log_error(msg.clone());
                    self.status = msg;
                    false
                }
            },
            None => {
                self.pending_resume = None;
                self.status = "Stream ready, but no audio device.".into();
                false
            }
        }
    }

    /// Play already-downloaded bytes on either backend (mpv loads bytes
    /// just like rodio here: no stream URL needed). True on success.
    fn play_offline_bytes(&mut self, t: &Track, bytes: Vec<u8>) -> bool {
        match &self.audio {
            Some(audio) => match audio.play_track_bytes(bytes) {
                Ok(()) => {
                    self.loading_track = None;
                    self.paused = false;
                    self.end_advanced = false;
                    self.position_secs = 0.0;
                    self.status = format!("Playing (offline): {} · {}.", t.title, t.artist);
                    self.consume_resume();
                    true
                }
                Err(e) => {
                    let msg = format!("Playback failed for {}: {e}", t.id);
                    self.log_error(msg.clone());
                    self.status = msg;
                    false
                }
            },
            None => false,
        }
    }

    /// Play the on-disk download for `t` when present. False when the
    /// bytes are missing (caller falls through to the network) or when
    /// playback itself fails (status already reports it).
    fn try_play_offline(&mut self, t: &Track) -> bool {
        self.try_play_offline_in(&offline_cache_dir(), t)
    }

    /// [`try_play_offline`] against an explicit dir (tests pass a temp dir).
    fn try_play_offline_in(&mut self, dir: &str, t: &Track) -> bool {
        match offline_bytes_in(dir, &t.id) {
            Some(bytes) => self.play_offline_bytes(t, bytes),
            None => false,
        }
    }

    /// Flip shuffle. Turning on snapshots the order and physically shuffles
    /// the whole queue with the current track first (index 0); turning off
    /// restores the snapshot. Playback never skips: reorder retargets the
    /// current index by track id. Drags made while shuffled are absorbed by
    /// the id-restore (effectively undone on toggle-off).
    fn set_shuffle(&mut self, on: bool) {
        if self.queue.shuffle == on {
            return;
        }
        self.queue.shuffle = on;
        if !on {
            self.restore_shuffle();
            return;
        }
        self.pre_shuffle = Some(self.queue.items().iter().map(|t| t.id.clone()).collect());
        self.pre_shuffle_cur = self.queue.position().and_then(|cur| {
            self.queue.current().map(|t| {
                let rank = self.queue.items()[..cur]
                    .iter()
                    .filter(|x| x.id == t.id)
                    .count();
                (t.id.clone(), rank)
            })
        });
        let len = self.queue.len();
        if len <= 1 {
            return;
        }
        // Fisher-Yates over everything but current (xorshift, nanos seed);
        // the current track leads the new order at index 0.
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x2545F491)
            ^ 0x9E3779B97F4A7C15;
        if seed == 0 {
            seed = 0x2545F491;
        }
        match self.queue.position() {
            None => {
                // Nothing playing yet: full shuffle, index stays None.
                let mut order: Vec<usize> = (0..len).collect();
                for i in (1..order.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    order.swap(i, (seed as usize) % (i + 1));
                }
                let _ = self.queue.reorder(&order);
            }
            Some(cur) => {
                let cur = cur.min(len - 1);
                let mut others: Vec<usize> = (0..len).filter(|&i| i != cur).collect();
                for i in (1..others.len()).rev() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    others.swap(i, (seed as usize) % (i + 1));
                }
                let mut order = vec![cur];
                order.extend(others);
                let _ = self.queue.reorder(&order);
            }
        }
    }

    /// Fisher-Yates permutation of 0..len (xorshift, nanos seed).
    fn shuffled_order(len: usize) -> Vec<usize> {
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x2545F491)
            ^ 0x9E3779B97F4A7C15;
        if seed == 0 {
            seed = 0x2545F491;
        }
        let mut order: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            order.swap(i, (seed as usize) % (i + 1));
        }
        order
    }

    /// Fresh-context play with optional full shuffle: replaces the
    /// queue, snapshots the incoming order for toggle-off, shuffles the
    /// source list first (so the queued order - and its first fetch -
    /// is already shuffled, random head included), then queues. Always
    /// reshuffles, even when shuffle mode was already on.
    fn play_fresh(
        &mut self,
        tracks: Vec<Track>,
        label: &str,
        shuffle: bool,
    ) -> iced::Task<Message> {
        self.queue.clear();
        self.autoplay.clear();
        self.autoplay_for = None;
        self.drag = None;
        let mut tracks = tracks;
        if shuffle {
            self.pre_shuffle = Some(tracks.iter().map(|t| t.id.clone()).collect());
            let order = Self::shuffled_order(tracks.len());
            let mut shuffled = Vec::with_capacity(tracks.len());
            for i in order {
                shuffled.push(tracks[i].clone());
            }
            tracks = shuffled;
        }
        let task = self.queue_many(tracks, label);
        if shuffle {
            // First-occurrence seating if duplicates exist (see restore).
            self.pre_shuffle_cur = self.queue.current().map(|t| (t.id.clone(), 0));
            self.queue.shuffle = true;
        }
        self.publish_mpris();
        task
    }

    /// Restore the pre-shuffle order. Ids added while shuffled keep their
    /// relative order at the end; removed ids vanish with their rows.
    /// Duplicate ids are matched occurrence-by-occurrence (used-index set)
    /// so the result is always a valid permutation. The current track
    /// keeps playing: an unchanged current re-seats its snapshot
    /// occurrence, a moved current re-seats its restore-time occurrence
    /// (`reorder` alone retargets duplicates by first match). Pure index
    /// moves: playback never restarts.
    fn restore_shuffle(&mut self) {
        let Some(snap) = self.pre_shuffle.take() else {
            return;
        };
        let snap_cur = self.pre_shuffle_cur.take();
        let cur_fix: Option<(String, usize)> = self.queue.position().and_then(|cur| {
            self.queue.current().map(|t| {
                let rank = self.queue.items()[..cur]
                    .iter()
                    .filter(|x| x.id == t.id)
                    .count();
                // Unchanged since snapshot: the snapshot occurrence is
                // the track the user was on, not this first match.
                if snap_cur.as_ref().is_some_and(|(id, _)| id == &t.id) {
                    snap_cur.clone().expect("checked")
                } else {
                    (t.id.clone(), rank)
                }
            })
        });
        let items = self.queue.items();
        let mut used = vec![false; items.len()];
        let mut order: Vec<usize> = Vec::with_capacity(items.len());
        for id in &snap {
            if let Some(i) = items
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .find(|(_, t)| &t.id == id)
                .map(|(i, _)| i)
            {
                used[i] = true;
                order.push(i);
            }
        }
        for (i, _) in items.iter().enumerate() {
            if !used[i] {
                order.push(i);
            }
        }
        let _ = self.queue.reorder(&order);
        // Re-seat the exact occurrence (see doc comment above).
        if let Some((id, rank)) = cur_fix {
            let mut seen = 0;
            for (i, t) in self.queue.items().iter().enumerate() {
                if t.id == id {
                    if seen == rank {
                        self.queue.jump_to(i);
                        break;
                    }
                    seen += 1;
                }
            }
        }
    }

    /// Id of the likely next track (repeat-all wraps), for prefetching.
    /// Repeat One needs none: the same song replays from the cache.
    fn peek_next_id(&self) -> Option<String> {
        if self.queue.repeat == yutu_player::RepeatMode::One {
            return None;
        }
        let items = self.queue.items();
        if items.is_empty() {
            return self.autoplay.first().map(|t| t.id.clone());
        }
        let pos = self.queue.position().unwrap_or(0);
        if pos + 1 < items.len() {
            Some(items[pos + 1].id.clone())
        } else if self.queue.repeat == yutu_player::RepeatMode::All {
            items.first().map(|t| t.id.clone())
        } else {
            self.autoplay.first().map(|t| t.id.clone())
        }
    }

    /// Same fetch without touching the status line (bulk actions keep theirs).
    /// Lyrics + hero art warming for a track start (every advance path
    /// funnels through play/fetch_current_task, so both call this).
    fn track_start_tasks(&mut self, t: &Track) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        if let Some(art) = self.large_art_task(t) {
            tasks.push(art);
        }
        // Warm rows ahead (current + next 5, both keys): art lands without
        // waiting for another payload, and failures retry since
        // ArtworkReady clears the loading flag on error.
        if let Some(pos) = self.queue.position() {
            let ahead: Vec<Track> = self
                .queue
                .items()
                .iter()
                .skip(pos)
                .take(6)
                .cloned()
                .collect();
            tasks.extend(self.artwork_tasks(&ahead));
        }
        #[cfg(feature = "lyrics")]
        {
            if !self.lyrics_cache.contains_key(&t.id)
                && self.lyrics_loading.as_deref() != Some(t.id.as_str())
            {
                // Downloaded lyrics render with no network.
                if let Some(cached) = load_lyrics(&t.id) {
                    self.lyrics_cache.insert(t.id.clone(), cached);
                } else {
                    self.lyrics_loading = Some(t.id.clone());
                    tasks.push(Self::lyrics_task(t.clone()));
                }
            }
        }
        tasks
    }

    fn fetch_current_task(&mut self) -> iced::Task<Message> {
        match self.queue.current().cloned() {
            None => iced::Task::none(),
            Some(t) => {
                // Same cut-the-old-song feedback as the streaming path.
                if let Some(audio) = &self.audio {
                    audio.stop();
                }
                self.loading_track = Some(t.id.clone());
                self.end_advanced = false;
                self.paused = false;
                self.paused_seek = false;
                self.seek_hold_until = None;
                self.position_secs = 0.0;
                // New load cancels any pending resume seek (see above).
                self.pending_resume = None;
                self.restored_pending = false;
                let mut tasks = self.track_start_tasks(&t);
                // Downloaded bytes play with no network (bulk queue paths
                // like playlist Play-all funnel here, not through
                // play_current_task).
                if self.downloaded.contains(&t.id) && self.try_play_offline(&t) {
                    return iced::Task::batch(tasks);
                }
                tasks.push(Self::resolve_task(t.id));
                iced::Task::batch(tasks)
            }
        }
    }

    /// Manual queue first (shuffle/repeat apply there); when it truly ends
    /// and repeat is Off, the autoplay tail shifts in head-first so the
    /// single current-pointer (progress/lyrics/MPRIS) keeps working.
    /// True when current changed.
    fn advance_queue(&mut self) -> bool {
        self.advance_inner(false)
    }

    /// Manual Next path: Repeat One never replays here, only at the
    /// natural end of a song.
    fn advance_queue_manual(&mut self) -> bool {
        self.advance_inner(true)
    }

    fn advance_inner(&mut self, manual: bool) -> bool {
        // Shuffle is a physical order (see set_shuffle), so the walk is
        // always linear: no repeats, a real end, then autoplay.
        let advanced = if manual {
            self.queue.next_manual().is_some()
        } else {
            self.queue.next_track().is_some()
        };
        if advanced {
            return true;
        }
        // Repeat All/One loop the manual queue by design (like
        // Spotify's repeat); only Off reaches autoplay.
        if self.queue.repeat != yutu_player::RepeatMode::Off {
            return false;
        }
        if self.autoplay.is_empty() {
            return false;
        }
        let t = self.autoplay.remove(0);
        self.history.push(t.id.clone());
        self.persist_play(&t.id);
        self.queue.push(t);
        self.queue.jump_to(self.queue.len().saturating_sub(1));
        true
    }

    fn apply_volume(&self) {
        if let Some(a) = &self.audio {
            a.set_volume(self.volume);
        }
    }

    /// Backend clock (drives the progress bar + MPRIS position).
    fn audio_position(&self) -> f64 {
        self.audio
            .as_ref()
            .map(|a| a.position_secs())
            .unwrap_or(0.0)
            .max(0.0)
    }

    /// Backend duration when it knows better, else the track's metadata.
    fn audio_duration(&self) -> f64 {
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
    fn maybe_scrobble_task(&mut self, track: &Track) -> iced::Task<Message> {
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
    fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (add a ListenBrainz token or Last.fm keys in Settings)");
    }

    #[cfg(not(feature = "submit"))]
    fn status_hint_no_scrobble_creds(&mut self) {
        self.status
            .push_str(" | Scrobble-ready (rebuild with `--features submit`)");
    }

    /// Big green PLAY pill for hero headers (Spotify-style).
    fn play_pill(label: String, msg: Message) -> iced::Element<'static, Message> {
        use iced::widget::{button, row, text};
        button(
            row![
                icon("play", 16.0, iced::Color::WHITE),
                text(label).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::default()
                }),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(green_play)
        .on_press(msg)
        .padding(9)
        .into()
    }

    /// Shuffle pill for hero headers: same red pill as play_pill with a
    /// shuffle glyph (search heroes).
    fn shuffle_pill(label: String, msg: Message) -> iced::Element<'static, Message> {
        use iced::widget::{button, row, text};
        button(
            row![
                icon("shuffle", 16.0, iced::Color::WHITE),
                text(label).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::default()
                }),
            ]
            .spacing(6)
            .align_y(iced::alignment::Vertical::Center),
        )
        .style(green_play)
        .on_press(msg)
        .padding(9)
        .into()
    }

    /// Spotify-style numbered row: rank + art + play + title + time + actions.
    /// Row art wrapped in a click-to-play button: on hover the art
    /// dims behind a backdrop with a white play icon on top.
    fn art_play_button(
        &self,
        t: &Track,
        hovered: bool,
        play: Message,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, stack};
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
        button(art)
            .style(row_click)
            .on_press(play)
            .padding(0)
            .into()
    }

    /// Fixed box standing in for a hidden row action (rows never jump).
    fn action_spacer() -> iced::Element<'static, Message> {
        iced::widget::Space::new()
            .width(iced::Length::Fixed(ROW_ACTION_W))
            .height(iced::Length::Shrink)
            .into()
    }

    /// Like + more buttons for song rows: hidden until hover (spacers
    /// keep the width); the liked heart stays visible as state.
    fn song_actions(
        &self,
        t: &Track,
        hovered: bool,
    ) -> (iced::Element<'_, Message>, iced::Element<'_, Message>) {
        use iced::widget::button;
        let liked = self.liked.contains(&t.id);
        let heart_color = if liked { SPOT_GREEN } else { MUTED };
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
            button(icon("ellipsis", 15.0, MUTED))
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
    fn coll_more(hovered: bool, open: Message) -> iced::Element<'static, Message> {
        use iced::widget::button;
        if hovered {
            button(icon("ellipsis", 15.0, MUTED))
                .style(hover_row)
                .on_press(open)
                .padding(6)
                .into()
        } else {
            Self::action_spacer()
        }
    }

    fn numbered_track_row(&self, t: &Track, n: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let (heart, more) = self.song_actions(t, hovered);
        mouse_area(
            container(
                row![
                    text(format!("{n}")).size(13).color(MUTED).width(28),
                    self.art_play_button(t, hovered, Message::PlayNow(t.id.clone())),
                    Self::track_text_single(&t.title, &t.artist, 14, iced::Color::WHITE),
                    duration_text(t.duration_secs),
                    heart,
                    more,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    /// Album card with cover art (search grids, artist pages).
    /// Fixed standard size with single-line truncated labels: long titles
    /// crop instead of stretching the card, so every card aligns.
    fn album_card(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, text};
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => format!("{} · {}", a.artist, y),
            None => a.artist.clone(),
        };
        button(
            column![
                self.art_thumb(cover.as_deref(), HOME_PICK_ART),
                column![
                    text(truncate_title(&a.title, 20))
                        .size(13)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                    text(truncate_title(&sub, 22))
                        .size(11)
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
    fn artist_card(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, text};
        use iced::Fill;
        // Same hires key stage_search warms (raw would placeholder).
        let photo = a
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        button(
            column![
                self.art_thumb(photo.as_deref(), HOME_PICK_ART),
                column![
                    text(truncate_title(&a.name, 20))
                        .size(13)
                        .width(Fill)
                        .wrapping(Wrapping::None),
                    text("Artist").size(11).color(MUTED).width(Fill),
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

    /// Cover for an album: API art first, else the first known track's.
    fn album_cover(&self, album: &Album) -> Option<String> {
        // Hi-res for 140px+ cards and detail headers (hires passes
        // non-YTM hosts through unchanged, so tests stay green).
        album
            .thumbnail_url
            .clone()
            .or_else(|| {
                self.catalog
                    .values()
                    .find(|t| t.album.as_deref() == Some(album.title.as_str()))
                    .and_then(|t| t.thumbnail_url.clone())
            })
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Photo for an artist: last search payload first, else catalog.
    /// Hi-res for 140px+ cards and the detail header.
    fn artist_photo(&self, name: &str) -> Option<String> {
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
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
    }

    /// Bulk-append without spamming history (one entry-free push per track),
    /// then play the first new track for real.
    /// Drop one id from the autoplay tail (queue ops absorb matches so
    /// the two lists never show the same song twice).
    fn absorb_autoplay(&mut self, id: &str) {
        self.autoplay.retain(|t| t.id != id);
    }

    fn queue_many(&mut self, tracks: Vec<Track>, label: &str) -> iced::Task<Message> {
        let n = tracks.len();
        if n == 0 {
            self.status = format!("{label}: nothing to queue.");
            return iced::Task::none();
        }
        // A bulk queue (radio/play-all) absorbs matching autoplay ids so
        // the two lists never show the same song twice.
        let incoming: std::collections::HashSet<&str> =
            tracks.iter().map(|t| t.id.as_str()).collect();
        self.autoplay.retain(|t| !incoming.contains(t.id.as_str()));
        let first_new = self.queue.len();
        // Warm art for the incoming head (~6 tracks: 2 keys each under
        // the 12-task cap); the rest paints as the ahead-window reaches
        // them on advance. Deduped, so repeats cost nothing.
        let art_tasks = self.artwork_tasks(&tracks);
        // Resolve in memory first, persist once: per-track SQLite traffic
        // (open + fsync each) freezes the GUI on long playlists.
        let resolved: Vec<Track> = tracks
            .iter()
            .map(|t| self.resolve_for_play_mem(t))
            .collect();
        self.cache_tracks(&resolved);
        for rt in resolved {
            self.queue.push(rt);
        }
        self.queue.jump_to(first_new);
        self.playing_from = label.to_string();
        self.status = format!("{label}: queued {n} track(s).");
        self.apply_volume();
        let mut tasks = art_tasks;
        tasks.push(self.fetch_current_task());
        iced::Task::batch(tasks)
    }

    /// Play-all for the current detail view (artist/album/playlist/
    /// radio/discovery/search). With `shuffle`, the fresh queue plays in
    /// fully shuffled order (random first track, always reshuffled).
    fn play_all_tracks(&mut self, shuffle: bool) -> iced::Task<Message> {
        let (tracks, label) = match &self.route {
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                (self.artist_tracks(&name), format!("Artist {name}"))
            }
            Route::Album(id) => {
                let title = self.resolve_album_title(id);
                (self.album_tracks(id), format!("Album {title}"))
            }
            Route::PlaylistDetail(id) => match self.find_playlist(id) {
                Some(p) => (p.tracks.clone(), format!("Playlist {}", p.title)),
                None => (Vec::new(), "Playlist not found".into()),
            },
            Route::Radio(_) => (self.radio_tracks.clone(), "Radio".to_string()),
            Route::Discovery => (self.weekly_mix_tracks(), "Weekly Mix".to_string()),
            _ => (self.results.tracks.clone(), "Search results".to_string()),
        };
        if tracks.is_empty() {
            self.status = format!("{label}: nothing to queue.");
            return iced::Task::none();
        }
        // Fresh context (like PlayNow): the old queue and tail go away,
        // then the list plays (shuffled first when asked, so the first
        // fetch already targets the random head).
        self.play_fresh(tracks, &label, shuffle)
    }

    /// Shuffle-play a known track list (search hero actions): same fresh
    /// context as play-all, always shuffled, honest empty status.
    fn shuffle_tracks(&mut self, tracks: Vec<Track>, label: &str) -> iced::Task<Message> {
        if tracks.is_empty() {
            self.status = format!("{label}: nothing to play yet.");
            return iced::Task::none();
        }
        self.play_fresh(tracks, label, true)
    }

    fn play_index(&mut self, i: usize) -> iced::Task<Message> {
        match self.results.tracks.get(i).cloned() {
            Some(t) => self.enqueue(t),
            None => {
                self.status = format!("No track at index {i}.");
                iced::Task::none()
            }
        }
    }

    fn enqueue_track_task(&mut self, id: &str) -> iced::Task<Message> {
        match self.find_track(id) {
            Some(t) => self.enqueue(t),
            None => self.unknown_track_task(id),
        }
    }

    /// Fetch missing artwork for freshly seen tracks (cap: 12 per payload,
    /// deduped by URL, never refetched once attempted).
    fn artwork_tasks(&mut self, tracks: &[Track]) -> Vec<iced::Task<Message>> {
        // Both resolutions, deduped first. Two keys per track means the
        // 12-task cap covers about 6 fresh tracks per payload, not 12.
        // The large side uses the hero URL so rows share the drawer cache
        // key instead of downloading the same image twice.
        let mut seen = std::collections::HashSet::new();
        let urls: Vec<String> = tracks
            .iter()
            .flat_map(|t| {
                [
                    t.thumbnail_url.clone(),
                    // Hero key for small-only tracks so the drawer shares
                    // the row fetch instead of warming late.
                    t.thumbnail_large_url
                        .clone()
                        .map(|u| yutu_core::hires_thumb_url(&u, 544))
                        .or(t
                            .thumbnail_url
                            .clone()
                            .map(|u| yutu_core::hires_thumb_url(&u, 544))),
                ]
            })
            .flatten()
            .filter(|u| seen.insert(u.clone()))
            .collect();
        self.artwork_tasks_for_urls(&urls)
    }

    /// Same fetch for any URL list (album covers, artist photos).
    /// Skips memory, in-flight, on-disk, and cooldown-failed URLs.
    fn artwork_tasks_for_urls(&mut self, urls: &[String]) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        for url in urls {
            if !self.artwork.contains_key(url)
                && !self.artwork_loading.contains(url)
                && tasks.len() < 12
                && !artwork_file_exists(url)
                && !Self::cooled(&self.artwork_failed, url)
            {
                self.artwork_loading.insert(url.clone());
                let url_owned = url.clone();
                let url_for_msg = url_owned.clone();
                tasks.push(iced::Task::perform(
                    async move { yutu_player::fetch_bytes(&url_owned).await },
                    move |result| Message::ArtworkReady {
                        url: url_for_msg.clone(),
                        result: result.map_err(|e| e.to_string()),
                    },
                ));
            }
        }
        tasks
    }

    /// True when `url` is artwork for a downloaded track (raw thumb or
    /// 544px hero key: the two keys the fetch tasks warm). Gates on-disk
    /// art writes so the bundle holds only downloaded songs.
    fn is_downloaded_art(&self, url: &str) -> bool {
        self.downloaded_order.iter().any(|id| {
            self.catalog.get(id).is_some_and(|t| {
                t.thumbnail_url.as_deref() == Some(url)
                    || t.thumbnail_large_url.as_deref() == Some(url)
                    || t.thumbnail_large_url
                        .as_deref()
                        .is_some_and(|u| yutu_core::hires_thumb_url(u, 544) == url)
            })
        })
    }

    /// Cover/photo art by URL at any size (albums, artists).
    /// Memory first, then the on-disk download bundle (`Handle::from_path`
    /// stays cached by iced, so repeated rows decode once).
    fn art_handle(&self, url: &str) -> Option<iced::widget::image::Handle> {
        if let Some(handle) = self.artwork.get(url) {
            return Some(handle.clone());
        }
        artwork_path_for(url).map(iced::widget::image::Handle::from_path)
    }

    /// Cover/photo art by URL at any size (albums, artists).
    fn art_thumb(&self, url: Option<&str>, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image};
        match url.and_then(|u| self.art_handle(u)) {
            // Same card box as the placeholder so loaded art never looks
            // like a different component (or shifts the row).
            Some(handle) => container(image(handle.clone()).width(size).height(size))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            None => container(icon("music", size * 0.4, MUTED))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    /// 40px artwork for rows/panel, music-note placeholder until loaded.
    fn thumb(&self, track: &Track) -> iced::Element<'_, Message> {
        self.thumb_sized(track, 40.0)
    }

    /// Two-line song label for the narrow queue sidebar: the title
    /// wraps to at most two lines (40 chars + ellipsis), the artist is
    /// always a single truncated line below it, and the compact block
    /// is vertically centered by the fixed-height row container. The
    /// column clips, so a narrower window cuts text instead of painting
    /// over the duration/remove controls.
    fn track_text<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        use iced::widget::text::{LineHeight, Wrapping};
        use iced::widget::{column, text};
        use iced::Fill;
        // Explicit 1.25 line height keeps 1- and 2-line titles compact.
        const LH: f32 = 1.25;
        let artist_px = f32::from(title_size.saturating_sub(2).max(11));
        column![
            text(truncate_title(title, 40))
                .size(f32::from(title_size))
                .color(color)
                .width(Fill)
                .line_height(LineHeight::Relative(LH)),
            text(truncate_title(artist, 32))
                .size(artist_px)
                .color(MUTED)
                .width(Fill)
                .wrapping(Wrapping::None)
                .line_height(LineHeight::Relative(LH)),
        ]
        .spacing(2)
        .width(Fill)
        .clip(true)
        .into()
    }

    /// Single-line song label for wide lists (search, library, radio,
    /// queue page): title and artist both truncate with an ellipsis and
    /// never wrap. The column clips, so in a narrow window the text cuts
    /// instead of painting over the duration/action controls, and every
    /// row shares one fixed height.
    fn track_text_single<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{column, text};
        use iced::Fill;
        column![
            text(truncate_title(title, 45))
                .size(f32::from(title_size))
                .color(color)
                .width(Fill)
                .wrapping(Wrapping::None),
            text(truncate_title(artist, 40))
                .size(f32::from(title_size.saturating_sub(2).max(11)))
                .color(MUTED)
                .width(Fill)
                .wrapping(Wrapping::None),
        ]
        .spacing(2)
        .width(Fill)
        .clip(true)
        .into()
    }

    /// Settings login panel: states are signed-in, code-pending, idle.
    /// The URL is also printed to stderr (clickable in the terminal).
    #[cfg(feature = "innertube")]
    fn google_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text};
        if self.logged_in {
            return column![
                text("Signed in ✓").size(13).color(SPOT_GREEN),
                row![
                    button(
                        text(if self.login_busy {
                            "…"
                        } else {
                            "Import library"
                        })
                        .size(12)
                    )
                    .style(green_play)
                    .on_press(Message::LibraryImport)
                    .padding(7),
                    button(text("Sign out").size(12))
                        .style(hover_row)
                        .on_press(Message::GoogleLogout)
                        .padding(7),
                ]
                .spacing(8),
            ]
            .spacing(4)
            .into();
        }
        if let Some(login) = self.google_login.clone() {
            return column![
                text("1. Open this URL in your browser:")
                    .size(12)
                    .color(MUTED),
                text(login.verification_url.clone()).size(12),
                text(format!("2. Enter the code: {}", login.user_code)).size(13),
                row![
                    button(
                        text(if self.login_busy {
                            "Checking…"
                        } else {
                            "I’ve approved: check"
                        })
                        .size(13)
                    )
                    .style(green_play)
                    .on_press(Message::GoogleLoginPoll)
                    .padding(8),
                    button(text("Cancel").size(12))
                        .style(hover_row)
                        .on_press(Message::GoogleLogout)
                        .padding(7),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center),
            ]
            .spacing(4)
            .into();
        }
        column![
            text("Your liked songs + library, imported from YouTube Music.")
                .size(12)
                .color(MUTED),
            text("Needs a free Google OAuth client: set YUTU_OAUTH_CLIENT_ID + YUTU_OAUTH_CLIENT_SECRET (same as youtui: register a desktop client).")
                .size(11)
                .color(MUTED),
            button(
                text(if self.login_busy {
                    "Contacting Google…"
                } else {
                    "Sign in with Google"
                })
                .size(13)
            )
            .style(green_play)
            .on_press(Message::GoogleLoginStart)
            .padding(8),
        ]
        .spacing(4)
        .into()
    }

    /// Cookie import panel: normal browser login, no OAuth client needed.
    /// A saved session stays signed in across restarts (re-import refreshes
    /// without another export); likes, dislikes, and playlist edits mirror
    /// to YouTube Music while signed in.
    #[cfg(feature = "innertube")]
    fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text, text_input};
        use iced::Fill;
        if self.cookie_logged_in {
            return column![
                text("Signed in with cookies").size(13).color(SPOT_GREEN),
                row![
                    button(
                        text(if self.login_busy {
                            "…"
                        } else {
                            "Import library"
                        })
                        .size(12)
                    )
                    .style(green_play)
                    .on_press(Message::CookieSessionImport)
                    .padding(7),
                    button(text("Sign out").size(12))
                        .style(hover_row)
                        .on_press(Message::CookieLogout)
                        .padding(7),
                ]
                .spacing(8),
            ]
            .spacing(4)
            .into();
        }
        column![
            text("Log into music.youtube.com in your browser, export cookies (e.g. the “Get cookies.txt” extension), then paste the file path (no Google Cloud client required). The session is saved, so restarts stay signed in.")
                .size(11)
                .color(MUTED),
            text_input("/path/to/cookies.txt", &self.cookie_path)
            .style(rounded_input)
                .on_input(Message::CookiePathChanged)
                .width(Fill)
                .padding(10),
            button(
                text(if self.login_busy {
                    "…"
                } else {
                    "Import"
                })
                .size(12)
            )
            .style(green_play)
            .on_press(Message::CookieImport)
            .padding(7),
        ]
        .spacing(4)
        .into()
    }

    /// Sync panel: manual refresh plus the scheduled pull that keeps
    /// web edits (likes, playlist songs/titles, deletions) flowing into
    /// the app. Pending local pushes replay first, so unpushed intent is
    /// never dropped by the pull.
    #[cfg(feature = "innertube")]
    fn sync_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, checkbox, column, row, text};
        if !self.logged_in && !self.cookie_logged_in {
            return text("Sign in above to sync.").size(12).color(MUTED).into();
        }
        column![
            text(format!(
                "Last synced: {}",
                fmt_sync_ago(self.last_sync_unix, now_unix())
            ))
            .size(12)
            .color(MUTED),
            row![
                button(
                    text(if self.login_busy || self.import_active {
                        "…"
                    } else {
                        "Sync now"
                    })
                    .size(12)
                )
                .style(green_play)
                .on_press(Message::SyncNow)
                .padding(7),
                checkbox(self.auto_sync)
                    .label("Auto-sync every 15 min")
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
    fn sync_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text("Sync needs `--features innertube`.")
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Top import banner (in-flow, pushes content down): live progress
    /// while an import runs (songs/liked counts, playlists done/total,
    /// red bar with %). `None` when idle, so layout is untouched.
    #[cfg(feature = "innertube")]
    fn import_banner_view(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{column, container, progress_bar, row, text, Space};
        use iced::Fill;
        if !self.import_active {
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
                format!("{shown}/{expected} songs")
            } else {
                format!("{shown} songs")
            };
            format!(
                "{songs} · {} liked · {}/{} playlists",
                self.import_likes, self.import_done, self.import_total
            )
        } else {
            format!(
                "{} songs · {} liked · reading library…",
                shown, self.import_likes
            )
        };
        Some(
            container(
                column![
                    row![
                        text("Importing your library")
                            .size(13)
                            .color(iced::Color::WHITE),
                        Space::new().width(Fill),
                        text(pct_label).size(13).color(SPOT_GREEN),
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
    /// Debug-only UI (release builds keep stderr, not the panel).
    fn error_section(&self) -> iced::Element<'_, Message> {
        #[cfg(not(debug_assertions))]
        {
            return iced::widget::text("").size(1).into();
        }
        #[cfg(debug_assertions)]
        {
            use iced::widget::{column, text};
            let body: iced::Element<'_, Message> = if self.error_log.is_empty() {
                text("No errors recorded.").size(12).color(MUTED).into()
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
            column![text("Recent playback errors").size(17), body,]
                .spacing(4)
                .into()
        }
    }

    /// Offline builds: cookie import needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text("Cookie import needs `--features innertube`.")
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Offline builds: login needs the innertube feature.
    #[cfg(not(feature = "innertube"))]
    fn google_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::text;
        text("Google login needs `--features innertube`.")
            .size(12)
            .color(MUTED)
            .into()
    }

    /// Open the context menu, freezing the cursor so the menu stays put.
    fn open_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.context_menu = Some(id);
        self.submenu_playlists_for = None;
        self.pl_menu = None;
        self.coll_menu = None;
    }

    /// Sidebar visibility with hover hysteresis: unpinned, the panel expands
    /// when the cursor enters the rail + grace zone, then stays open while
    /// the cursor is anywhere inside the expanded panel + grace. It only
    /// collapses once the cursor leaves that wider zone. The state is
    /// latched in `CursorMoved` (never derived from the transient animated
    /// width, which would flip the target mid-glide).
    fn sidebar_open(&self) -> bool {
        self.sidebar_pinned || self.sidebar_latched
    }

    /// Right panel visibility, same hysteresis contract as the sidebar.
    fn right_open(&self) -> bool {
        self.right_pinned || self.right_latched
    }

    /// Animated sidebar target widths from the open states above.
    fn sidebar_target(&self) -> f32 {
        if self.sidebar_open() {
            235.0
        } else {
            56.0
        }
    }

    /// Animated right-panel target width (collapsed keeps a rail with
    /// queue/autoplay thumbnails; an empty rail reserves nothing).
    fn right_target(&self) -> f32 {
        if self.right_open() {
            285.0
        } else if self.right_rail_empty() {
            0.0
        } else {
            RIGHT_RAIL_W
        }
    }

    /// No queue and no autoplay: the collapsed rail would show nothing.
    fn right_rail_empty(&self) -> bool {
        self.queue.is_empty() && self.autoplay.is_empty()
    }

    /// True while any transition is still converging (drives the ticker).
    fn animating(&self) -> bool {
        self.sidebar_w != self.sidebar_target()
            || self.right_w != self.right_target()
            || self.drawer_anim != if self.drawer_open { 1.0 } else { 0.0 }
            || self.drag_gap != self.drag_gap_target()
            || self.scroll_fading()
    }

    /// Scrollbar opacity for a scrollable key (0 when never scrolled).
    fn scroll_opacity(&self, key: &'static str) -> f32 {
        self.scroll_hint.get(key).map(|(_, o)| *o).unwrap_or(0.0)
    }

    /// Record scroll motion: the bar shows at full strength, then the
    /// ticker fades it once idle (see `Message::Animate`).
    fn lit_scroll(&mut self, key: &'static str) {
        self.scroll_hint
            .insert(key, (std::time::Instant::now(), 1.0));
    }

    /// True while any scrollbar fade still needs ticker frames: lit but
    /// idle past the delay (fresh motion needs no frames, it sits at 1).
    fn scroll_fading(&self) -> bool {
        self.scroll_hint.values().any(|(at, o)| {
            *o > 0.0 && at.elapsed() > std::time::Duration::from_millis(SCROLL_FADE_DELAY_MS)
        })
    }

    /// Eased drop-slot gap target: open while an armed slot commits to a
    /// real move, else shut. Own-spot hovers never open the gap.
    fn drag_gap_target(&self) -> f32 {
        if self.effective_drag_slot().is_some() {
            1.0
        } else {
            0.0
        }
    }

    /// Latched slot filtered to real moves (own-spot hovers are None).
    fn effective_drag_slot(&self) -> Option<usize> {
        self.drag.and_then(|d| {
            d.slot
                .filter(|s| slot_commit(self.queue.len(), d.from, *s).is_some())
        })
    }

    /// Cancel any in-progress drag plus its press snapshot (one place,
    /// so no stale snap survives a queue mutation).
    fn cancel_drag(&mut self) {
        self.drag = None;
        self.press_snap = None;
    }

    /// Close the context menu and drop its anchor.
    fn close_menu(&mut self) {
        self.context_menu = None;
        self.menu_at = None;
        self.submenu_playlists_for = None;
        self.pl_menu = None;
        self.coll_menu = None;
    }

    /// Open the playlist menu, freezing the cursor like the track menu.
    fn open_pl_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.pl_menu = Some(id);
        self.context_menu = None;
        self.submenu_playlists_for = None;
        self.coll_menu = None;
        self.show_suggest = false;
    }

    /// Open the search collection menu, same frozen anchor (only one
    /// menu kind is ever open).
    fn open_coll_menu(&mut self, menu: CollectionMenu) {
        self.menu_at = self.cursor;
        self.coll_menu = Some(menu);
        self.context_menu = None;
        self.submenu_playlists_for = None;
        self.pl_menu = None;
        self.show_suggest = false;
    }

    /// Collection hover with the same modal freeze as track rows: rows
    /// behind scrims never highlight.
    fn coll_hover(&self, h: Option<String>) -> Option<String> {
        if self.context_menu.is_some()
            || self.pl_menu.is_some()
            || self.coll_menu.is_some()
            || self.show_suggest
        {
            None
        } else {
            h
        }
    }

    /// The collection menu's hit is still in results (stale rows never
    /// open a scrim-less modal, and fresh results prune a dead menu).
    fn coll_hit_present(&self, menu: &CollectionMenu) -> bool {
        match menu {
            CollectionMenu::Album(id) => self.results.albums.iter().any(|a| &a.id == id),
            CollectionMenu::Artist(id) => self.results.artists.iter().any(|a| &a.id == id),
            CollectionMenu::Playlist(id) => self.results.playlists.iter().any(|p| &p.id == id),
        }
    }

    /// Open a collection menu only when its hit is still in results.
    fn open_coll_for(&mut self, menu: CollectionMenu) {
        if self.coll_hit_present(&menu) {
            self.show_suggest = false;
            self.open_coll_menu(menu);
        } else {
            self.status = "That result is gone: search again.".into();
        }
    }

    /// Drop the collection menu when fresh results no longer contain
    /// its hit (nothing would render, but the modal freeze would
    /// linger and swallow hovers).
    fn prune_coll_menu(&mut self) {
        if self
            .coll_menu
            .as_ref()
            .is_some_and(|m| !self.coll_hit_present(m))
        {
            self.coll_menu = None;
        }
    }

    /// Stored-row URL at a display size (shared threshold, one place).
    fn cover_url_for(url: &str, size: f32) -> String {
        if size >= Self::HIRES_MIN_PX {
            yutu_core::hires_thumb_url(url, 544)
        } else {
            url.to_string()
        }
    }

    /// Playlist cover: first resolvable track art, else the stored
    /// row's own art, else a fixed music placeholder (same box either
    /// way, rows never shift).
    fn playlist_cover(&self, pl: &Playlist, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::container;
        if let Some(t) = pl.tracks.first().and_then(|t| self.find_track(&t.id)) {
            return self.thumb_sized(&t, size);
        }
        if let Some(url) = pl
            .tracks
            .first()
            .and_then(|t| t.thumbnail_url.clone())
            .map(|u| Self::cover_url_for(&u, size))
        {
            if let Some(handle) = self.art_handle(&url) {
                use iced::widget::image;
                return container(image(handle.clone()).width(size).height(size))
                    .style(card_bg)
                    .padding(0)
                    .width(iced::Length::Fixed(size))
                    .height(iced::Length::Fixed(size))
                    .align_x(iced::alignment::Horizontal::Center)
                    .align_y(iced::alignment::Vertical::Center)
                    .into();
            }
        }
        container(icon("music", size * 0.4, MUTED))
            .style(card_bg)
            .padding(0)
            .width(iced::Length::Fixed(size))
            .height(iced::Length::Fixed(size))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    /// Display size at/above which track art uses the 544px hero key
    /// instead of the raw small URL (song rows stay low-res). Focal
    /// sub-64 headers (home picks 52, menu headers 56) deliberately
    /// use thumb_hero anyway: they are card headers, not rows, and the
    /// hero key is already warmed, so the extra fetch pays once.
    const HIRES_MIN_PX: f32 = 64.0;

    /// Art URL for a display size: catalog merged, hero at/above the
    /// threshold, raw small below it.
    fn art_url_for(&self, t: &Track, display_px: f32) -> Option<String> {
        let merged = self.catalog.get(&t.id).unwrap_or(t);
        if display_px >= Self::HIRES_MIN_PX {
            Self::hero_art_url(merged)
        } else {
            merged.thumbnail_url.clone()
        }
    }

    /// Hero art element for focal headers (context menu, playlist menu,
    /// search hero): always the 544px key.
    fn thumb_hero(&self, track: &Track, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image};
        let url = self.hero_art_for(track);
        match url.as_deref().and_then(|u| self.art_handle(u)) {
            Some(handle) => container(image(handle.clone()).width(size).height(size))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            None => container(icon("music", size * 0.4, MUTED))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    /// Artwork at any square size (home cards use 52, rows use 40).
    fn thumb_sized(&self, track: &Track, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image};
        // Catalog wins on art: queued/stored rows may predate the fetch.
        // At/above HIRES_MIN_PX the hero key is used (same key the
        // fetch warms), below it the raw small URL.
        let url = self.art_url_for(track, size);
        match url.as_deref().and_then(|u| self.art_handle(u)) {
            // Same card box as the placeholder (see art_thumb).
            Some(handle) => container(image(handle.clone()).width(size).height(size))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
            None => container(icon("music", size * 0.4, MUTED))
                .style(card_bg)
                .padding(0)
                .width(iced::Length::Fixed(size))
                .height(iced::Length::Fixed(size))
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    /// Record a playback/fetch failure for the Settings error log (capped)
    /// and stderr (same line, so journald and the in-app log agree).
    fn log_error(&mut self, msg: String) {
        eprintln!("[yutu-play] {msg}");
        if self.error_log.len() >= 30 {
            self.error_log.remove(0);
        }
        self.error_log.push(msg);
    }

    /// Apply a pending resume seek once audio has started (restored
    /// queue): seeks when past 5s, otherwise starts from the top.
    fn consume_resume(&mut self) {
        // Pending resume seek after audio starts (restored queue):
        // applied once, dropped on failure paths by the callers.
        if let Some(pos) = self.pending_resume.take() {
            if pos > 5.0 {
                self.seek_to_secs(pos);
            }
        }
    }

    /// First play after a session restore: load the saved track and arm
    /// the saved-position seek. Some(task) when it fired (callers return
    /// it); any other load clears the flag instead.
    fn take_restored_play(&mut self) -> Option<iced::Task<Message>> {
        if self.restored_pending && self.queue.current().is_some() {
            self.restored_pending = false;
            let resume = (self.position_secs > 5.0).then_some(self.position_secs);
            let task = self.play_current_task();
            self.pending_resume = resume;
            Some(task)
        } else {
            self.restored_pending = false;
            None
        }
    }

    /// Seek to an absolute position: backend first, manual clock +
    /// anti-flicker hold on success, logged honest error otherwise.
    fn seek_to_secs(&mut self, pos: f64) {
        let pos = pos.max(0.0);
        match &self.audio {
            None => {
                self.status = "No audio device.".into();
            }
            Some(audio) => match audio.seek_secs(pos) {
                Ok(()) => {
                    self.position_secs = pos;
                    // Hold the manual clock ~1.2s: the backend still
                    // reports the pre-seek position for a tick or two.
                    self.seek_hold_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(1200));
                    // While paused the backend may never catch up, so the
                    // hold lasts until resume (kills the paused flicker).
                    self.paused_seek = self.paused;
                }
                Err(e) => {
                    let msg = format!("Seek failed ({e}).");
                    self.log_error(msg.clone());
                    self.status = msg;
                }
            },
        }
    }

    /// Apply one desktop transport command (MPRIS widget / media keys).
    /// Next/Previous return their real play tasks (dropping them would
    /// move the pointer without ever fetching audio).
    fn apply_player_command(&mut self, cmd: yutu_desktop::PlayerCommand) -> iced::Task<Message> {
        use yutu_desktop::PlayerCommand as C;
        match cmd {
            C::Play => {
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.paused_seek = false;
                    self.status = "Resumed (desktop).".into();
                }
            }
            C::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                }
                self.paused = true;
                self.status = "Paused (desktop).".into();
            }
            C::PlayPause => {
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if self.paused {
                    if let Some(a) = &self.audio {
                        a.resume();
                    }
                    self.paused = false;
                    self.paused_seek = false;
                    self.status = "Resumed (desktop).".into();
                } else {
                    if let Some(a) = &self.audio {
                        a.pause();
                    }
                    self.paused = true;
                    self.status = "Paused (desktop).".into();
                }
            }
            C::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                }
                self.paused = true;
                self.status = "Stopped (desktop).".into();
            }
            C::Next => {
                if self.advance_queue_manual() {
                    self.apply_volume();
                    return self.play_current_task();
                }
            }
            C::Previous => {
                if self.queue.prev_track().is_some() {
                    self.apply_volume();
                    return self.play_current_task();
                }
            }
            C::Seek(offset) => {
                self.seek_to_secs(self.position_secs + offset);
            }
            C::SetPosition(pos) => {
                self.seek_to_secs(pos);
            }
            C::SetVolume(v) => {
                self.volume = (v as f32).clamp(0.0, 1.0);
                if let Some(a) = &self.audio {
                    a.set_volume(self.volume);
                }
            }
            C::SetShuffle(shuffle) => {
                if self.queue.shuffle != shuffle {
                    self.set_shuffle(shuffle);
                    self.status = if shuffle {
                        "Shuffle on (desktop).".into()
                    } else {
                        "Shuffle off (desktop).".into()
                    };
                }
            }
            C::SetLoop(loop_status) => {
                let mode = match loop_status {
                    yutu_desktop::LoopStatus::None => yutu_player::RepeatMode::Off,
                    yutu_desktop::LoopStatus::Track => yutu_player::RepeatMode::One,
                    yutu_desktop::LoopStatus::Playlist => yutu_player::RepeatMode::All,
                };
                self.queue.repeat = mode;
                self.status = format!("Repeat: {mode:?} (desktop).");
            }
        }
        iced::Task::none()
    }

    /// Drain desktop transport commands queued since the last tick.
    fn drain_player_commands(&mut self) -> iced::Task<Message> {
        let cmds: Vec<yutu_desktop::PlayerCommand> = self
            .mpris
            .as_ref()
            .map(|h| {
                h.lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .drain_commands()
            })
            .unwrap_or_default();
        if cmds.is_empty() {
            return iced::Task::none();
        }
        let mut tasks = Vec::new();
        for cmd in cmds {
            tasks.push(self.apply_player_command(cmd));
        }
        iced::Task::batch(tasks)
    }

    /// Player-bar status text: the full last-action log in debug builds;
    /// release builds only surface failures (no action chatter).
    fn status_text(&self) -> String {
        #[cfg(debug_assertions)]
        {
            self.status.clone()
        }
        #[cfg(not(debug_assertions))]
        {
            if self.status.to_lowercase().contains("fail") {
                self.status.clone()
            } else {
                String::new()
            }
        }
    }

    /// Publish current track/status/volume to the MPRIS handle (if any) so
    /// desktop clients observe live state on property read. No-op headless.
    /// Poison-tolerant: a poisoned mutex still publishes via the inner guard.
    fn publish_mpris(&self) {
        let Some(handle) = self.mpris.as_ref() else {
            return;
        };
        let mut player = handle.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(t) = self.queue.current() {
            // Crisp hero URL so external widgets get cover art (https URI
            // is valid MPRIS artUrl; no file download lifecycle needed).
            let art_url = self.hero_art_for(t).unwrap_or_default();
            player.update_track(yutu_desktop::NowPlaying {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone().unwrap_or_default(),
                track_id: t.id.clone(),
                length_secs: t.duration_secs,
                art_url,
            });
        }
        player.set_shuffle(self.queue.shuffle);
        player.set_loop_status(match self.queue.repeat {
            yutu_player::RepeatMode::Off => yutu_desktop::LoopStatus::None,
            yutu_player::RepeatMode::One => yutu_desktop::LoopStatus::Track,
            yutu_player::RepeatMode::All => yutu_desktop::LoopStatus::Playlist,
        });
        player.set_status(if self.queue.current().is_none() {
            yutu_desktop::PlaybackStatus::Stopped
        } else if self.paused {
            yutu_desktop::PlaybackStatus::Paused
        } else {
            yutu_desktop::PlaybackStatus::Playing
        });
        player.set_volume(f64::from(self.volume));
        player.set_position(self.position_secs);
    }

    /// Validate + select an EQ preset (in-memory only; never persisted).
    /// Honest about backends: rodio has no filter stage, so selecting a
    /// preset there records the choice and says so (it takes effect on mpv).
    fn select_eq_preset(&mut self, name: &str) {
        match EqPreset::preset_by_name(name) {
            Some(p) => match p.validate() {
                Ok(()) => {
                    self.eq_preset = p.name.clone();
                    match &self.audio {
                        None => {
                            self.status = format!("EQ: {} (no audio device)", p.name);
                        }
                        Some(audio) => match audio.apply_eq(&p) {
                            Ok(()) => {
                                self.status =
                                    format!("EQ: {} (live on {})", p.name, audio.backend_name());
                            }
                            Err(e) => {
                                self.status = format!("EQ: {} (saved; {e})", p.name);
                            }
                        },
                    }
                }
                Err(e) => {
                    self.status = format!("EQ preset invalid: {e}.");
                }
            },
            None => {
                self.status = format!("Unknown EQ preset: {name}.");
            }
        }
    }

    /// One-line EQ backend note for Settings (honest on rodio builds).
    fn eq_note(&self) -> String {
        match &self.audio {
            Some(a) if a.supports_eq() => "EQ is live on mpv.".into(),
            Some(_) => {
                "EQ needs the mpv backend: install libmpv / rebuild with --features mpv.".into()
            }
            None => "EQ: no audio device.".into(),
        }
    }

    /// Persisted play history for discovery (up to 200 entries).
    fn discovery_history(&self) -> Vec<HistoryEntry> {
        LibraryStore::open(&self.library_path)
            .and_then(|s| s.history(200))
            .unwrap_or_default()
    }

    /// Weekly Mix from persisted history + in-memory likes + real catalog.
    /// Disliked ("don't recommend") tracks never appear.
    pub fn weekly_mix_tracks(&self) -> Vec<Track> {
        weekly_mix(
            &self.discovery_history(),
            &self.liked,
            &self.catalog_tracks(),
            10,
            now_unix(),
        )
        .into_iter()
        .filter(|t| !self.disliked.contains(&t.id))
        .collect()
    }

    /// Forgotten Favorites from in-memory likes + persisted history + catalog.
    /// Disliked ("don't recommend") tracks never appear.
    pub fn forgotten_tracks(&self) -> Vec<Track> {
        forgotten_favorites(
            &self.liked,
            &self.discovery_history(),
            &self.catalog_tracks(),
            now_unix(),
        )
        .into_iter()
        .filter(|t| !self.disliked.contains(&t.id))
        .collect()
    }

    /// Virtual Favorites id (never persisted, never a `uN` user id).
    const FAVORITES_ID: &'static str = "__favorites";
    /// Virtual Downloaded id (ids persisted in SQLite, never a `uN` id).
    const DOWNLOADED_ID: &'static str = "__downloaded";

    fn all_playlists(&self) -> Vec<Playlist> {
        let mut out = vec![
            Playlist {
                id: Self::FAVORITES_ID.into(),
                title: "Favorites".into(),
                tracks: self.favorite_tracks(),
            },
            Playlist {
                id: Self::DOWNLOADED_ID.into(),
                title: "Downloaded".into(),
                tracks: self.downloaded_tracks(),
            },
        ];
        out.extend(self.user_playlists.clone());
        out
    }

    /// Liked tracks in add order via catalog (unresolved skipped).
    fn favorite_tracks(&self) -> Vec<Track> {
        self.liked_order
            .iter()
            .filter_map(|id| self.find_track(id))
            .collect()
    }

    /// Downloaded tracks in add order via catalog (unresolved skipped).
    fn downloaded_tracks(&self) -> Vec<Track> {
        self.downloaded_order
            .iter()
            .filter_map(|id| self.find_track(id))
            .collect()
    }

    /// Like helper: membership plus add-order append.
    fn like_id(&mut self, id: &str) {
        if self.liked.insert(id.to_string()) {
            self.liked_order.push(id.to_string());
        }
    }

    /// Unlike helper: membership plus order removal.
    fn unlike_id(&mut self, id: &str) {
        self.liked.remove(id);
        self.liked_order.retain(|x| x != id);
    }

    /// Dislike helper: membership plus add-order append.
    fn dislike_id(&mut self, id: &str) {
        if self.disliked.insert(id.to_string()) {
            self.disliked_order.push(id.to_string());
        }
    }

    /// Undislike helper: membership plus order removal.
    fn undislike_id(&mut self, id: &str) {
        self.disliked.remove(id);
        self.disliked_order.retain(|x| x != id);
    }

    /// Download helper: membership plus add-order append.
    fn mark_downloaded_id(&mut self, id: &str) {
        if self.downloaded.insert(id.to_string()) {
            self.downloaded_order.push(id.to_string());
        }
    }

    /// Undownload helper: membership plus order removal.
    fn unmark_downloaded_id(&mut self, id: &str) {
        self.downloaded.remove(id);
        self.downloaded_order.retain(|x| x != id);
    }

    fn find_playlist(&self, id: &str) -> Option<Playlist> {
        // Favorites skips the clone plus sort of the whole library.
        if id == Self::FAVORITES_ID {
            return Some(Playlist {
                id: Self::FAVORITES_ID.into(),
                title: "Favorites".into(),
                tracks: self.favorite_tracks(),
            });
        }
        if id == Self::DOWNLOADED_ID {
            return Some(Playlist {
                id: Self::DOWNLOADED_ID.into(),
                title: "Downloaded".into(),
                tracks: self.downloaded_tracks(),
            });
        }
        self.all_playlists().into_iter().find(|p| p.id == id)
    }

    /// Accept an artist id, a catalog track id, or a plain name.
    fn resolve_artist_name(&self, id_or_name: &str) -> String {
        if let Some(a) = self.results.artists.iter().find(|a| a.id == id_or_name) {
            return a.name.clone();
        }
        if let Some(t) = self.catalog.get(id_or_name) {
            return t.artist.clone();
        }
        if self.results.artists.iter().any(|a| a.name == id_or_name) {
            return id_or_name.to_string();
        }
        if self.catalog.values().any(|t| t.artist == id_or_name) {
            return id_or_name.to_string();
        }
        self.results
            .artists
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| id_or_name.to_string())
    }

    fn artist_tracks(&self, artist: &str) -> Vec<Track> {
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.artist == artist)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    fn artist_albums(&self, artist: &str) -> Vec<Album> {
        let mut albums: Vec<Album> = self
            .results
            .albums
            .iter()
            .filter(|a| a.artist == artist)
            .cloned()
            .collect();
        for t in self.artist_tracks(artist) {
            if let Some(title) = t.album.clone() {
                if !albums.iter().any(|a| a.title == title) {
                    albums.push(Album {
                        id: format!("ax-{title}"),
                        title,
                        artist: artist.into(),
                        year: None,
                        thumbnail_url: t.thumbnail_url.clone(),
                    });
                }
            }
        }
        albums
    }

    /// Accept an album id, a catalog track id, or a plain title.
    fn resolve_album_title(&self, id_or_title: &str) -> String {
        if let Some(a) = self.results.albums.iter().find(|a| a.id == id_or_title) {
            return a.title.clone();
        }
        if let Some(t) = self.catalog.get(id_or_title) {
            if let Some(album) = t.album.as_deref() {
                return album.to_string();
            }
        }
        // Synthetic cards use ax-Title ids; strip back to the title.
        if let Some(rest) = id_or_title.strip_prefix("ax-") {
            if self
                .catalog
                .values()
                .any(|t| t.album.as_deref() == Some(rest))
            {
                return rest.to_string();
            }
        }
        // Plain title passthrough (OpenTrackAlbum already passes titles).
        if self
            .catalog
            .values()
            .any(|t| t.album.as_deref() == Some(id_or_title))
        {
            return id_or_title.to_string();
        }
        id_or_title.to_string()
    }

    fn album_tracks(&self, id_or_title: &str) -> Vec<Track> {
        let title = self.resolve_album_title(id_or_title);
        let mut v: Vec<Track> = self
            .catalog
            .values()
            .filter(|t| t.album.as_deref() == Some(title.as_str()))
            .cloned()
            .collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
    }

    /// Display label for a search-result playlist id (title when known).
    /// Live lookup only runs on innertube builds; tests exercise it on
    /// every configuration.
    #[cfg(any(feature = "innertube", test))]
    fn search_playlist_label(&self, id: &str) -> String {
        self.results
            .playlists
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.title.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Play one search-result playlist: fetch its tracks live first
    /// (`shuffle` shuffles the fresh queue like hero actions).
    fn play_search_playlist(&mut self, id: String, shuffle: bool) -> iced::Task<Message> {
        self.close_menu();
        #[cfg(feature = "innertube")]
        {
            let label = self.search_playlist_label(&id);
            self.status = format!("Loading playlist {label}…");
            self.publish_mpris();
            Self::search_playlist_task(id, label, shuffle)
        }
        #[cfg(not(feature = "innertube"))]
        {
            let _ = (id, shuffle);
            self.status = "Playlists need `--features innertube`.".into();
            self.publish_mpris();
            iced::Task::none()
        }
    }

    /// Dispatcher: actions with async follow-ups (scrobble submits) return
    /// their task directly; everything else goes through `update_sync`.
    /// MPRIS state is published on every path that returns here.
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(i) => {
                self.show_suggest = false;
                let task = self.play_index(i);
                self.publish_mpris();
                task
            }
            Message::EnqueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                self.cancel_drag();
                let task = self.enqueue_track_task(&id);
                self.publish_mpris();
                task
            }
            Message::ScrobbleSubmitted { target, result } => {
                self.status = match &result {
                    Ok(()) => format!("Scrobbled to {target}."),
                    Err(e) => format!("Scrobble to {target} failed: {e}"),
                };
                self.publish_mpris();
                iced::Task::none()
            }
            message => self.update_sync(message),
        }
    }

    /// Synchronous state updates (plus stream-resolve tasks, which are
    /// Iced-side async, not scrobble submits). Publishes MPRIS before
    /// returning; the `PlayStreamId` early return publishes explicitly.
    fn update_sync(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::PlayTrack(_) | Message::EnqueueTrack(_) => {
                unreachable!("play actions are dispatched in update()")
            }
            Message::ScrobbleSubmitted { .. } => {
                unreachable!("scrobble results are dispatched in update()")
            }
            Message::Navigate(r) => {
                self.navigate(r);
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::NavBack => {
                // Side buttons never disturb a drag (Esc owns cancel);
                // menus and the popup dismiss first, second press goes.
                // Header buttons share this message, so they inherit the
                // same dismiss-first order by design (one code path).
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.coll_menu.is_some()
                {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_back();
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::NavForward => {
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.coll_menu.is_some()
                {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_forward();
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::SetRightTab(t) => {
                self.right_tab = t;
            }
            Message::SetSearchFilter(f) => {
                self.search_filter = f;
            }
            Message::HomeSearchChanged(q) => {
                self.search_query = q.clone();
                let trimmed = q.trim().to_string();
                self.search_gen = self.search_gen.wrapping_add(1);
                self.suggest_gen = self.suggest_gen.wrapping_add(1);
                if trimmed.is_empty() {
                    self.results = SearchResults::default();
                    self.all_results = SearchAll::default();
                    self.suggest.clear();
                    self.show_suggest = false;
                    self.searching = false;
                } else if trimmed.len() >= 2 {
                    self.show_suggest = true;
                    // Premature results: known tracks paint instantly.
                    let premature = self.catalog_hits(&trimmed);
                    self.results = SearchResults {
                        tracks: premature.clone(),
                        albums: Vec::new(),
                        artists: Vec::new(),
                        playlists: Vec::new(),
                    };
                    // Premature All mirrors the tracks with matching refs
                    // (hero + rows, same as a real response).
                    self.all_results = SearchAll {
                        tracks: premature.clone(),
                        mixed: premature
                            .iter()
                            .map(|t| yutu_core::MixedRef {
                                kind: 0,
                                id: t.id.clone(),
                            })
                            .collect(),
                        ..SearchAll::default()
                    };
                    #[cfg(feature = "innertube")]
                    {
                        let gen = self.search_gen;
                        let sgen = self.suggest_gen;
                        let q2 = trimmed.clone();
                        self.searching = true;
                        self.publish_mpris();
                        return iced::Task::batch([
                            Self::search_debounce_task(gen, trimmed),
                            Self::suggest_debounce_task(sgen, q2),
                        ]);
                    }
                } else {
                    // Single character: no stale results, no network yet.
                    self.results = SearchResults::default();
                    self.all_results = SearchAll::default();
                    self.suggest.clear();
                    self.searching = false;
                }
            }
            Message::SuggestCompleted { gen, result } => {
                if gen != self.suggest_gen {
                    return iced::Task::none();
                }
                match result {
                    Ok(texts) => {
                        self.suggest = texts.into_iter().take(6).collect();
                    }
                    Err(_) => {
                        self.suggest.clear();
                    }
                }
            }
            Message::SearchDebounced { gen, query } => {
                // Stale keystroke (user kept typing): drop silently.
                if gen != self.search_gen {
                    return iced::Task::none();
                }
                let q = query.trim().to_string();
                if q.is_empty() || q.len() < 2 {
                    return iced::Task::none();
                }
                // Query changed while debouncing (Enter fired first): skip.
                if q != self.search_query.trim() {
                    return iced::Task::none();
                }
                #[cfg(feature = "innertube")]
                {
                    self.status = format!("Searching YouTube Music for “{q}”…");
                    self.searching = true;
                    self.publish_mpris();
                    // Repeat query (retype, Enter-after-popup): paint the
                    // cached results with no network round trip.
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    return Self::search_task(q);
                }
                #[cfg(not(feature = "innertube"))]
                {
                    // Cached queries still paint offline.
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                }
            }
            Message::SearchStay => {
                self.show_suggest = false;
                self.go(Route::Search);
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = "Type something to search YouTube Music.".into();
                    } else {
                        self.status = format!("Searching YouTube Music for “{q}”…");
                        self.searching = true;
                        self.publish_mpris();
                        if let Some(task) = self.search_cache_hit_task(&q) {
                            return task;
                        }
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                }
            }
            Message::SearchChanged(q) => {
                self.search_query = q.clone();
                self.results = filter_results(&self.search_base, &q);
                self.all_results = filter_all(&self.search_all_base, &q);
            }
            Message::Search => {
                let q = self.search_query.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if q.is_empty() {
                        self.status = "Type something to search YouTube Music.".into();
                    } else {
                        self.status = format!("Searching YouTube Music for “{q}”…");
                        self.searching = true;
                        self.publish_mpris();
                        if let Some(task) = self.search_cache_hit_task(&q) {
                            return task;
                        }
                        return Self::search_task(q);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    if let Some(task) = self.search_cache_hit_task(&q) {
                        return task;
                    }
                    self.status =
                        "Real search needs `--features innertube` (offline build).".into();
                    self.go(Route::Search);
                }
            }
            Message::SearchCompleted { query, result } => {
                self.searching = false;
                match result {
                    Ok(search) => {
                        self.search_cache_put(&query, (*search).clone());
                        // Out-of-order arrival (user typed on): keep the
                        // tracks in the catalog, but never overwrite the
                        // newer query's results.
                        if query.trim() == self.search_query.trim() {
                            return self.apply_search_hit(&query, *search);
                        }
                        let (_, tasks) = self.stage_search(*search);
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        // Stale-query errors stay silent; current ones report.
                        if query.trim() == self.search_query.trim() {
                            self.status = format!("Search failed: {e}");
                        }
                    }
                }
            }
            Message::NewPlaylistNameChanged(n) => {
                self.new_playlist_name = n;
            }
            Message::Pause => {
                if let Some(a) = &self.audio {
                    a.pause();
                    self.paused = true;
                    self.status = "Paused.".into();
                }
            }
            Message::TogglePause => {
                if let Some(task) = self.take_restored_play() {
                    return task;
                }
                if let Some(a) = &self.audio {
                    if self.paused {
                        a.resume();
                        self.paused = false;
                        self.paused_seek = false;
                        self.status = "Resumed.".into();
                    } else {
                        a.pause();
                        self.paused = true;
                        self.status = "Paused.".into();
                    }
                } else {
                    self.paused = !self.paused;
                    self.status = "No audio device.".into();
                }
            }
            Message::Stop => {
                if let Some(a) = &self.audio {
                    a.stop();
                    self.paused = false;
                    self.status = "Stopped.".into();
                }
            }
            Message::Next => {
                if self.advance_queue_manual() {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::Prev => {
                if self.queue.prev_track().is_some() {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::Like(id) => {
                // Local state applies first; the server push (if signed
                // in) only ever reports failures, so the row never lies.
                // Like and dislike are one status server-side: setting one
                // clears the other locally too.
                let liked: bool;
                if self.liked.contains(&id) {
                    self.unlike_id(&id);
                    self.persist_like(&id, false);
                    self.status = format!("Removed from favorites: {id}.");
                    liked = false;
                } else {
                    let known = self.find_track(&id).is_some();
                    self.like_id(&id);
                    self.persist_like(&id, true);
                    if self.disliked.contains(&id) {
                        self.undislike_id(&id);
                        self.persist_dislike(&id, false);
                    }
                    liked = true;
                    if known {
                        self.status = format!("Added to favorites: {id}.");
                    } else {
                        // Bare id with no metadata yet (older installs only
                        // stored ids): keep the like, fetch the info so the
                        // row appears without a manual search.
                        self.status = format!("Added to favorites: {id} (fetching info…).");
                        self.publish_mpris();
                        #[cfg(feature = "innertube")]
                        {
                            return iced::Task::batch(vec![
                                Self::hydrate_task(vec![id.clone()]),
                                Self::like_sync_task(id, true),
                            ]);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            return Self::hydrate_task(vec![id]);
                        }
                    }
                }
                self.publish_mpris();
                self.record_pending(if liked { "like" } else { "unlike" }, None, Some(&id), None);
                return Self::like_sync_task(id, liked);
            }
            Message::Dislike(id) => {
                // "Don't recommend": local dislike first, server push only
                // reports failures. Mutual exclusion with likes (see Like).
                let disliked: bool = if self.disliked.contains(&id) {
                    self.undislike_id(&id);
                    self.persist_dislike(&id, false);
                    self.status = format!("Removed don't-recommend for {id}.");
                    false
                } else {
                    self.dislike_id(&id);
                    self.persist_dislike(&id, true);
                    if self.liked.contains(&id) {
                        self.unlike_id(&id);
                        self.persist_like(&id, false);
                    }
                    self.status = format!("Won't recommend {id} again.");
                    true
                };
                self.publish_mpris();
                self.record_pending(
                    if disliked { "dislike" } else { "undislike" },
                    None,
                    Some(&id),
                    None,
                );
                return Self::dislike_sync_task(id, disliked);
            }
            Message::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(a) = &self.audio {
                    a.set_volume(self.volume);
                }
            }
            Message::QueueRemove(i) => {
                self.cancel_drag();
                self.queue.remove_at(i);
            }
            Message::QueueUp(i) => {
                self.cancel_drag();
                self.queue.move_up(i);
            }
            Message::QueueDown(i) => {
                self.cancel_drag();
                self.queue.move_down(i);
            }
            Message::QueueClear => {
                self.cancel_drag();
                self.queue.clear();
                self.status = if self.autoplay.is_empty() {
                    "Queue cleared.".into()
                } else {
                    format!("Queue cleared ({} autoplay kept).", self.autoplay.len())
                };
            }
            Message::OpenArtist(id) => {
                self.close_menu();
                self.go(Route::Artist(id));
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::OpenAlbum(id) => {
                self.close_menu();
                self.go(Route::Album(id));
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::OpenPlaylist(id) => {
                // Canonicalise: pre-migration sessions may hand a browse id.
                let id = yutu_core::canonical_playlist_id(&id);
                self.go(Route::PlaylistDetail(id));
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::OpenLyrics(id) => {
                self.close_menu();
                self.go(Route::Lyrics(id.clone()));
                #[cfg(feature = "lyrics")]
                {
                    let needs = !self.lyrics_cache.contains_key(&id)
                        && self.lyrics_loading.as_deref() != Some(id.as_str());
                    if needs {
                        // Downloaded lyrics open with no network.
                        if let Some(cached) = load_lyrics(&id) {
                            self.lyrics_cache.insert(id, cached);
                        } else {
                            match self.find_track(&id) {
                                None => {
                                    self.status = format!("Fetching info for {id}…");
                                    self.publish_mpris();
                                    return Self::hydrate_task(vec![id]);
                                }
                                Some(t) => {
                                    self.lyrics_loading = Some(id);
                                    self.publish_mpris();
                                    return Self::lyrics_task(t);
                                }
                            }
                        }
                    }
                }
            }
            Message::LyricsReady { track_id, result } => {
                // Stale arrivals must not clear a newer in-flight fetch.
                if self.lyrics_loading.as_deref() == Some(track_id.as_str()) {
                    self.lyrics_loading = None;
                }
                match result {
                    Ok(lyrics) => {
                        // Heal the download bundle when online: sidecars on
                        // disk exist only for downloaded tracks (keeps unit
                        // tests hermetic: no test marks fixtures downloaded
                        // before a LyricsReady).
                        if self.downloaded.contains(&track_id) {
                            save_lyrics(&track_id, &lyrics);
                        }
                        self.lyrics_cache.insert(track_id, lyrics);
                    }
                    Err(e) => {
                        self.status = format!("Lyrics failed: {e}");
                    }
                }
            }
            Message::ArtworkReady { url, result } => {
                self.artwork_loading.remove(&url);
                match &result {
                    Ok(bytes) if bytes.len() > 512 => {
                        Self::note_success(&mut self.artwork_failed, &url);
                    }
                    _ => {
                        // Tiny/errored bodies never decode: cool down so
                        // chained visible scans do not refetch in a loop.
                        Self::note_failed(&mut self.artwork_failed, url.clone());
                    }
                }
                if let Ok(bytes) = result {
                    if bytes.len() > 512 {
                        // Like lyrics: on-disk art exists only for
                        // downloaded tracks, so rows render from disk when
                        // the in-memory handle is gone.
                        if self.is_downloaded_art(&url) {
                            save_artwork(&url, &bytes);
                        }
                        self.artwork
                            .insert(url, iced::widget::image::Handle::from_bytes(bytes));
                    }
                }
                // Cover the rest of the screenful: the batch cap (12 tasks
                // for ~6 two-key tracks) leaves remainder that only an
                // arrival can pick up. Self-terminating: memory, in-flight,
                // disk, and cooldown gates make this a no-op when done.
                self.publish_mpris();
                return self.ensure_visible_tasks();
            }
            Message::StartRadio(seed) => {
                self.close_menu();
                match self.find_track(&seed) {
                    None => {
                        self.status = format!("Fetching info for {seed}…");
                        self.publish_mpris();
                        return Self::hydrate_task(vec![seed]);
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        #[cfg(feature = "innertube")]
                        {
                            self.status = format!("Starting radio for {} · {}…", t.title, t.artist);
                            self.go(Route::Radio(seed.clone()));
                            self.publish_mpris();
                            return Self::radio_task(seed, artist);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            self.status = "Radio needs `--features innertube`.".into();
                        }
                    }
                }
            }
            Message::RadioReady { seed, result } => match result {
                Ok(tracks) => {
                    let tracks: Vec<Track> = self
                        .register_tracks(tracks)
                        .into_iter()
                        .filter(|t| !self.disliked.contains(&t.id))
                        .collect();
                    let n = tracks.len();
                    self.radio_tracks = tracks.clone();
                    self.go(Route::Radio(seed.clone()));
                    let mut tasks = self.artwork_tasks(&tracks);
                    tasks.push(self.queue_many(tracks, &format!("Radio (seed {seed})")));
                    if n == 0 {
                        self.status = format!("Radio (seed {seed}): nothing found.");
                    }
                    self.publish_mpris();
                    return iced::Task::batch(tasks);
                }
                Err(e) => {
                    self.status = format!("Radio failed: {e}");
                }
            },
            Message::QueueTrack(id) => {
                self.show_suggest = false;
                self.close_menu();
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        self.absorb_autoplay(&t.id);
                        let was_empty = self.queue.is_empty();
                        self.queue.push(t.clone());
                        self.status = if was_empty {
                            format!("Added to queue: {} · {} (press play).", t.title, t.artist)
                        } else {
                            format!("Added to queue: {} · {}.", t.title, t.artist)
                        };
                    }
                }
            }
            Message::QueueJump(i) => {
                // Jumping to the live song is a no-op (never restarts it).
                if self.queue.position() == Some(i) {
                    return iced::Task::none();
                }
                // A reorder shares its release click with the row: eat it.
                if self
                    .just_reordered_at
                    .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150))
                {
                    self.just_reordered_at = None;
                    return iced::Task::none();
                }
                self.just_reordered_at = None;
                if self.queue.jump_to(i) {
                    self.apply_volume();
                    self.publish_mpris();
                    return self.play_current_task();
                }
            }
            Message::ToggleShuffle => {
                let on = !self.queue.shuffle;
                self.cancel_drag();
                self.set_shuffle(on);
                self.status = if on {
                    "Shuffle on: queue reordered.".into()
                } else {
                    "Shuffle off: original order back.".into()
                };
            }
            Message::CycleRepeat => {
                let mode = self.queue.cycle_repeat();
                self.status = format!("Repeat: {mode:?}.");
            }
            Message::PlayAll => return self.play_all_tracks(false),
            Message::PlayAllShuffled => return self.play_all_tracks(true),
            Message::PlayPlaylistAt { playlist_id, index } => {
                let Some(p) = self.find_playlist(&playlist_id) else {
                    self.status = "Playlist not found.".into();
                    return iced::Task::none();
                };
                let label = format!("Playlist {}", p.title);
                let tracks: Vec<Track> = p.tracks.into_iter().skip(index).collect();
                if tracks.is_empty() {
                    self.status = format!("{label}: nothing to play.");
                    return iced::Task::none();
                }
                // Fresh context like PlayNow: the old queue and tail go
                // away, then the playlist plays from the clicked song on.
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                self.drag = None;
                return self.queue_many(tracks, &label);
            }
            Message::PlayArtistShuffled(name) => {
                self.close_menu();
                let tracks = self.artist_tracks(&name);
                return self.shuffle_tracks(tracks, &format!("Artist {name}"));
            }
            Message::PlayAlbumShuffled(id) => {
                self.close_menu();
                let title = self.resolve_album_title(&id);
                let tracks = self.album_tracks(&id);
                return self.shuffle_tracks(tracks, &format!("Album {title}"));
            }
            Message::PlaySearchPlaylist(id) => {
                return self.play_search_playlist(id, false);
            }
            Message::PlaySearchPlaylistShuffled(id) => {
                return self.play_search_playlist(id, true);
            }
            Message::SearchPlaylistReady {
                id,
                label,
                shuffle,
                result,
            } => match result {
                Ok(tracks) => {
                    let tracks = self.register_tracks(tracks);
                    if tracks.is_empty() {
                        self.status = format!("Playlist {label}: nothing playable.");
                        return iced::Task::none();
                    }
                    return self.play_fresh(tracks, &format!("Playlist {label}"), shuffle);
                }
                Err(e) => {
                    self.status = format!("Playlist {label} ({id}): {e}.");
                }
            },
            Message::CreatePlaylist(title) => {
                let title = title.trim().to_string();
                if title.is_empty() {
                    self.status = "Playlist name is empty.".into();
                } else {
                    self.playlist_seq += 1;
                    let id = format!("u{}", self.playlist_seq);
                    self.user_playlists.push(Playlist {
                        id: id.clone(),
                        title: title.clone(),
                        tracks: Vec::new(),
                    });
                    self.persist_playlist_created(&id, &title);
                    self.record_pending("create", Some(&id), None, Some(&title));
                    self.new_playlist_name.clear();
                    self.status = format!("Created playlist {title} ({id}).");
                    self.publish_mpris();
                    // Signed in: mirror on YouTube Music, migrating the
                    // local shell to the server id on success.
                    return Self::playlist_create_sync_task(id, title);
                }
            }
            Message::AddToPlaylist {
                playlist_id,
                track_id,
            } => match self.find_track(&track_id) {
                None => {
                    return self.unknown_track_task(&track_id);
                }
                Some(track) => {
                    // Accept either stored form: older sessions kept browse
                    // (`VL...`) ids, storage is canonical (edit form) now.
                    let playlist_id = yutu_core::canonical_playlist_id(&playlist_id);
                    // Virtual Favorites maps to likes, never SQLite.
                    if playlist_id == Self::FAVORITES_ID {
                        if self.liked.contains(&track.id) {
                            self.status = format!("Already in Favorites: {}.", track.title);
                            self.publish_mpris();
                            return iced::Task::none();
                        }
                        self.like_id(&track.id);
                        self.persist_like_track(&track, true);
                        self.record_pending("like", None, Some(&track.id), None);
                        self.status = format!("Added {} to Favorites.", track.title);
                        self.publish_mpris();
                        return Self::like_sync_task(track.id.clone(), true);
                    }
                    // Virtual Downloaded maps to an offline download: the
                    // file must exist, so kick off the download flow.
                    if playlist_id == Self::DOWNLOADED_ID {
                        if self.downloaded.contains(&track.id) {
                            self.status = format!("Already in Downloaded: {}.", track.title);
                            self.publish_mpris();
                            return iced::Task::none();
                        }
                        self.close_menu();
                        self.status = format!("Downloading {} · {}…", track.title, track.artist);
                        self.publish_mpris();
                        return Self::download_task(track);
                    }
                    match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                        None => {
                            self.status = format!(
                                "Playlist {playlist_id} is built-in (read-only) or missing."
                            );
                        }
                        Some(i) => {
                            if self.user_playlists[i]
                                .tracks
                                .iter()
                                .any(|t| t.id == track.id)
                            {
                                self.status = format!(
                                    "Already in {}: {}.",
                                    self.user_playlists[i].title, track.title
                                );
                            } else {
                                let title = self.user_playlists[i].title.clone();
                                self.user_playlists[i].tracks.push(track.clone());
                                self.persist_playlist_added(&playlist_id, &track);
                                if Self::is_server_playlist(&playlist_id) {
                                    self.record_pending(
                                        "add",
                                        Some(&playlist_id),
                                        Some(&track.id),
                                        None,
                                    );
                                }
                                self.status = format!("Added {} to {}.", track.title, title);
                                self.publish_mpris();
                                // Server lists mirror on YouTube Music;
                                // local shells stay local-only.
                                return Self::playlist_add_sync_task(playlist_id, track.id.clone());
                            }
                        }
                    }
                }
            },
            Message::RemoveFromPlaylist {
                playlist_id,
                track_id,
            } => {
                // Same either-form rule as adds (see AddToPlaylist).
                let playlist_id = yutu_core::canonical_playlist_id(&playlist_id);
                // Virtual Favorites maps to unlikes, never SQLite.
                if playlist_id == Self::FAVORITES_ID {
                    if !self.liked.contains(&track_id) {
                        self.status = format!("{track_id} is not in Favorites.");
                        self.publish_mpris();
                        return iced::Task::none();
                    }
                    self.unlike_id(&track_id);
                    self.persist_like(&track_id, false);
                    self.record_pending("unlike", None, Some(&track_id), None);
                    self.status = format!("Removed {track_id} from Favorites.");
                    self.publish_mpris();
                    return Self::like_sync_task(track_id, false);
                }
                // Virtual Downloaded maps to deleting the offline bundle.
                if playlist_id == Self::DOWNLOADED_ID {
                    if self.downloaded.contains(&track_id) {
                        let track = self.find_track(&track_id);
                        self.unmark_downloaded_id(&track_id);
                        self.persist_downloaded(&track_id, false);
                        Self::remove_download_bundle(&track_id, track.as_ref());
                        self.status = format!("Removed {track_id} from Downloaded.");
                    } else {
                        self.status = format!("{track_id} is not in Downloaded.");
                    }
                    self.publish_mpris();
                    return iced::Task::none();
                }
                match self.user_playlists.iter().position(|p| p.id == playlist_id) {
                    None => {
                        self.status =
                            format!("Playlist {playlist_id} is built-in (read-only) or missing.");
                    }
                    Some(i) => {
                        let title = self.user_playlists[i].title.clone();
                        self.user_playlists[i].tracks.retain(|t| t.id != track_id);
                        self.persist_playlist_removed(&playlist_id, &track_id);
                        if Self::is_server_playlist(&playlist_id) {
                            self.record_pending(
                                "remove",
                                Some(&playlist_id),
                                Some(&track_id),
                                None,
                            );
                        }
                        self.status = format!("Removed {track_id} from {title}.");
                        self.publish_mpris();
                        // Server lists mirror the removal on YouTube Music
                        // (setVideoId resolved from raw browse JSON);
                        // local shells stay local-only.
                        return Self::playlist_remove_sync_task(playlist_id, track_id);
                    }
                }
            }
            Message::DeletePlaylist(id) => {
                self.close_menu();
                // Same either-form rule as adds (see AddToPlaylist).
                let id = yutu_core::canonical_playlist_id(&id);
                if id == Self::FAVORITES_ID {
                    self.status = "Favorites is built-in and can't be deleted.".into();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                if id == Self::DOWNLOADED_ID {
                    self.status = "Downloaded is built-in and can't be deleted.".into();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                let before = self.user_playlists.len();
                self.user_playlists.retain(|p| p.id != id);
                if self.user_playlists.len() < before {
                    self.persist_playlist_deleted(&id);
                    if Self::is_server_playlist(&id) {
                        self.record_pending("delete", Some(&id), None, None);
                    }
                    if self.route == Route::PlaylistDetail(id.clone()) {
                        self.go(Route::Library);
                    }
                    self.status = format!("Deleted playlist {id}.");
                    self.publish_mpris();
                    // Server lists mirror the delete on YouTube Music.
                    return Self::playlist_delete_sync_task(id);
                } else {
                    self.status = format!("Playlist {id} is built-in and can't be deleted.");
                }
            }
            Message::SelectEqPreset(name) => self.select_eq_preset(&name),
            Message::ScrobbleToggled(on) => {
                self.scrobble_enabled = on;
                self.status = if on {
                    "Scrobbling enabled (submits when credentials are set).".into()
                } else {
                    "Scrobbling disabled.".into()
                };
            }
            Message::ScrobbleUserChanged(u) => {
                self.scrobble_user = u;
            }
            Message::ScrobbleLbTokenChanged(t) => {
                self.scrobble_lb_token = t;
            }
            Message::ScrobbleFmKeyChanged(k) => {
                self.scrobble_fm_key = k;
            }
            Message::ScrobbleFmSecretChanged(s) => {
                self.scrobble_fm_secret = s;
            }
            Message::ScrobbleFmSkChanged(sk) => {
                self.scrobble_fm_sk = sk;
            }
            Message::PlayWeeklyMix => {
                let tracks = self.weekly_mix_tracks();
                if tracks.is_empty() {
                    self.status = "Weekly Mix: nothing to queue.".into();
                } else {
                    let task = self.queue_many(tracks, "Weekly Mix");
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayForgotten => {
                let tracks = self.forgotten_tracks();
                if tracks.is_empty() {
                    self.status = "Forgotten Favorites: nothing to queue.".into();
                } else {
                    let task = self.queue_many(tracks, "Forgotten Favorites");
                    self.publish_mpris();
                    return task;
                }
            }
            Message::PlayNow(id) => {
                self.close_menu();
                self.show_suggest = false;
                // Re-clicking the live song is a no-op (never restarts it).
                if self.queue.current().is_some_and(|t| t.id == id) {
                    return iced::Task::none();
                }
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        let artist = t.artist.clone();
                        self.queue.clear();
                        self.queue.push(t);
                        self.queue.jump_to(0);
                        self.playing_from = "Single + autoplay".into();
                        self.autoplay_for = Some(id.clone());
                        self.autoplay.clear();
                        self.history.push(id.clone());
                        self.persist_play(&id);
                        self.apply_volume();
                        self.publish_mpris();
                        let play = self.play_current_task();
                        #[cfg(feature = "innertube")]
                        {
                            let auto = Self::autoplay_task(id, artist);
                            return iced::Task::batch([play, auto]);
                        }
                        #[cfg(not(feature = "innertube"))]
                        {
                            let _ = artist;
                            return play;
                        }
                    }
                }
            }
            Message::AutoplayReady { seed, result } => {
                match result {
                    Err(e) => {
                        // Quiet: the tail is best-effort, but keep the log.
                        self.log_error(format!("Autoplay for {seed} failed: {e}"));
                    }
                    Ok(tracks) => {
                        let tracks = self.register_tracks(tracks);
                        // Stale (user moved on): catalog only, no append.
                        if self.autoplay_for.as_deref() != Some(seed.as_str()) {
                            return iced::Task::none();
                        }
                        let known: std::collections::HashSet<String> = self
                            .queue
                            .items()
                            .iter()
                            .chain(self.autoplay.iter())
                            .map(|t| t.id.clone())
                            .collect();
                        let fresh: Vec<Track> = tracks
                            .into_iter()
                            .filter(|t| {
                                t.id != seed
                                    && !known.contains(&t.id)
                                    && !self.disliked.contains(&t.id)
                            })
                            .take(10)
                            .collect();
                        if fresh.is_empty() {
                            return iced::Task::none();
                        }
                        let art = self.artwork_tasks(&fresh);
                        self.autoplay.extend(fresh);
                        self.publish_mpris();
                        return iced::Task::batch(art);
                    }
                }
            }
            Message::PlayNext(id) => {
                self.close_menu();
                self.show_suggest = false;
                self.cancel_drag();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        self.absorb_autoplay(&t.id);
                        if self.queue.is_empty() {
                            self.queue.push(t);
                            self.queue.jump_to(0);
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        let at = self.queue.insert_next(t.clone());
                        self.status = format!("Queued next: {} · {}.", t.title, t.artist);
                        let _ = at;
                    }
                }
            }
            Message::PlayAutoplay(i) => {
                self.close_menu();
                self.cancel_drag();
                match self.autoplay.get(i).cloned() {
                    None => {
                        self.status = format!("Nothing at autoplay index {i}.");
                    }
                    Some(t) => {
                        self.autoplay.remove(i);
                        self.queue.push(t);
                        self.queue.jump_to(self.queue.len().saturating_sub(1));
                        self.apply_volume();
                        self.publish_mpris();
                        return self.play_current_task();
                    }
                }
            }
            Message::RemoveFromQueue(id) => {
                self.close_menu();
                self.cancel_drag();
                let was_current = self.queue.current().is_some_and(|t| t.id == id);
                if self.queue.remove_id(&id) {
                    self.status = format!("Removed {id} from the queue.");
                    if was_current {
                        // The ghost must not keep playing: move on, or stop.
                        if self.advance_queue() {
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                        if let Some(a) = &self.audio {
                            a.stop();
                        }
                        self.position_secs = 0.0;
                        self.loading_track = None;
                    }
                } else {
                    self.status = format!("Not in queue: {id}.");
                }
            }
            Message::RemoveAutoplay(i) => {
                self.close_menu();
                if i < self.autoplay.len() {
                    let t = self.autoplay.remove(i);
                    self.status = format!("Removed {} from autoplay.", t.title);
                } else {
                    self.status = format!("Nothing at autoplay index {i}.");
                }
            }
            Message::QueueHover(h) => {
                // Modal open: rows behind scrims never highlight.
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_queue = None;
                } else {
                    self.hovered_queue = h;
                }
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
            }
            Message::TrackHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_track = None;
                } else {
                    self.hovered_track = h;
                }
                // Entering a non-queue row drops stale queue hovers so a
                // press there never arms a drag off old state.
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
            }
            Message::AlbumHover(h) => {
                self.hovered_album = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::ArtistHover(h) => {
                self.hovered_artist = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_album = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::SearchPlaylistHover(h) => {
                self.hovered_search_playlist = self.coll_hover(h);
                self.hovered_track = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::ClearHovers => {
                self.hovered_track = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::RemoveHover(h) => {
                self.hover_remove = h;
            }
            Message::PlaylistHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_playlist = None;
                } else {
                    self.hovered_playlist = h;
                }
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
            }
            Message::AutoHover(h) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_auto = None;
                } else {
                    self.hovered_auto = h;
                }
                self.hovered_queue = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
            }
            Message::PanelHover(i) => {
                if self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest
                {
                    self.hovered_queue = None;
                } else {
                    self.hovered_queue = Some(i);
                }
                self.hovered_auto = None;
                self.hovered_album = None;
                self.hovered_artist = None;
                self.hovered_search_playlist = None;
            }
            Message::DragStart(from) => {
                // Two-row lists reorder via the trailing edge slot, so
                // drags start at two rows and up. Never from the remove
                // button: slipping off delete must not reorder.
                if from < self.queue.len()
                    && self.queue.len() >= 2
                    && self.hover_remove != Some(from)
                {
                    self.drag = Some(DragState { from, slot: None });
                    self.press_snap = Some(PressSnap {
                        from,
                        pos: self.cursor,
                        id: self
                            .queue
                            .items()
                            .get(from)
                            .map(|t| t.id.clone())
                            .unwrap_or_default(),
                        len: self.queue.len(),
                    });
                }
            }
            Message::GlobalPress => {
                // Arm from the hovered queue row: one press path for text,
                // art, and gaps (only the remove button owns its press).
                // Queue-page buttons coexist order-independently (a shared
                // release either jumps once or no-ops).
                if self.drag.is_none() && self.queue.len() >= 2 {
                    if let Some(i) = self.hovered_queue {
                        if i < self.queue.len() && self.hover_remove != Some(i) {
                            self.drag = Some(DragState {
                                from: i,
                                slot: None,
                            });
                            self.press_snap = Some(PressSnap {
                                from: i,
                                pos: self.cursor,
                                id: self
                                    .queue
                                    .items()
                                    .get(i)
                                    .map(|t| t.id.clone())
                                    .unwrap_or_default(),
                                len: self.queue.len(),
                            });
                        }
                    }
                }
            }
            Message::DragSlot(k) => {
                // Slot strips only exist while dragging (edges included);
                // anything else is a stale hover (no-op).
                if let Some(drag) = self.drag.as_mut() {
                    if k <= self.queue.len() {
                        drag.slot = Some(k);
                    }
                }
            }
            Message::DragEnd => {
                let snap = self.press_snap.take();
                if let Some(drag) = self.drag.take() {
                    // No move plus a clean press/release over the same
                    // unmutated row: the whole row clicks (plays it).
                    // Snapshot-guarded on both paths: a queue mutated
                    // mid-press (remove, play-all, mix) never jumps or
                    // reorders off stale indices.
                    let snap_ok = snap.as_ref().is_some_and(|s| {
                        s.from == drag.from
                            && self.queue.len() == s.len
                            && self.queue.items().get(s.from).is_some_and(|t| t.id == s.id)
                    });
                    let mut committed = false;
                    if snap_ok {
                        if let Some(slot) = drag.slot {
                            if let Some(to) = slot_commit(self.queue.len(), drag.from, slot) {
                                if self.queue.move_to(drag.from, to) {
                                    self.status = "Queue reordered.".into();
                                    self.just_reordered_at = Some(std::time::Instant::now());
                                    committed = true;
                                }
                            }
                        }
                    }
                    let modal = self.context_menu.is_some()
                        || self.pl_menu.is_some()
                        || self.coll_menu.is_some()
                        || self.show_suggest;
                    let click = !committed
                        && !modal
                        && snap.as_ref().is_some_and(|s| {
                            snap_ok
                                && !self.just_reordered_at.is_some_and(|at| {
                                    at.elapsed() < std::time::Duration::from_millis(150)
                                })
                                && release_is_click_at(s.pos, self.cursor)
                                && self.hovered_queue == Some(s.from)
                                && self.queue.position() != Some(s.from)
                        });
                    if click {
                        let from = snap.as_ref().map(|s| s.from).unwrap_or(0);
                        if self.queue.jump_to(from) {
                            self.apply_volume();
                            self.publish_mpris();
                            return self.play_current_task();
                        }
                    }
                }
            }
            Message::Animate => {
                if !self.animations_enabled {
                    self.sidebar_w = self.sidebar_target();
                    self.right_w = self.right_target();
                    self.drawer_anim = if self.drawer_open { 1.0 } else { 0.0 };
                    self.drag_gap = self.drag_gap_target();
                    for (_, o) in self.scroll_hint.values_mut() {
                        *o = 0.0;
                    }
                } else {
                    // Duration-based steps with a 1px floor: each glide
                    // finishes near the slider ms, and short nudges still
                    // move at least a pixel per tick.
                    let (sidebar_step, right_step, unit_step) = anim_steps(self.anim_ms);
                    self.sidebar_w =
                        ease_toward(self.sidebar_w, self.sidebar_target(), sidebar_step);
                    self.right_w = ease_toward(self.right_w, self.right_target(), right_step);
                    let target = if self.drawer_open { 1.0 } else { 0.0 };
                    self.drawer_anim = ease_toward(self.drawer_anim, target, unit_step);
                    self.drag_gap = ease_toward(self.drag_gap, self.drag_gap_target(), unit_step);
                    // Scrollbar fades ease out after their idle delay (a
                    // real transition: bars light on motion, then fade).
                    for (at, o) in self.scroll_hint.values_mut() {
                        if *o > 0.0
                            && at.elapsed() > std::time::Duration::from_millis(SCROLL_FADE_DELAY_MS)
                        {
                            *o = (*o - SCROLL_FADE_STEP).max(0.0);
                        }
                    }
                }
            }
            Message::ToggleDrawer => {
                self.drawer_open = !self.drawer_open;
                if self.drawer_open {
                    self.drawer_scroll = 0.0;
                }
                // Warm the hi-res hero and the lyrics on open: the drawer
                // is the lyrics home, but no fetch trigger fires for it.
                if self.drawer_open {
                    if let Some(t) = self.queue.current().cloned() {
                        let tasks = self.track_start_tasks(&t);
                        if !tasks.is_empty() {
                            return iced::Task::batch(tasks);
                        }
                    }
                }
            }
            Message::DrawerScrolled(y) => {
                self.drawer_scroll = y.max(0.0);
                self.lit_scroll("drawer");
            }
            Message::Scrolled(key) => {
                self.lit_scroll(key);
            }
            Message::OpenContext(id) => {
                self.show_suggest = false;
                self.open_menu(id);
            }
            Message::TogglePlaylistSubmenu(id) => {
                // Click-toggle: hover-open fights the frozen menu anchor.
                // Only the open menu's own track can own the flyout.
                if self.context_menu.as_deref() != Some(id.as_str()) {
                    return iced::Task::none();
                }
                if self.submenu_playlists_for.as_deref() == Some(id.as_str()) {
                    self.submenu_playlists_for = None;
                } else {
                    self.submenu_playlists_for = Some(id);
                }
            }
            Message::OpenPlaylistMenu(id) => {
                self.open_pl_menu(id);
            }
            Message::OpenAlbumMenu(id) => {
                self.open_coll_for(CollectionMenu::Album(id));
            }
            Message::OpenArtistMenu(id) => {
                self.open_coll_for(CollectionMenu::Artist(id));
            }
            Message::OpenSearchPlaylistMenu(id) => {
                self.open_coll_for(CollectionMenu::Playlist(id));
            }
            Message::PlayPlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = format!("Playlist not found: {id}.");
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = format!("Playlist {}: nothing to queue.", p.title);
                    return iced::Task::none();
                }
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                self.cancel_drag();
                let title = p.title.clone();
                // queue_many resolves each row against the catalog itself.
                let task = self.queue_many(p.tracks.clone(), &format!("Playlist {title}"));
                self.publish_mpris();
                return task;
            }
            Message::QueuePlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = format!("Playlist not found: {id}.");
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = format!("Playlist {}: nothing to queue.", p.title);
                    return iced::Task::none();
                }
                let title = p.title.clone();
                let n = p.tracks.len();
                let was_empty = self.queue.is_empty();
                let incoming: std::collections::HashSet<&str> =
                    p.tracks.iter().map(|t| t.id.as_str()).collect();
                self.autoplay.retain(|t| !incoming.contains(t.id.as_str()));
                // Resolve in memory first, persist once: per-track SQLite
                // traffic freezes the GUI on long playlists.
                let resolved: Vec<Track> = p
                    .tracks
                    .iter()
                    .map(|t| self.resolve_for_play_mem(t))
                    .collect();
                self.cache_tracks(&resolved);
                for rt in resolved {
                    self.queue.push(rt);
                }
                self.status = format!("Playlist {title}: queued {n} track(s).");
                self.publish_mpris();
                if was_empty {
                    self.queue.jump_to(0);
                    self.apply_volume();
                    return self.play_current_task();
                }
                return iced::Task::none();
            }
            Message::ShufflePlaylist(id) => {
                self.close_menu();
                self.show_suggest = false;
                let Some(p) = self.find_playlist(&id) else {
                    self.status = format!("Playlist not found: {id}.");
                    return iced::Task::none();
                };
                if p.tracks.is_empty() {
                    self.status = format!("Playlist {}: nothing to queue.", p.title);
                    return iced::Task::none();
                }
                let title = p.title.clone();
                // Same path as the page Shuffle pill: fully shuffled with
                // a random head, always reshuffled.
                self.cancel_drag();
                return self.play_fresh(p.tracks.clone(), &format!("Playlist {title}"), true);
            }
            Message::CursorMoved(p) => {
                self.cursor = Some(p);
                // Modal freeze: hover latch never glides behind menus.
                let modal = self.context_menu.is_some()
                    || self.pl_menu.is_some()
                    || self.coll_menu.is_some()
                    || self.show_suggest;
                // Latched hover hysteresis: expand on rail/edge entry, stay
                // open anywhere inside panel + grace, collapse only outside.
                // Y-gated above the player zone so the bottom strip never
                // expands the rails.
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                let above_player = p.y < wh - self.player_zone_h();
                if !self.sidebar_pinned && !modal {
                    if p.x < 72.0 && above_player {
                        self.sidebar_latched = true;
                    } else if p.x >= self.sidebar_w + SIDEBAR_GRACE || !above_player {
                        self.sidebar_latched = false;
                    }
                }
                if !self.right_pinned && !modal {
                    let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                    if p.x > ww - RIGHT_RAIL_W - RIGHT_EXPAND_GRACE && above_player {
                        self.right_latched = true;
                    } else if p.x <= ww - self.right_w - RIGHT_GRACE || !above_player {
                        self.right_latched = false;
                    }
                }
                return iced::Task::none();
            }
            Message::WindowResized(sz) => {
                self.window = Some(sz);
                return iced::Task::none();
            }
            Message::CloseContext => {
                // Topmost first: menus, popup, drag, drawer. Menus sit
                // above the popup in the layer stack.
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.coll_menu.is_some()
                {
                    self.close_menu();
                } else if self.show_suggest {
                    self.show_suggest = false;
                } else if self.drag.is_some() {
                    // Esc cancels an in-progress drag first (it owns the
                    // pointer); the drawer goes on the next Esc.
                    self.cancel_drag();
                } else if self.drawer_open {
                    self.drawer_open = false;
                }
            }
            Message::CloseSuggest => {
                self.show_suggest = false;
            }
            Message::OpenTrackAlbum(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => match t.album.clone() {
                        Some(album) => self.go(Route::Album(album)),
                        None => {
                            self.status = format!("No album for {} · {}.", t.title, t.artist);
                        }
                    },
                    None => {
                        return self.unknown_track_task(&id);
                    }
                }
            }
            Message::OpenTrackArtist(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => self.go(Route::Artist(t.artist.clone())),
                    None => {
                        return self.unknown_track_task(&id);
                    }
                }
            }
            Message::DownloadTrack(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    None => {
                        return self.unknown_track_task(&id);
                    }
                    Some(t) => {
                        self.status = format!("Downloading {} · {}…", t.title, t.artist);
                        self.publish_mpris();
                        return Self::download_task(t);
                    }
                }
            }
            Message::Downloaded { track_id, result } => match result {
                Ok(()) => {
                    // The bytes are on disk: join the Downloaded list and
                    // keep the metadata cache warm so it renders after a
                    // restart with no search.
                    if let Some(t) = self.find_track(&track_id) {
                        let t = t.clone();
                        self.mark_downloaded_id(&t.id);
                        self.persist_downloaded_track(&t, true);
                        self.status = format!(
                            "Downloaded {} · {} for offline listening.",
                            t.title, t.artist
                        );
                    } else {
                        self.mark_downloaded_id(&track_id);
                        self.persist_downloaded(&track_id, true);
                        self.status = format!("Downloaded {track_id} for offline listening.");
                    }
                    self.publish_mpris();
                }
                Err(e) => {
                    let msg = format!("Download failed for {track_id}: {e}");
                    self.log_error(msg.clone());
                    self.status = msg;
                }
            },
            Message::ToggleSidebarPin => {
                self.sidebar_pinned = !self.sidebar_pinned;
                // Recompute the latch from the current cursor so unpinning
                // with the cursor far away collapses immediately. Y-gated
                // like CursorMoved so the player strip never expands.
                let zone = self.player_zone_h();
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                self.sidebar_latched = self.sidebar_pinned
                    || self.cursor.is_some_and(|p| p.x < 72.0 && p.y < wh - zone);
            }
            Message::ToggleRightPin => {
                self.right_pinned = !self.right_pinned;
                let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
                let wh = self.window.map(|s| s.height).unwrap_or(800.0);
                let zone = self.player_zone_h();
                self.right_latched = self.right_pinned
                    || self.cursor.is_some_and(|p| {
                        p.x > ww - RIGHT_RAIL_W - RIGHT_EXPAND_GRACE && p.y < wh - zone
                    });
            }
            Message::AnimationsToggled(on) => {
                self.animations_enabled = on;
            }
            Message::AnimMsChanged(ms) => {
                self.anim_ms = (ms as u64).clamp(ANIM_MIN_MS, ANIM_MAX_MS);
            }
            Message::SidebarFilterChanged(f) => {
                self.sidebar_filter = f;
            }
            Message::CookiePathChanged(p) => {
                self.cookie_path = p;
            }
            Message::CookieImport => {
                let path = self.cookie_path.trim().to_string();
                #[cfg(feature = "innertube")]
                {
                    if path.is_empty() {
                        self.status =
                            "Pick your exported cookies file first (see the note below).".into();
                    } else {
                        self.login_busy = true;
                        self.import_active = true;
                        self.status = "Reading cookies + contacting YouTube Music…".into();
                        self.publish_mpris();
                        return Self::cookie_import_task(path, self.library_path.clone());
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = path;
                    self.status =
                        "Cookie import needs `--features innertube` (offline build).".into();
                }
            }
            #[cfg(feature = "innertube")]
            Message::ImportStageReady { kind, result } => match result {
                Ok(staged) => {
                    return self.begin_playlist_stages(kind, staged);
                }
                Err(e) => {
                    self.login_busy = false;
                    self.import_active = false;
                    // A dead saved session must not keep posing as signed
                    // in: drop it so the panel goes back to the file picker.
                    if matches!(kind, ImportKind::Cookie | ImportKind::Sync)
                        && e.contains("saved cookie session")
                    {
                        let _ = std::fs::remove_file(cookie_session_path());
                        self.cookie_logged_in = false;
                    }
                    self.status = if kind == ImportKind::Sync {
                        format!("Sync failed: {e}")
                    } else {
                        format!("{} import failed: {e}", kind.label())
                    };
                }
            },
            #[cfg(feature = "innertube")]
            Message::PlaylistImported {
                kind,
                gen,
                title,
                index,
                total,
                count,
                result,
            } => {
                // Stragglers from an older run (re-import, sign-out):
                // ignore, never merge, never touch the counters.
                if gen != self.import_gen {
                    return iced::Task::none();
                }
                // A newer run re-staged while this list was in flight:
                // same treatment (totals belong to the new run).
                if total != self.import_total {
                    return iced::Task::none();
                }
                let _ = index;
                self.import_done += 1;
                let done = self.import_done;
                // This list leaves the live set: its songs now count via
                // Completed songs already count via `import_songs`;
                // failures drop their share out of the expected total
                // (so the bar can still reach 100%). A Done that
                // beat its ticks here still carries the details count, so
                // the expected total stays exact in every ordering.
                if let Some(live) = self.import_live.remove(&index) {
                    if result.is_err() {
                        if let Some(c) = live.count {
                            self.import_expected = self.import_expected.saturating_sub(c);
                        }
                    }
                } else if let Some(c) = count {
                    self.import_expected += c;
                }
                self.import_finished.insert(index);
                let finish = |s: &mut Self| {
                    s.login_busy = false;
                    s.import_active = false;
                    s.mark_synced();
                    if kind == ImportKind::Cookie {
                        s.cookie_logged_in = cookie_session_exists();
                    }
                };
                match result {
                    Ok(playlist) => {
                        let n = playlist.tracks.len();
                        self.import_songs += n;
                        let tasks = self.merge_imported(Vec::new(), Vec::new(), vec![playlist]);
                        if done >= total {
                            finish(self);
                            self.status = format!(
                                "{} complete: {} songs ({} liked), {total} playlists.",
                                kind.done_label(),
                                self.import_songs,
                                self.import_likes,
                            );
                        } else {
                            self.status =
                                format!("Playlist '{title}' done ({n} tracks) - {done}/{total}.");
                        }
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        if done >= total {
                            finish(self);
                            // "failures" keeps the release status filter
                            // (`status_text` shows only lines with "fail").
                            self.status = format!(
                                "{} complete with failures: {} songs ({} liked), {done}/{total} playlists.",
                                kind.done_label(),
                                self.import_songs,
                                self.import_likes,
                            );
                        } else {
                            self.status =
                                format!("Playlist '{title}' failed ({e}) - {done}/{total}.");
                        }
                        self.publish_mpris();
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::PlaylistTick {
                gen,
                index,
                arrived,
                count,
            } => {
                // Quarantine stale or already-finished arrivals; ticks only
                // move banner counters, never merge data (no publish: the
                // status text is untouched, so widgets see no change).
                if gen != self.import_gen || self.import_finished.contains(&index) {
                    return iced::Task::none();
                }
                match self.import_live.get_mut(&index) {
                    Some(live) => {
                        live.arrived = arrived;
                        if live.count.is_none() {
                            live.count = count;
                        }
                    }
                    None => {
                        eprintln!("[yutu-import] playlist {index}: {arrived} songs arrived");
                        if let Some(c) = count {
                            self.import_expected += c;
                        }
                        self.import_live
                            .insert(index, PlaylistLive { arrived, count });
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::CookieSessionImport => {
                if !cookie_session_exists() {
                    self.status = "Cookie session is gone: import your cookies file again.".into();
                } else {
                    self.login_busy = true;
                    self.import_active = true;
                    self.status = "Importing your library…".into();
                    self.publish_mpris();
                    return Self::cookie_session_import_task(self.library_path.clone());
                }
            }
            #[cfg(feature = "innertube")]
            Message::CookieLogout => {
                // Orphan in-flight playlist arrivals (generation bump).
                self.import_gen += 1;
                self.import_done = 0;
                self.import_total = 0;
                self.import_active = false;
                self.import_expected = 0;
                self.import_shown = 0;
                self.import_live.clear();
                self.import_finished.clear();
                if cookie_session_exists() {
                    match std::fs::remove_file(cookie_session_path()) {
                        Ok(()) => {
                            self.cookie_logged_in = false;
                            self.status = "Cookie session cleared.".into();
                        }
                        Err(e) => {
                            self.status = format!("Sign-out failed: {e}");
                        }
                    }
                } else {
                    self.cookie_logged_in = false;
                    self.status = "Cookie session cleared.".into();
                }
            }
            #[cfg(feature = "innertube")]
            Message::LikeSynced {
                track_id,
                liked,
                result,
            } => match result {
                Ok(()) => self.clear_pending_rate(if liked { "like" } else { "unlike" }, &track_id),
                Err(e) => {
                    let verb = if liked { "like" } else { "unlike" };
                    self.status =
                        format!("YouTube {verb} of {track_id} failed (kept locally): {e}");
                }
            },
            #[cfg(feature = "innertube")]
            Message::DislikeSynced {
                track_id,
                disliked,
                result,
            } => match result {
                Ok(()) => self
                    .clear_pending_rate(if disliked { "dislike" } else { "undislike" }, &track_id),
                Err(e) => {
                    let verb = if disliked {
                        "don't-recommend"
                    } else {
                        "clear-dislike"
                    };
                    self.status =
                        format!("YouTube {verb} of {track_id} failed (kept locally): {e}");
                }
            },
            #[cfg(feature = "innertube")]
            Message::ServerPlaylistCreated {
                local_id,
                title,
                result,
            } => match result {
                Ok(server_id) => {
                    // The create row clears first (by shell id), then any
                    // add/remove rows recorded mid-flight move shell id to
                    // server id for the flush below.
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ = store.clear_pending("create", Some(&local_id), None);
                        let _ = store.repoint_pending(&local_id, &server_id);
                    }
                    // Migrate the local shell to the real server id, in
                    // memory and in SQLite, so later edits sync. Tracks
                    // added while the create was in flight ride along:
                    // they flush to the server id below.
                    let Some(pos) = self.user_playlists.iter().position(|p| p.id == local_id)
                    else {
                        self.status = format!(
                            "Created {title} on YouTube ({server_id}): the local list was removed."
                        );
                        return iced::Task::none();
                    };
                    let pl = self.user_playlists.remove(pos);
                    if self.route == Route::PlaylistDetail(local_id.clone()) {
                        self.go(Route::PlaylistDetail(server_id.clone()));
                    }
                    // Import race: fold the shell rows into the existing
                    // server list instead of duplicating it.
                    let merged = match self.user_playlists.iter().find(|p| p.id == server_id) {
                        Some(existing) => {
                            let mut m = existing.clone();
                            for t in &pl.tracks {
                                if !m.tracks.iter().any(|x| x.id == t.id) {
                                    m.tracks.push(t.clone());
                                }
                            }
                            m
                        }
                        None => Playlist {
                            id: server_id.clone(),
                            title: pl.title.clone(),
                            tracks: pl.tracks,
                        },
                    };
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ =
                            store.sync_server_playlist(&server_id, &merged.title, &merged.tracks);
                        let _ = store.delete_playlist(&local_id);
                    }
                    match self.user_playlists.iter().position(|p| p.id == server_id) {
                        Some(i) => self.user_playlists[i] = merged.clone(),
                        None => self
                            .user_playlists
                            .insert(pos.min(self.user_playlists.len()), merged.clone()),
                    }
                    self.status = format!("Created playlist {} (synced to YouTube).", merged.title);
                    self.publish_mpris();
                    // Flush any in-flight rows to the server id (failures
                    // report per track, the local rows are kept).
                    return iced::Task::batch(
                        merged
                            .tracks
                            .iter()
                            .map(|t| Self::playlist_add_sync_task(server_id.clone(), t.id.clone()))
                            .collect::<Vec<_>>(),
                    );
                }
                Err(e) => {
                    self.status = format!("Created {title} locally (YouTube sync failed: {e})");
                }
            },
            #[cfg(feature = "innertube")]
            Message::ServerPlaylistSynced { detail, op, result } => match result {
                Ok(()) => {
                    let (op_name, pid, tid) = match &op {
                        PlaylistSyncOp::Add {
                            playlist_id,
                            track_id,
                        } => ("add", Some(playlist_id.as_str()), Some(track_id.as_str())),
                        PlaylistSyncOp::Remove {
                            playlist_id,
                            track_id,
                        } => (
                            "remove",
                            Some(playlist_id.as_str()),
                            Some(track_id.as_str()),
                        ),
                        PlaylistSyncOp::Delete { playlist_id } => {
                            ("delete", Some(playlist_id.as_str()), None)
                        }
                    };
                    if let Ok(store) = LibraryStore::open(&self.library_path) {
                        let _ = store.clear_pending(op_name, pid, tid);
                    }
                }
                Err(e) => {
                    self.status = format!("{detail} (kept locally): {e}");
                }
            },
            #[cfg(feature = "innertube")]
            Message::GoogleLoginStart => {
                self.login_busy = true;
                self.status = "Contacting Google for a device code…".into();
                self.publish_mpris();
                return Self::google_begin_task();
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginBegan { result } => {
                self.login_busy = false;
                match result {
                    Ok(login) => {
                        // Terminal copy: the URL is clickable there; the
                        // Settings panel shows it as text (Iced copy TBA).
                        eprintln!(
                            "[yutu-login] open {} and enter {}",
                            login.verification_url, login.user_code
                        );
                        self.google_login = Some(login);
                        self.status =
                            "Approve in your browser, then press “I’ve approved: check”.".into();
                    }
                    Err(e) => {
                        self.status = format!("Sign-in failed: {e}");
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPoll => match self.google_login.clone() {
                None => {
                    self.status = "Press “Sign in with Google” first.".into();
                }
                Some(login) => {
                    self.login_busy = true;
                    self.status = "Checking approval…".into();
                    self.publish_mpris();
                    return Self::google_poll_task(login);
                }
            },
            #[cfg(feature = "innertube")]
            Message::GoogleLoginPolled { result } => {
                self.login_busy = false;
                match result {
                    Ok(Some(persist)) => {
                        match persist.save_to_file(std::path::Path::new(&oauth_path())) {
                            Ok(()) => {
                                self.logged_in = true;
                                self.google_login = None;
                                self.login_busy = true;
                                self.import_active = true;
                                self.status = "Signed in: importing your library…".into();
                                self.publish_mpris();
                                return Self::library_import_task(self.library_path.clone());
                            }
                            Err(e) => {
                                self.status = format!("Sign-in failed: {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        self.status =
                            "Still waiting: approve in your browser, then press Check again."
                                .into();
                    }
                    Err(e) => {
                        if e.contains("expired") {
                            self.google_login = None;
                            self.status =
                                "Code expired: press “Sign in with Google” to restart.".into();
                        } else {
                            self.status = format!("Check failed: {e}");
                        }
                    }
                }
            }
            #[cfg(feature = "innertube")]
            Message::GoogleLogout => {
                let _ = std::fs::remove_file(oauth_path());
                // Orphan in-flight playlist arrivals (generation bump).
                self.import_gen += 1;
                self.import_done = 0;
                self.import_total = 0;
                self.import_active = false;
                self.import_expected = 0;
                self.import_shown = 0;
                self.import_live.clear();
                self.import_finished.clear();
                self.logged_in = false;
                self.google_login = None;
                self.status = "Signed out (token file removed).".into();
            }
            #[cfg(feature = "innertube")]
            Message::LibraryImport => {
                if !self.logged_in {
                    self.status = "Sign in with Google first.".into();
                } else {
                    self.login_busy = true;
                    self.import_active = true;
                    self.status = "Reading library + contacting YouTube Music…".into();
                    self.publish_mpris();
                    return Self::library_import_task(self.library_path.clone());
                }
            }
            #[cfg(feature = "innertube")]
            Message::SyncNow => {
                return self.start_sync(false);
            }
            #[cfg(feature = "innertube")]
            Message::AutoSyncToggled(on) => {
                self.auto_sync = on;
                if let Ok(store) = LibraryStore::open(&self.library_path) {
                    let _ = store.set_pref("auto_sync", if on { "1" } else { "0" });
                }
                self.status = if on {
                    "Auto-sync on (every 15 min while signed in).".into()
                } else {
                    "Auto-sync off.".into()
                };
            }
            Message::Tick(now) => {
                // Desktop transport commands first (widget / media keys).
                let mut tasks = vec![self.drain_player_commands()];
                // First frame after launch: lazily fetch only the Home
                // screenful (cached titles + disk covers already painted).
                // Startup itself does zero network so the window opens fast.
                if !self.lazy_init_done {
                    self.lazy_init_done = true;
                    tasks.push(self.ensure_visible_tasks());
                }
                // Position clock: manual advance while a user seek settles
                // (the backend reports the stale pre-seek position for a
                // tick or two: that stale tick is the lyrics flicker).
                let dt = self
                    .last_tick
                    .map(|last| now.duration_since(last).as_secs_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 2.0);
                self.last_tick = Some(now);
                // Session saver: queue changes land within a tick, the
                // playing position refreshes every 30s.
                self.maybe_persist_queue(now);
                if self.seek_hold_until.is_some_and(|until| now < until) {
                    if !self.paused {
                        self.position_secs = (self.position_secs + dt).max(0.0);
                    }
                    // Paused: freeze the user-set position either way.
                } else if self.paused && self.paused_seek {
                    // Seek landed while paused and the backend never caught
                    // up: keep the manual position until resume.
                } else {
                    self.seek_hold_until = None;
                    self.paused_seek = false;
                    self.position_secs = self.audio_position();
                }
                let duration = self.audio_duration();
                // Prefetch the next song while this one still plays (>75%).
                // One attempt per track: failures stay silent until advance.
                if !self.paused
                    && duration > 30.0
                    && self.prefetched.is_none()
                    && self.loading_track.is_none()
                    && self.position_secs > duration * 0.75
                {
                    if let Some(id) = self.peek_next_id() {
                        if self.prefetch_attempted.as_deref() != Some(id.as_str()) {
                            self.prefetch_attempted = Some(id.clone());
                            self.publish_mpris();
                            tasks.push(Self::prefetch_task(id));
                        }
                    }
                }
                // Auto-advance once per track end. The threshold sits 1s
                // before the reported duration: mpv's time-pos can hold just
                // short of it, and the rodio wall-clock can overshoot, so
                // both cross (duration - 1) exactly once per song.
                if !self.paused
                    && !self.end_advanced
                    && duration > 2.0
                    && self.position_secs >= duration - 1.0
                    && self.advance_queue()
                {
                    self.end_advanced = true;
                    self.publish_mpris();
                    tasks.push(self.play_current_task());
                }
                // Import banner smoothing toward the true arrived count.
                #[cfg(feature = "innertube")]
                {
                    self.ease_import_shown();
                }
                // Scheduled sync: web changes arrive on their own while
                // signed in (plus Sync now in Settings). Idle only: never
                // while a pull is already in flight.
                #[cfg(feature = "innertube")]
                {
                    if sync_due(self.auto_sync, self.last_sync, self.started_at, now)
                        && Self::has_session()
                    {
                        tasks.push(self.start_sync(true));
                    }
                }
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
            Message::SeekTo(pos) => {
                self.seek_to_secs(pos as f64);
            }
            Message::SeekToMs(ms) => {
                self.seek_to_secs(ms as f64 / 1000.0);
            }
            Message::UrlResolved { track_id, result } => {
                // Only the in-flight track may report; anything else is stale.
                if self.loading_track.as_deref() != Some(track_id.as_str()) {
                    return iced::Task::none();
                }
                self.loading_track = None;
                match result {
                    Err(e) => {
                        self.pending_resume = None;
                        let msg = format!("Playback failed for {track_id}: {e}");
                        self.log_error(msg.clone());
                        self.status = msg;
                    }
                    Ok(url) => {
                        if let Some(expires) = url_expiry(&url) {
                            self.url_cache
                                .insert(track_id.clone(), (url.clone(), expires));
                        }
                        if let Some(t) = self.queue.current().cloned() {
                            if t.id == track_id && !self.try_play_url(&url, &t) {
                                // Stream play failed: fall back to bytes.
                                self.loading_track = Some(t.id.clone());
                                self.publish_mpris();
                                return Self::resolve_task(t.id);
                            }
                        }
                    }
                }
            }
            Message::UrlPrefetched { track_id, result } => {
                // Keep it only if it is still the upcoming track.
                if self.peek_next_id().as_deref() == Some(track_id.as_str()) {
                    if let Ok(url) = result {
                        self.prefetched = Some((track_id, url));
                    }
                }
            }
            Message::StreamResolved { video_id, result } => {
                self.loading_track = None;
                match result {
                    Err(e) => {
                        self.pending_resume = None;
                        let msg = format!("Playback failed for {video_id}: {e}");
                        self.log_error(msg.clone());
                        self.status = msg;
                    }
                    Ok(bytes) => {
                        // Stale arrival (user moved on): drop, keep playing.
                        let current_id = self
                            .queue
                            .current()
                            .map(|t| t.id.clone())
                            .unwrap_or_default();
                        if video_id != current_id {
                            return iced::Task::none();
                        }
                        match &self.audio {
                            None => {
                                self.pending_resume = None;
                                self.status =
                                    format!("Stream {video_id} fetched, but no audio device.");
                            }
                            Some(audio) => match audio.play_track_bytes(bytes) {
                                Ok(()) => {
                                    self.paused = false;
                                    self.end_advanced = false;
                                    self.position_secs = 0.0;
                                    if let Some(t) = self.queue.current() {
                                        self.status =
                                            format!("Playing: {} · {}.", t.title, t.artist);
                                    }
                                    self.consume_resume();
                                }
                                Err(e) => {
                                    self.pending_resume = None;
                                    let msg = format!("Playback failed for {video_id}: {e}");
                                    self.log_error(msg.clone());
                                    self.status = msg;
                                }
                            },
                        }
                    }
                }
            }
            Message::TracksHydrated { requested, tracks } => {
                // Probes landed (or failed): clear the in-flight marks so a
                // later navigation can retry what is still visible.
                for id in &requested {
                    self.hydrating.remove(id);
                }
                if tracks.is_empty() {
                    // Offline or removed videos: cool down so chained
                    // visible scans do not re-probe in a loop. Explicit
                    // navigation retries after the cooldown expires.
                    for id in requested {
                        Self::note_failed(&mut self.hydrate_cooldown, id);
                    }
                    return iced::Task::none();
                }
                let registered = self.register_tracks(tracks);
                let tasks = self.artwork_tasks(&registered);
                // Still-missing ids are unresolvable right now: cool them
                // down instead of letting art arrivals re-probe at once.
                let still_missing: Vec<String> = requested
                    .into_iter()
                    .filter(|id| !self.catalog.contains_key(id))
                    .collect();
                if !still_missing.is_empty() {
                    eprintln!(
                        "[yutu-hydrate] {} still missing after batch",
                        still_missing.len()
                    );
                    for id in still_missing {
                        Self::note_failed(&mut self.hydrate_cooldown, id);
                    }
                }
                let n = registered.len();
                eprintln!("[yutu-hydrate] restored {n} track(s)");
                // Nudge the status only when Favorites was empty before:
                // otherwise this is a silent background heal.
                if self.favorite_tracks().len() <= n {
                    self.status = format!("Restored {n} saved song(s).");
                }
                self.publish_mpris();
                return iced::Task::batch(tasks);
            }
        }
        self.publish_mpris();
        iced::Task::none()
    }

    /// Resolve + download task for a video ID. Runs on iced's tokio executor.
    fn resolve_task(video_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_and_fetch(&video_id).await;
                (video_id, result)
            },
            |(video_id, result)| Message::StreamResolved { video_id, result },
        )
    }

    /// Debounced keystroke: wait 200ms for the user to keep typing, then
    /// fire (suggest path waits 150ms); repeats are cache-served. Only
    /// the latest generation fires a real search.
    #[cfg(feature = "innertube")]
    fn search_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                (gen, query)
            },
            |(gen, query)| Message::SearchDebounced { gen, query },
        )
    }

    /// Real YouTube Music search (needs the `innertube` feature): one
    /// anonymous connection, three category queries run in sequence.
    /// Partial success is kept: only a total failure errors.
    #[cfg(feature = "innertube")]
    fn search_task(query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_search(&query).await;
                (query, result)
            },
            |(query, result)| Message::SearchCompleted { query, result },
        )
    }

    /// Resolve a track to a stream URL fast (yt-dlp only, no download).
    /// mpv plays the URL progressively; the rodio path uses `resolve_task`
    /// (full bytes) instead.
    fn resolve_url_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_stream_url(&track_id).await;
                (track_id, result)
            },
            |(track_id, result)| Message::UrlResolved { track_id, result },
        )
    }

    /// Lyrics fetch for one catalog track (LRCLIB, no login needed).
    #[cfg(feature = "lyrics")]
    fn lyrics_task(track: Track) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = yutu_core::fetch_lrclib(
                    &track.artist,
                    &track.title,
                    track.album.as_deref(),
                    track.duration_secs,
                )
                .await;
                (track.id, result)
            },
            |(track_id, result)| Message::LyricsReady { track_id, result },
        )
    }

    /// Background prefetch of the next song's URL (never auto-plays).
    fn prefetch_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = resolve_stream_url(&track_id).await;
                (track_id, result)
            },
            |(track_id, result)| Message::UrlPrefetched { track_id, result },
        )
    }

    /// Google device-flow: request the code pair (network I/O).
    #[cfg(feature = "innertube")]
    fn google_begin_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::begin_google_login().await },
            |result| Message::GoogleLoginBegan { result },
        )
    }

    /// Google device-flow: one approval poll (caller retries on pending).
    #[cfg(feature = "innertube")]
    fn google_poll_task(login: yutu_core::GoogleLogin) -> iced::Task<Message> {
        iced::Task::perform(
            async move { yutu_core::poll_google_login(&login).await },
            |result| Message::GoogleLoginPolled { result },
        )
    }

    /// Signed-in library import, base stage: songs + liked + playlist
    /// headers over OAuth. Playlist tracks stream in next like cookies.
    #[cfg(feature = "innertube")]
    fn library_import_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Library,
                result,
            },
        )
    }

    /// Disk half of a base pull (background): track cache plus the
    /// server-wins like/dislike replace, so restarts keep the base even
    /// if playlist stages never finish.
    #[cfg(feature = "innertube")]
    fn persist_base_pull(library_path: &str, base: &BasePull) {
        Self::persist_imported(library_path, &base.tracks, &[], &[]);
        Self::apply_sync_ratings(library_path, base);
    }

    /// Server-wins replace of the stored like/dislike sets after a base
    /// pull (background): still-pending rates ride over the server sets
    /// so unpushed intent survives. Skipped when the library fetch
    /// failed open (an empty pull must never wipe local state).
    #[cfg(feature = "innertube")]
    fn apply_sync_ratings(library_path: &str, base: &BasePull) {
        if !base.library_ok {
            return;
        }
        let Ok(store) = LibraryStore::open(library_path) else {
            return;
        };
        let _ = store.apply_sync_pull(&base.liked_ids, &base.disliked_ids, true, &[]);
    }

    /// [`StagedImport`] from a fetched [`BasePull`] (moves every field).
    #[cfg(feature = "innertube")]
    fn staged_of(backend: yutu_core::InnertubeBackend, base: BasePull) -> StagedImport {
        StagedImport {
            backend,
            tracks: base.tracks,
            liked_ids: base.liked_ids,
            disliked_ids: base.disliked_ids,
            headers: base.headers,
            headers_clean: base.headers_clean,
            library_ok: base.library_ok,
        }
    }

    /// Any saved login: OAuth token first, else the cookie session.
    /// Powers best-effort server sync (likes, playlist writes) without
    /// caring which flow signed the user in. A broken OAuth file falls
    /// back to a valid cookie session instead of failing outright.
    #[cfg(feature = "innertube")]
    async fn session_backend() -> Result<yutu_core::InnertubeBackend, String> {
        if std::path::Path::new(&oauth_path()).exists() {
            match yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path())) {
                Ok(persist) => {
                    return yutu_core::InnertubeBackend::connect_with_oauth(&persist);
                }
                Err(e) => {
                    eprintln!(
                        "[yutu-login] saved OAuth token unusable ({e}): trying cookie session"
                    );
                }
            }
        }
        if cookie_session_exists() {
            return yutu_core::InnertubeBackend::connect_with_cookie_file(std::path::Path::new(
                &cookie_session_path(),
            ))
            .await;
        }
        Err("sign in first (cookies or Google)".into())
    }

    /// Auth headers for raw search, mirroring [`Self::session_backend`]
    /// precedence (OAuth file, else cookie file, else anonymous). The
    /// server personalizes ranking for signed-in sessions, like the
    /// logged-in web client.
    #[cfg(feature = "innertube")]
    fn session_search_auth() -> yutu_core::SearchAuth {
        use yutu_core::SearchAuth;
        if std::path::Path::new(&oauth_path()).exists() {
            if let Ok(persist) =
                yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))
            {
                if let Some(auth) = SearchAuth::from_oauth(&persist) {
                    return auth;
                }
            }
        }
        if cookie_session_exists() {
            if let Ok(auth) =
                yutu_core::CookieAuth::from_file(std::path::Path::new(&cookie_session_path()))
            {
                return SearchAuth::from_cookie(&auth);
            }
        }
        SearchAuth::anonymous()
    }

    /// True when a like/playlist write can attempt a server sync.
    #[cfg(feature = "innertube")]
    fn has_session() -> bool {
        std::path::Path::new(&oauth_path()).exists() || cookie_session_exists()
    }

    /// Import base read: library songs + liked ids + playlist headers
    /// (tracks stream in next, one task per list). Each list fails open
    /// (empty vec), so one bad endpoint cannot sink the import; callers
    /// reject a fully-empty result with an account-specific hint.
    #[cfg(feature = "innertube")]
    async fn fetch_base(backend: &yutu_core::InnertubeBackend) -> Result<BasePull, String> {
        let (lib, status_liked, status_disliked, library_ok) =
            match backend.library_songs_with_status().await {
                Ok(rows) => {
                    let mut lib = Vec::with_capacity(rows.len());
                    for (t, _) in &rows {
                        lib.push(t.clone());
                    }
                    let (liked, disliked) = yutu_core::split_library_status(&rows);
                    (lib, liked, disliked, true)
                }
                Err(_) => (Vec::new(), Vec::new(), Vec::new(), false),
            };
        // Liked playlist union: either source may miss rows the other has.
        let liked_songs = backend.liked_songs().await.unwrap_or_default();
        let mut liked_ids = status_liked;
        for t in &liked_songs {
            if !liked_ids.iter().any(|x| x == &t.id) {
                liked_ids.push(t.id.clone());
            }
        }
        let mut all = lib;
        for t in liked_songs {
            if !all.iter().any(|x| x.id == t.id) {
                all.push(t);
            }
        }
        let (headers, failed) = match backend.library_playlists_report().await {
            Ok((h, f)) => (h, f),
            Err(_) => (Vec::new(), 1),
        };
        Ok(BasePull {
            tracks: all,
            liked_ids,
            disliked_ids: status_disliked,
            headers,
            headers_clean: failed == 0,
            library_ok,
        })
    }

    /// Push one like/unlike to YouTube Music (best effort: the local
    /// state is already applied, so only failures report back).
    /// Sends `like/like` vs `like/removelike` with a `videoId` target.
    #[cfg(feature = "innertube")]
    fn like_sync_task(track_id: String, liked: bool) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.rate_like(&track_id, liked).await,
                    Err(e) => Err(e),
                };
                (track_id, liked, result)
            },
            |(track_id, liked, result)| Message::LikeSynced {
                track_id,
                liked,
                result,
            },
        )
    }

    /// Push one dislike/clear ("don't recommend") to YouTube Music
    /// (best effort, same local-first contract as [`Self::like_sync_task`]).
    /// Sends `like/dislike` vs `like/removelike` with a `videoId` target.
    #[cfg(feature = "innertube")]
    fn dislike_sync_task(track_id: String, disliked: bool) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.rate_dislike(&track_id, disliked).await,
                    Err(e) => Err(e),
                };
                (track_id, disliked, result)
            },
            |(track_id, disliked, result)| Message::DislikeSynced {
                track_id,
                disliked,
                result,
            },
        )
    }

    /// Create the playlist on YouTube Music (best effort): success
    /// migrates the local `u{seq}` shell to the real server id.
    #[cfg(feature = "innertube")]
    fn playlist_create_sync_task(local_id: String, title: String) -> iced::Task<Message> {
        if !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let backend = Self::session_backend().await;
                let result = match backend {
                    Ok(b) => b.create_server_playlist(&title).await,
                    Err(e) => Err(e),
                };
                (local_id, title, result)
            },
            |(local_id, title, result)| Message::ServerPlaylistCreated {
                local_id,
                title,
                result,
            },
        )
    }

    /// Append one track to a server playlist (best effort).
    #[cfg(feature = "innertube")]
    fn playlist_add_sync_task(playlist_id: String, track_id: String) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.add_to_server_playlist(&playlist_id, &track_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, track_id, result)
            },
            |(playlist_id, track_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube add of {track_id} to {playlist_id} failed"),
                op: PlaylistSyncOp::Add {
                    playlist_id: playlist_id.clone(),
                    track_id: track_id.clone(),
                },
                result,
            },
        )
    }

    /// Delete a server playlist (best effort).
    /// Sends `playlist/delete` with the server `playlistId`.
    #[cfg(feature = "innertube")]
    fn playlist_delete_sync_task(playlist_id: String) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.delete_server_playlist(&playlist_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, result)
            },
            |(playlist_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube delete of {playlist_id} failed"),
                op: PlaylistSyncOp::Delete {
                    playlist_id: playlist_id.clone(),
                },
                result,
            },
        )
    }

    /// Remove one track from a server playlist (best effort).
    /// Sends `browse/edit_playlist` with `ACTION_REMOVE_VIDEO` after
    /// resolving the per-membership `setVideoId` from raw browse JSON.
    #[cfg(feature = "innertube")]
    fn playlist_remove_sync_task(playlist_id: String, track_id: String) -> iced::Task<Message> {
        if !Self::is_server_playlist(&playlist_id) || !Self::has_session() {
            return iced::Task::none();
        }
        iced::Task::perform(
            async move {
                let result = match Self::session_backend().await {
                    Ok(b) => b.remove_from_server_playlist(&playlist_id, &track_id).await,
                    Err(e) => Err(e),
                };
                (playlist_id, track_id, result)
            },
            |(playlist_id, track_id, result)| Message::ServerPlaylistSynced {
                detail: format!("YouTube remove of {track_id} from {playlist_id} failed"),
                op: PlaylistSyncOp::Remove {
                    playlist_id: playlist_id.clone(),
                    track_id: track_id.clone(),
                },
                result,
            },
        )
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn like_sync_task(_track_id: String, _liked: bool) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn dislike_sync_task(_track_id: String, _disliked: bool) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn playlist_create_sync_task(_local_id: String, _title: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn playlist_add_sync_task(_playlist_id: String, _track_id: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn playlist_delete_sync_task(_playlist_id: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Offline builds: server sync is a no-op (local state is the truth).
    #[cfg(not(feature = "innertube"))]
    fn playlist_remove_sync_task(_playlist_id: String, _track_id: String) -> iced::Task<Message> {
        iced::Task::none()
    }

    /// Start a sync pull (manual or scheduled): replays pending pushes,
    /// then pulls web changes through the saved session (OAuth preferred,
    /// cookie fallback). Silent no-op while busy; honest status when
    /// signed out.
    #[cfg(feature = "innertube")]
    fn start_sync(&mut self, auto: bool) -> iced::Task<Message> {
        if self.import_active || self.login_busy {
            return iced::Task::none();
        }
        let pull = if std::path::Path::new(&oauth_path()).exists() {
            Some(Self::sync_oauth_task(self.library_path.clone()))
        } else if cookie_session_exists() {
            Some(Self::sync_cookie_task(self.library_path.clone()))
        } else {
            None
        };
        let Some(pull) = pull else {
            self.status = "Sync needs sign-in (cookies or Google).".into();
            self.publish_mpris();
            return iced::Task::none();
        };
        self.login_busy = true;
        self.import_active = true;
        self.status = if auto {
            "Auto-syncing with YouTube Music…".into()
        } else {
            "Syncing with YouTube Music…".into()
        };
        self.publish_mpris();
        iced::Task::batch([self.replay_pending_tasks(), pull])
    }

    /// Re-fire every pending push (sync replays leftovers before the
    /// pull reads; successes clear their rows, failures stay queued).
    /// Each task no-ops without a session on its own.
    #[cfg(feature = "innertube")]
    fn replay_pending_tasks(&self) -> iced::Task<Message> {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return iced::Task::none();
        };
        let ops = store.pending_ops().unwrap_or_default();
        let mut tasks = Vec::with_capacity(ops.len());
        for op in ops {
            match op.op.as_str() {
                "like" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::like_sync_task(id, true));
                    }
                }
                "unlike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::like_sync_task(id, false));
                    }
                }
                "dislike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::dislike_sync_task(id, true));
                    }
                }
                "undislike" => {
                    if let Some(id) = op.track_id {
                        tasks.push(Self::dislike_sync_task(id, false));
                    }
                }
                "add" => {
                    if let (Some(pid), Some(tid)) = (op.playlist_id, op.track_id) {
                        tasks.push(Self::playlist_add_sync_task(pid, tid));
                    }
                }
                "remove" => {
                    if let (Some(pid), Some(tid)) = (op.playlist_id, op.track_id) {
                        tasks.push(Self::playlist_remove_sync_task(pid, tid));
                    }
                }
                "create" => {
                    // Only while the shell still exists locally.
                    if let (Some(local), Some(title)) = (op.playlist_id, op.title) {
                        if self.user_playlists.iter().any(|p| p.id == local) {
                            tasks.push(Self::playlist_create_sync_task(local, title));
                        }
                    }
                }
                "delete" => {
                    if let Some(pid) = op.playlist_id {
                        tasks.push(Self::playlist_delete_sync_task(pid));
                    }
                }
                _ => {}
            }
        }
        iced::Task::batch(tasks)
    }

    /// Record a completed pull (any kind) for the scheduler + label.
    #[cfg(feature = "innertube")]
    fn mark_synced(&mut self) {
        self.last_sync = Some(std::time::Instant::now());
        let unix = now_unix();
        self.last_sync_unix = Some(unix);
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.set_pref("last_sync_unix", &unix.to_string());
        }
    }

    /// Suggestion popup: one light query (texts only, no tracks).
    #[cfg(feature = "innertube")]
    async fn live_suggest(query: &str) -> Result<Vec<String>, String> {
        use yutu_core::InnertubeBackend;
        let backend = InnertubeBackend::connect_anonymous().await?;
        backend.search_suggestions(query).await
    }

    /// Suggestion debounce (shorter than search: the popup must feel instant).
    #[cfg(feature = "innertube")]
    fn suggest_debounce_task(gen: u64, query: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let result = Self::live_suggest(&query).await;
                (gen, result)
            },
            |(gen, result)| Message::SuggestCompleted { gen, result },
        )
    }

    /// Autoplay tail for [`Message::PlayNow`]: same-artist radio, seed first
    /// (the caller strips the seed and anything already queued).
    #[cfg(feature = "innertube")]
    fn autoplay_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::AutoplayReady { seed, result },
        )
    }

    /// Offline download bundle: audio bytes plus the sidecar files that
    /// make the song whole with no connection (lyrics + artwork). Audio
    /// failing fails the download; sidecar misses only skip that file
    /// (online playback re-heals them through the usual tasks).
    fn download_task(track: Track) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result: Result<(), String> = async {
                    let bytes = resolve_and_fetch(&track.id).await?;
                    let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
                    cache.set_dir(offline_cache_dir());
                    cache
                        .store_bytes(&track.id, &bytes)
                        .map_err(|e| e.to_string())?;
                    #[cfg(feature = "lyrics")]
                    {
                        if let Ok(lyrics) = yutu_core::fetch_lrclib(
                            &track.artist,
                            &track.title,
                            track.album.as_deref(),
                            track.duration_secs,
                        )
                        .await
                        {
                            save_lyrics(&track.id, &lyrics);
                        }
                    }
                    let mut urls: Vec<String> = track.thumbnail_url.iter().cloned().collect();
                    urls.extend(
                        track
                            .thumbnail_large_url
                            .clone()
                            .map(|u| yutu_core::hires_thumb_url(&u, 544)),
                    );
                    for url in urls {
                        if let Ok(bytes) = yutu_player::fetch_bytes(&url).await {
                            if bytes.len() > 512 {
                                save_artwork(&url, &bytes);
                            }
                        }
                    }
                    Ok(())
                }
                .await;
                let track_id = track.id.clone();
                (track_id, result)
            },
            |(track_id, result)| Message::Downloaded { track_id, result },
        )
    }

    /// Cookie import, base stage: connect, stash the 0600 session, fetch
    /// songs + liked + playlist headers. The user logs into
    /// music.youtube.com in their own browser and exports cookies to a
    /// file (no OAuth client needed). Playlist tracks stream in next, one
    /// task per list, so progress shows instead of one silent future.
    #[cfg(feature = "innertube")]
    fn cookie_import_task(path: String, library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&path),
                )
                .await?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                // Stay signed in: stash the normalised header (0600) only
                // for a validated import, so failures leave no session.
                if let Ok(auth) = yutu_core::CookieAuth::from_file(std::path::Path::new(&path)) {
                    if let Err(e) = auth.save_to_file(std::path::Path::new(&cookie_session_path()))
                    {
                        eprintln!("[yutu-login] session stash failed: {e}");
                    }
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Cookie,
                result,
            },
        )
    }

    /// Re-import base stage via the saved cookie session (no file picker
    /// round trip). Playlist tracks stream in next like a fresh import.
    #[cfg(feature = "innertube")]
    fn cookie_session_import_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&cookie_session_path()),
                )
                .await
                .map_err(|e| {
                    format!("saved cookie session failed ({e}): import your cookies file again")
                })?;
                let base = Self::fetch_base(&backend).await?;
                if base.tracks.is_empty() && base.headers.is_empty() {
                    return Err("library is empty: like songs or make a playlist on music.youtube.com, then import again".to_string());
                }
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Cookie,
                result,
            },
        )
    }

    /// Sync base stage over OAuth (same pull as an import, "Sync"
    /// wording): replays pending pushes first via [`Self::start_sync`]
    /// tasks, then reconciles web changes. Empty pulls succeed (an
    /// empty account has nothing to reconcile; the flags guard wipes).
    #[cfg(feature = "innertube")]
    fn sync_oauth_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let base = Self::fetch_base(&backend).await?;
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Sync,
                result,
            },
        )
    }

    /// Sync base stage via the saved cookie session (same pull as an
    /// import, "Sync" wording). See [`Self::sync_oauth_task`].
    #[cfg(feature = "innertube")]
    fn sync_cookie_task(library_path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&cookie_session_path()),
                )
                .await
                .map_err(|e| {
                    format!("saved cookie session failed ({e}): import your cookies file again")
                })?;
                let base = Self::fetch_base(&backend).await?;
                Self::persist_base_pull(&library_path, &base);
                Ok(Self::staged_of(backend, base))
            },
            |result| Message::ImportStageReady {
                kind: ImportKind::Sync,
                result,
            },
        )
    }

    /// One playlist's tracks over the stage's reused backend (no
    /// reconnect per list). Each mapped page sends its cumulative song
    /// count through a channel (forwarded as [`Message::PlaylistTick`]),
    /// so the banner bar moves song by song; the full merge still lands
    /// once via [`Message::PlaylistImported`]. Failures report per list;
    /// the import continues with the rest.
    #[cfg(feature = "innertube")]
    fn playlist_import_task(
        backend: yutu_core::InnertubeBackend,
        library_path: String,
        kind: ImportKind,
        gen: u64,
        header: yutu_core::ServerPlaylist,
        index: usize,
        total: usize,
    ) -> iced::Task<Message> {
        let (tick_tx, tick_rx) = tokio::sync::mpsc::unbounded_channel();
        let fetch = iced::Task::perform(
            async move {
                let count = backend.playlist_details_count(&header.id).await.ok();
                let _ = tick_tx.send((0usize, count));
                let result = backend
                    .playlist_tracks_live(&header.id, |arrived| {
                        let _ = tick_tx.send((arrived, count));
                    })
                    .await
                    .map(|tracks| {
                        // Disk first (off the GUI thread): the handler only
                        // touches memory, so arrivals never stall events.
                        // Lists with unpushed edits are not persisted:
                        // the server copy predates local intent.
                        let playlist = Playlist {
                            id: header.id,
                            title: header.title.clone(),
                            tracks,
                        };
                        if !Self::playlist_has_pending_store(&library_path, &playlist.id) {
                            Self::persist_imported(
                                &library_path,
                                &playlist.tracks,
                                &[],
                                std::slice::from_ref(&playlist),
                            );
                        }
                        playlist
                    });
                (kind, gen, header.title, index, total, count, result)
            },
            |(kind, gen, title, index, total, count, result)| Message::PlaylistImported {
                kind,
                gen,
                title,
                index,
                total,
                count,
                result,
            },
        );
        // Channel ticks end when the fetch drops its sender; late ticks
        // after completion are quarantined by generation + finished set.
        let forward = iced::Task::stream(Self::tick_forward_stream(tick_rx, gen, index));
        iced::Task::batch([fetch, forward])
    }

    /// Forward import tick channel items as messages (split out for
    /// headless testing of the combinator: order preserved, ends when
    /// the fetch drops its sender).
    #[cfg(feature = "innertube")]
    fn tick_forward_stream(
        mut tick_rx: tokio::sync::mpsc::UnboundedReceiver<(usize, Option<usize>)>,
        gen: u64,
        index: usize,
    ) -> impl futures::Stream<Item = Message> {
        futures::stream::poll_fn(move |cx| {
            tick_rx.poll_recv(cx).map(|opt| {
                opt.map(|(arrived, count)| Message::PlaylistTick {
                    gen,
                    index,
                    arrived,
                    count,
                })
            })
        })
    }

    /// Radio seed: same-artist tracks first, then the rest of the search.
    #[cfg(feature = "innertube")]
    fn radio_task(seed: String, artist: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_radio(&seed, &artist).await;
                (seed, result)
            },
            |(seed, result)| Message::RadioReady { seed, result },
        )
    }

    /// Search-result playlist fetch: tracks arrive via
    /// [`Message::SearchPlaylistReady`], which queues or reports.
    #[cfg(feature = "innertube")]
    fn search_playlist_task(id: String, label: String, shuffle: bool) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result = live_playlist_tracks(&id).await;
                (id, label, shuffle, result)
            },
            |(id, label, shuffle, result)| Message::SearchPlaylistReady {
                id,
                label,
                shuffle,
                result,
            },
        )
    }

    pub fn theme(&self) -> iced::Theme {
        iced::Theme::Dark
    }

    fn is_nav(&self, route: &Route) -> bool {
        std::mem::discriminant(&self.route) == std::mem::discriminant(route)
    }

    /// One compact track row: green play, title/artist, heart. All other
    /// actions live on detail screens: this killed the button-soup.
    fn track_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row, text};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            SPOT_GREEN
        } else {
            MUTED
        };
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        mouse_area(
            container(
                row![
                    self.thumb(t),
                    button(icon("play", 13.0, iced::Color::WHITE))
                        .style(green_play)
                        .on_press(Message::PlayNow(t.id.clone()))
                        .padding(6),
                    Self::track_text_single(&t.title, &t.artist, 14, iced::Color::WHITE),
                    text(fmt_time(t.duration_secs as f64)).size(12).color(MUTED),
                    button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                        .style(hover_row)
                        .on_press(Message::Like(t.id.clone()))
                        .padding(6),
                    button(icon("ellipsis", 15.0, MUTED))
                        .style(hover_row)
                        .on_press(Message::OpenContext(t.id.clone()))
                        .padding(6),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    /// Playlist row: same controls as track_row, but art, text and play
    /// all start the playlist from this song on (the rest of the list
    /// queues behind it).
    fn playlist_track_row(
        &self,
        playlist_id: &str,
        t: &Track,
        index: usize,
    ) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row, text};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            SPOT_GREEN
        } else {
            MUTED
        };
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let play_here = Message::PlayPlaylistAt {
            playlist_id: playlist_id.to_string(),
            index,
        };
        let play_here_text = Message::PlayPlaylistAt {
            playlist_id: playlist_id.to_string(),
            index,
        };
        mouse_area(
            container(
                row![
                    button(self.thumb(t))
                        .style(row_click)
                        .on_press(play_here)
                        .padding(2),
                    button(Self::track_text_single(
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
                    button(icon("ellipsis", 15.0, MUTED))
                        .style(hover_row)
                        .on_press(Message::OpenContext(t.id.clone()))
                        .padding(6),
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    /// Compact search-popup row: smaller cover, single-line title/artist,
    /// play count + duration, no inline buttons. The whole row clicks to
    /// play; right-click opens the context menu. Fixed height plus a
    /// clipped text column so long names cut instead of painting over
    /// the play count.
    fn suggest_track_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let play_id = t.id.clone();
        let menu_id = t.id.clone();
        let plays = t.plays.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let meta = match plays {
            Some(p) => format!("{p} · {}", fmt_time(t.duration_secs as f64)),
            None => fmt_time(t.duration_secs as f64),
        };
        mouse_area(
            container(
                button(
                    row![
                        self.thumb_sized(t, 32.0),
                        column![
                            text(truncate_title(&t.title, 30))
                                .size(13)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            text(truncate_title(&t.artist, 28))
                                .size(11)
                                .color(MUTED)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                        ]
                        .spacing(2)
                        .width(Fill)
                        .clip(true),
                        text(meta).size(11).color(MUTED).wrapping(Wrapping::None),
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
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(SUGGEST_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(menu_id))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    fn section<'a>(
        title: &'a str,
        rows: Vec<iced::Element<'a, Message>>,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, text};
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text("Nothing here yet.").size(12).color(MUTED).into()
        } else {
            column(rows).spacing(2).into()
        };
        column![text(title).size(19), list,].spacing(6).into()
    }

    /// Wrapping card strip: fixed-size cards flow onto the next line when
    /// the window is narrow instead of squashing, so every card keeps its
    /// standard size and height alignment.
    fn card_strip(cards: Vec<iced::Element<'_, Message>>) -> iced::Element<'_, Message> {
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
    fn search_filter_chips(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, text};
        use iced::Fill;
        fn chip(
            label: &'static str,
            f: SearchFilter,
            active: SearchFilter,
        ) -> iced::Element<'static, Message> {
            button(text(label).size(13))
                .style(nav_style(active == f))
                .on_press(Message::SetSearchFilter(f))
                .padding(8)
                .into()
        }
        let active = self.search_filter;
        iced::widget::Row::with_children(vec![
            chip("All", SearchFilter::All, active),
            chip("Songs", SearchFilter::Songs, active),
            chip("Albums", SearchFilter::Albums, active),
            chip("Artists", SearchFilter::Artists, active),
            chip("Playlists", SearchFilter::Playlists, active),
        ])
        .spacing(8)
        .width(Fill)
        .wrap()
        .into()
    }

    /// Loading state for the search page: text plus an animated
    /// ellipsis (the 500ms Tick repaints it, so no extra state).
    fn search_loading() -> iced::Element<'static, Message> {
        use iced::widget::{column, text};
        use iced::Fill;
        let dots = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_millis() / 500 % 4) as usize)
            .unwrap_or(0);
        column![text(format!("Searching YouTube Music{}", ".".repeat(dots)))
            .size(15)
            .color(MUTED),]
        .spacing(10)
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .into()
    }

    /// Top-result hero for the unified search list: the head hit in a
    /// nice card with its type in the subtitle. Songs keep play/like
    /// controls; albums and artists open their detail page.
    fn search_hero(&self, hit: &SearchHit) -> iced::Element<'_, Message> {
        match hit {
            SearchHit::Song(t) => self.search_track_hero(t),
            SearchHit::Album(a) => self.search_album_hero(a),
            SearchHit::Artist(a) => self.search_artist_hero(a),
            SearchHit::Playlist(p) => self.search_playlist_hero(p),
        }
    }

    /// Track hero: the previous top-result card, now with the result
    /// type in the subtitle (the mixed list has no sections).
    fn search_track_hero(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let heart_color = if self.liked.contains(&t.id) {
            SPOT_GREEN
        } else {
            MUTED
        };
        mouse_area(
            container(
                row![
                    self.thumb_sized(t, 96.0),
                    column![
                        text("Top result").size(12).color(MUTED),
                        text(t.title.clone()).size(20),
                        text(format!(
                            "Song · {} · {}",
                            t.artist,
                            t.album.clone().unwrap_or_default()
                        ))
                        .size(13)
                        .color(MUTED),
                        row![
                            button(
                                row![
                                    icon("play", 13.0, iced::Color::WHITE),
                                    text("Play").size(13),
                                ]
                                .spacing(6)
                            )
                            .style(green_play)
                            .on_press(Message::PlayNow(t.id.clone()))
                            .padding(8),
                            button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                                .style(hover_row)
                                .on_press(Message::Like(t.id.clone()))
                                .padding(8),
                            button(icon("ellipsis", 15.0, MUTED))
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
    fn search_album_hero(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => format!("Album · {} · {}", a.artist, y),
            None => format!("Album · {}", a.artist),
        };
        let tracks = self.album_tracks(&a.id);
        let actions: iced::Element<'_, Message> = if tracks.is_empty() {
            text("").size(1).into()
        } else {
            let seed = tracks.first().map(|t| t.id.clone()).unwrap_or_default();
            row![
                Self::shuffle_pill("SHUFFLE".into(), Message::PlayAlbumShuffled(a.id.clone())),
                button(
                    row![
                        icon("radio", 13.0, iced::Color::WHITE),
                        text("Radio").size(13),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(hover_row)
                .on_press(Message::StartRadio(seed))
                .padding(8),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        };
        container(
            row![
                button(self.art_thumb(cover.as_deref(), 96.0))
                    .style(row_click)
                    .on_press(Message::OpenAlbum(a.id.clone()))
                    .padding(0),
                column![
                    text("Top result").size(12).color(MUTED),
                    button(text(a.title.clone()).size(20))
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
    fn search_artist_hero(&self, a: &Artist) -> iced::Element<'_, Message> {
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
                        icon("radio", 13.0, iced::Color::WHITE),
                        text("Radio").size(13),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(hover_row)
                .on_press(Message::StartRadio(seed))
                .padding(8),
            ]
            .spacing(8)
            .align_y(iced::alignment::Vertical::Center)
            .into()
        };
        container(
            row![
                button(self.art_thumb(photo.as_deref(), 96.0))
                    .style(row_click)
                    .on_press(Message::OpenArtist(a.id.clone()))
                    .padding(0),
                column![
                    text("Top result").size(12).color(MUTED),
                    button(text(a.name.clone()).size(20))
                        .style(row_click)
                        .on_press(Message::OpenArtist(a.id.clone()))
                        .padding(0)
                        .width(Fill),
                    text("Artist").size(13).color(MUTED),
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
    fn search_hit_row(&self, hit: &SearchHit) -> iced::Element<'_, Message> {
        match hit {
            SearchHit::Song(t) => self.search_song_row(t),
            SearchHit::Album(a) => self.search_album_row(a),
            SearchHit::Artist(a) => self.search_artist_row(a),
            SearchHit::Playlist(p) => self.search_playlist_row(p),
        }
    }

    /// Song row for the unified search list: art with a hover play
    /// overlay (dim backdrop + white icon on top, whole art plays),
    /// type-led subtitle, and actions (like, more) that only appear on
    /// hover. Hidden actions keep same-size spacers so rows never jump.
    /// The liked heart stays visible: it is state, not just an action.
    /// Raw All results carry no durations, so a zero one renders blank.
    fn search_song_row(&self, t: &Track) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area, row};
        use iced::Fill;
        let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
        let hover_id = t.id.clone();
        let (heart, more) = self.song_actions(t, hovered);
        mouse_area(
            container(
                row![
                    self.art_play_button(t, hovered, Message::PlayNow(t.id.clone())),
                    Self::track_text_single(
                        &t.title,
                        &format!("Song · {}", t.artist),
                        14,
                        iced::Color::WHITE
                    ),
                    duration_text(t.duration_secs),
                    heart,
                    more,
                ]
                .spacing(8)
                .align_y(iced::alignment::Vertical::Center)
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    /// Album row for the unified search list: cover, type-led subtitle,
    /// one fixed row height, hover highlight + menu. Art and text open
    /// the album; the hover `…` (or right-click) opens its menu.
    fn search_album_row(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => format!("Album · {} · {}", a.artist, y),
            None => format!("Album · {}", a.artist),
        };
        let hovered = self.hovered_album.as_deref() == Some(a.id.as_str());
        mouse_area(
            container(
                row![
                    button(self.art_thumb(cover.as_deref(), 40.0))
                        .style(row_click)
                        .on_press(Message::OpenAlbum(a.id.clone()))
                        .padding(0),
                    button(
                        column![
                            text(truncate_title(&a.title, 45))
                                .size(14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            text(truncate_title(&sub, 40))
                                .size(11)
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
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenAlbumMenu(a.id.clone()))
        .on_enter(Message::AlbumHover(Some(a.id.clone())))
        .on_exit(Message::AlbumHover(None))
        .into()
    }

    /// Artist row for the unified search list: photo, type-led subtitle,
    /// one fixed row height, hover highlight + menu. Photo and name open
    /// the artist; the hover `…` (or right-click) opens its menu.
    fn search_artist_row(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let photo = a
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        let hovered = self.hovered_artist.as_deref() == Some(a.id.as_str());
        mouse_area(
            container(
                row![
                    button(self.art_thumb(photo.as_deref(), 40.0))
                        .style(row_click)
                        .on_press(Message::OpenArtist(a.id.clone()))
                        .padding(0),
                    button(
                        column![
                            text(truncate_title(&a.name, 45))
                                .size(14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            text("Artist").size(11).color(MUTED).width(Fill),
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
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenArtistMenu(a.id.clone()))
        .on_enter(Message::ArtistHover(Some(a.id.clone())))
        .on_exit(Message::ArtistHover(None))
        .into()
    }

    /// Playlist art at hero/row sizes (same 544px key the warmer stages).
    fn playlist_art(&self, p: &PlaylistHit, size: f32) -> iced::Element<'_, Message> {
        let url = p
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        self.art_thumb(url.as_deref(), size)
    }

    /// Playlist hero: art, type-led subtitle, Play + Shuffle pills.
    /// Clicking the art or title plays it too (there is no playlist
    /// detail page: playing is the open action).
    fn search_playlist_hero(&self, p: &PlaylistHit) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let sub = format!("Playlist · {} · {}", p.author, p.detail);
        container(
            row![
                button(self.playlist_art(p, 96.0))
                    .style(row_click)
                    .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                    .padding(0),
                column![
                    text("Top result").size(12).color(MUTED),
                    button(text(p.title.clone()).size(20))
                        .style(row_click)
                        .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                        .padding(0)
                        .width(Fill),
                    text(sub).size(13).color(MUTED),
                    row![
                        Self::play_pill("PLAY".into(), Message::PlaySearchPlaylist(p.id.clone())),
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
    fn search_playlist_row(&self, p: &PlaylistHit) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{button, column, container, mouse_area, row, text};
        use iced::Fill;
        let sub = format!("Playlist · {} · {}", p.author, p.detail);
        let hovered = self.hovered_search_playlist.as_deref() == Some(p.id.as_str());
        mouse_area(
            container(
                row![
                    button(self.playlist_art(p, 40.0))
                        .style(row_click)
                        .on_press(Message::PlaySearchPlaylist(p.id.clone()))
                        .padding(0),
                    button(
                        column![
                            text(truncate_title(&p.title, 45))
                                .size(14)
                                .color(iced::Color::WHITE)
                                .width(Fill)
                                .wrapping(Wrapping::None),
                            text(truncate_title(&sub, 40))
                                .size(11)
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
                .padding(4),
            )
            .style(row_bg(hovered))
            .width(Fill)
            .height(iced::Length::Fixed(TRACK_ROW_H))
            .clip(true)
            .align_y(iced::alignment::Vertical::Center),
        )
        .on_right_press(Message::OpenSearchPlaylistMenu(p.id.clone()))
        .on_enter(Message::SearchPlaylistHover(Some(p.id.clone())))
        .on_exit(Message::SearchPlaylistHover(None))
        .into()
    }

    /// YTM-style sidebar: Home + Search, Library with playlist search,
    /// inline new-playlist creation, and an expand/collapse rail.
    /// Queue/Recent live in the right panel; Settings lives in the header.
    fn sidebar_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, text, text_input};
        use iced::Fill;
        let pin_btn: iced::Element<'_, Message> = button(icon(
            "panel-left",
            16.0,
            if self.sidebar_pinned {
                SPOT_GREEN
            } else {
                MUTED
            },
        ))
        .style(hover_row)
        .on_press(Message::ToggleSidebarPin)
        .padding(8)
        .into();
        if self.sidebar_w < 150.0 {
            return container(
                column![
                    button(icon("house", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::Home)))
                        .on_press(Message::Navigate(Route::Home))
                        .padding(10),
                    button(icon("search", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::Search)))
                        .on_press(Message::Navigate(Route::Search))
                        .padding(10),
                    button(icon("library", 18.0, iced::Color::WHITE))
                        .style(nav_style(self.is_nav(&Route::Library)))
                        .on_press(Message::Navigate(Route::Library))
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
                    button(icon("play", 14.0, iced::Color::WHITE))
                        .style(green_play)
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
                    button(
                        row![
                            art,
                            column![
                                text(pl.title.clone()).size(13),
                                text(format!("Playlist · {} songs", pl.tracks.len()))
                                    .size(11)
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
                    .padding(4)
                    .width(Fill),
                )
                .on_right_press(Message::OpenPlaylistMenu(pl_id))
                .on_enter(Message::PlaylistHover(Some(pl_hover)))
                .on_exit(Message::PlaylistHover(None))
                .into()
            })
            .collect();
        let list: iced::Element<'_, Message> = if rows.is_empty() {
            text(if self.all_playlists().is_empty() {
                "No playlists yet: create one below."
            } else {
                "No playlists match."
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
                    text("yutu-mux")
                        .size(22)
                        .color(iced::Color::WHITE)
                        .width(Fill),
                    pin_btn,
                ]
                .align_y(iced::alignment::Vertical::Center),
                column![
                    button(
                        row![
                            icon("house", 14.0, iced::Color::WHITE),
                            text("Home").size(14),
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
                            icon("search", 14.0, iced::Color::WHITE),
                            text("Search").size(14),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(nav_style(self.is_nav(&Route::Search)))
                    .on_press(Message::Navigate(Route::Search))
                    .padding(8)
                    .width(Fill),
                ]
                .spacing(2),
                row![button(
                    row![
                        icon("library", 14.0, iced::Color::WHITE),
                        text("Library").size(14),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(nav_style(self.is_nav(&Route::Library)))
                .on_press(Message::Navigate(Route::Library))
                .padding(4)
                .width(Fill),],
                text_input("Filter playlists…", &self.sidebar_filter)
                    .style(rounded_input)
                    .on_input(Message::SidebarFilterChanged)
                    .width(Fill)
                    .padding(10),
                list,
                row![
                    text_input("New playlist…", &self.new_playlist_name)
                        .style(rounded_input)
                        .on_input(Message::NewPlaylistNameChanged)
                        .width(Fill)
                        .padding(10),
                    button(icon("plus", 16.0, iced::Color::WHITE))
                        .style(green_play)
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

    /// Collapsed origin: zero height while dragging, so siblings close
    /// ranks and no hole remains where the song was picked up.
    fn queue_row(&self, i: usize) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, mouse_area, row, text};
        use iced::Fill;
        if self.drag.is_some_and(|d| d.from == i) {
            // Origin keeps a full-size held chip (same art + title as the
            // row, no red border, no handlers): the song stays visible
            // while held and siblings never shift, since only the thin
            // bar opens at the armed slot.
            if let Some(t) = self.queue.items().get(i) {
                return container(
                    row![
                        self.thumb(t),
                        Self::track_text(&t.title, &t.artist, 12, iced::Color::WHITE),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center)
                    .padding(2),
                )
                .style(card_bg)
                .padding(0)
                .width(Fill)
                .height(iced::Length::Fixed(QUEUE_ROW_H))
                .clip(true)
                .align_y(iced::alignment::Vertical::Center)
                .into();
            }
            return iced::widget::Space::new().width(Fill).into();
        }
        let items = self.queue.items();
        let Some(t) = items.get(i) else {
            return iced::widget::Space::new().width(Fill).into();
        };
        let pos = self.queue.position();
        let is_current = Some(i) == pos;
        let menu_id = t.id.clone();
        let remove_id = t.id.clone();
        let hovered = self.hovered_queue == Some(i);
        let remove_btn: iced::Element<'_, Message> = if hovered {
            mouse_area(
                button(icon("x", 11.0, MUTED))
                    .style(hover_row)
                    .on_press(Message::RemoveFromQueue(remove_id))
                    .padding(3),
            )
            .on_enter(Message::RemoveHover(Some(i)))
            .on_exit(Message::RemoveHover(None))
            .into()
        } else {
            iced::widget::Space::new()
                .width(iced::Length::Fixed(20.0))
                .into()
        };
        let row_content = row![
            container(self.thumb(t)).padding(2),
            container(Self::track_text(
                &t.title,
                &t.artist,
                12,
                if is_current {
                    SPOT_GREEN
                } else {
                    iced::Color::WHITE
                },
            ))
            .padding(3)
            .width(Fill),
            container(text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED)).padding(3),
            remove_btn,
        ]
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center)
        .padding(2);
        // Fixed height: the title may wrap to two lines, anything more
        // clips instead of stretching the row.
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
        mouse_area(lifted)
            .on_press(Message::DragStart(i))
            .on_right_press(Message::OpenContext(menu_id))
            .on_enter(Message::PanelHover(i))
            .on_exit(Message::QueueHover(None))
            .into()
    }

    /// Insertion slot: slot `k` sits before row `k` (0..=len, edges
    /// included) so every song always has a committable move. Resting
    /// height is a 10px invisible lane; the armed slot shows a single
    /// red insertion bar, so rows visibly part for it.
    fn drop_slot(&self, k: usize) -> iced::Element<'_, Message> {
        use iced::widget::{container, mouse_area};
        use iced::Fill;
        // Only an armed slot that commits to a real move shows the bar.
        let active = self.effective_drag_slot() == Some(k);
        let bar_h = 2.0 + 6.0 * self.drag_gap;
        let inner: iced::Element<'_, Message> = if active {
            container(
                iced::widget::Space::new()
                    .height(iced::Length::Fixed(bar_h))
                    .width(Fill),
            )
            .style(drop_bar)
            .into()
        } else {
            iced::widget::Space::new()
                .height(iced::Length::Fixed(10.0))
                .width(Fill)
                .into()
        };
        // The bar reads as an insertion line by sitting slightly narrower
        // than the full rows around it.
        let padded = if active {
            container(inner).padding(iced::Padding {
                left: 10.0,
                right: 10.0,
                top: 2.0,
                bottom: 2.0,
            })
        } else {
            container(inner).padding(0)
        };
        mouse_area(padded).on_enter(Message::DragSlot(k)).into()
    }

    fn right_panel_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
        use iced::Fill;
        // Empty and collapsed: reserve nothing (gate on the target so the
        // panel does not flash mid-ease).
        if self.right_target() == 0.0 {
            return iced::widget::Space::new()
                .width(iced::Length::Fixed(0.0))
                .height(Fill)
                .into();
        }
        let current = self.queue.current().cloned();
        let now: iced::Element<'_, Message> = match &current {
            Some(t) => column![
                text("Now playing").size(13).color(MUTED),
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
                text("Now playing").size(13).color(MUTED),
                text("Nothing queued").size(14).color(MUTED),
            ]
            .spacing(4)
            .into(),
        };
        // Slot-interleaved manual list while dragging: slot k sits
        // before row k (0..=len, edges included). Dividers stay off
        // while dragging on purpose (slots need clean lanes); they
        // return at rest. At rest rows render plain with dividers.
        let n = self.queue.len();
        let mut upcoming: Vec<iced::Element<'_, Message>> =
            Vec::with_capacity(if self.drag.is_some() { 2 * n + 1 } else { n });
        if self.drag.is_some() {
            upcoming.push(self.drop_slot(0));
        }
        for k in 0..n {
            upcoming.push(self.queue_row(k));
            if self.drag.is_some() {
                upcoming.push(self.drop_slot(k + 1));
            }
        }
        let tab_queue = button(text("Queue").size(12))
            .style(nav_style(self.right_tab == RightTab::Queue))
            .on_press(Message::SetRightTab(RightTab::Queue))
            .padding(6);
        let tab_recent = button(text("Recent").size(12))
            .style(nav_style(self.right_tab == RightTab::Recent))
            .on_press(Message::SetRightTab(RightTab::Recent))
            .padding(6);
        let from_line: iced::Element<'_, Message> = if self.playing_from.is_empty() {
            text("").size(1).into()
        } else {
            text(format!("Playing from  {}", self.playing_from))
                .size(12)
                .color(MUTED)
                .into()
        };
        let auto_mini: Vec<iced::Element<'_, Message>> = self
            .autoplay
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let menu_id = t.id.clone();
                let remove_btn: iced::Element<'_, Message> = if self.hovered_auto == Some(i) {
                    button(icon("x", 11.0, MUTED))
                        .style(hover_row)
                        .on_press(Message::RemoveAutoplay(i))
                        .padding(3)
                        .into()
                } else {
                    iced::widget::Space::new()
                        .width(iced::Length::Fixed(20.0))
                        .into()
                };
                mouse_area(
                    container(
                        row![
                            self.thumb(t),
                            button(Self::track_text_single(
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
                        .padding(2),
                    )
                    .style(row_bg(self.hovered_auto == Some(i)))
                    .width(Fill)
                    .height(iced::Length::Fixed(TRACK_ROW_H))
                    .clip(true)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .on_right_press(Message::OpenContext(menu_id))
                .on_enter(Message::AutoHover(Some(i)))
                .on_exit(Message::AutoHover(None))
                .into()
            })
            .collect();
        let auto_block: iced::Element<'_, Message> = if auto_mini.is_empty() {
            text("").size(1).into()
        } else {
            column![
                text("Autoplay").size(13).color(MUTED),
                column(auto_mini).spacing(0),
            ]
            .spacing(2)
            .into()
        };
        let list: iced::Element<'_, Message> = match self.right_tab {
            RightTab::Queue => column![
                text("Up next").size(13).color(MUTED),
                button(text(format!("Clear queue ({})", self.queue.len())).size(12))
                    .style(hover_row)
                    .on_press(Message::QueueClear)
                    .padding(4),
                scrollable(
                    column![
                        column(if self.drag.is_some() {
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
                .on_scroll(|_| Message::Scrolled("queue"))
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
                    text("No recent plays yet.").size(12).color(MUTED).into()
                } else {
                    column(
                        recent
                            .iter()
                            .map(|t| {
                                let menu_id = t.id.clone();
                                let hover_id = t.id.clone();
                                let hovered = self.hovered_track.as_deref() == Some(t.id.as_str());
                                mouse_area(
                                    container(
                                        row![
                                            self.thumb(t),
                                            button(Self::track_text_single(
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
                                        .padding(2),
                                    )
                                    .style(row_bg(hovered))
                                    .width(Fill)
                                    .height(iced::Length::Fixed(TRACK_ROW_H))
                                    .clip(true)
                                    .align_y(iced::alignment::Vertical::Center),
                                )
                                .on_right_press(Message::OpenContext(menu_id))
                                .on_enter(Message::TrackHover(Some(hover_id)))
                                .on_exit(Message::TrackHover(None))
                                .into()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(0)
                    .into()
                }
            }
        };
        let pin_btn: iced::Element<'_, Message> = button(icon(
            "panel-right",
            16.0,
            if self.right_pinned { SPOT_GREEN } else { MUTED },
        ))
        .style(hover_row)
        .on_press(Message::ToggleRightPin)
        .padding(6)
        .into();
        if self.right_w < 140.0 {
            // Collapsed rail: queue thumbnails, a separator, then autoplay
            // thumbnails: same jump targets as the expanded rows.
            let pos = self.queue.position();
            let mut rail: Vec<iced::Element<'_, Message>> = Vec::new();
            for (i, t) in self.queue.items().iter().enumerate().take(6) {
                let btn: iced::Element<'_, Message> = button(self.thumb_sized(t, 32.0))
                    .style(hover_row)
                    .on_press(Message::QueueJump(i))
                    .padding(2)
                    .into();
                rail.push(if pos == Some(i) {
                    container(btn).style(card_bg).padding(1).into()
                } else {
                    btn
                });
            }
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
            for (i, t) in self.autoplay.iter().enumerate().take(4) {
                rail.push(
                    button(self.thumb_sized(t, 32.0))
                        .style(hover_row)
                        .on_press(Message::PlayAutoplay(i))
                        .padding(2)
                        .into(),
                );
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
        container(
            column![
                row![pin_btn, tab_queue, tab_recent].spacing(4),
                from_line,
                now,
                list,
            ]
            .spacing(8),
        )
        .style(sidebar_bg)
        .padding(12)
        .width(iced::Length::Fixed(self.right_w))
        .height(Fill)
        .into()
    }

    fn player_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, slider, text, tooltip};
        use iced::Fill;
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let mode = player_layout_mode(ww);
        let current = self.queue.current().cloned();
        let loading = self.loading_track.is_some();
        let title = if loading {
            "Loading…".into()
        } else {
            current
                .as_ref()
                .map(|t| t.title.clone())
                .unwrap_or_else(|| "Nothing playing".into())
        };
        let artist = current
            .as_ref()
            .map(|t| t.artist.clone())
            .unwrap_or_else(|| "Search for music, then click a song".into());
        let play_icon = if self.paused { "play" } else { "pause" };
        let duration = self.audio_duration();
        let slider_max = duration.max(1.0);
        let shown_pos = self.position_secs.clamp(0.0, slider_max);
        // Compact mode tightens everything but keeps every control.
        let tight = mode == PlayerLayout::Compact;
        let art_size = if tight { 32.0 } else { 40.0 };
        let title_size = if tight { 13 } else { 14 };
        let (left_part, center_part) = if tight { (3, 4) } else { (2, 3) };
        let transport_gap = if tight { 4.0 } else { 6.0 };
        let pad_side = if tight { 6 } else { 8 };
        let pad_play = if tight { 8 } else { 10 };
        let pad_shuffle = if tight { 5 } else { 7 };
        let volume_w = if tight { 80 } else { 120 };
        let main_gap = if tight { 8.0 } else { 12.0 };
        let like_button: iced::Element<'_, Message> = match &current {
            Some(t) => {
                let heart_color = if self.liked.contains(&t.id) {
                    SPOT_GREEN
                } else {
                    MUTED
                };
                button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                    .style(hover_row)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(6)
                    .into()
            }
            None => text("").size(1).into(),
        };
        let shuffle_button: iced::Element<'_, Message> = if self.queue.shuffle {
            button(icon("shuffle", 14.0, iced::Color::WHITE))
                .style(green_play)
                .on_press(Message::ToggleShuffle)
                .padding(pad_shuffle)
                .into()
        } else {
            button(icon("shuffle", 14.0, MUTED))
                .style(hover_row)
                .on_press(Message::ToggleShuffle)
                .padding(pad_shuffle)
                .into()
        };
        let (repeat_icon, repeat_badge): (&str, &str) = match self.queue.repeat {
            yutu_player::RepeatMode::Off => ("repeat", ""),
            yutu_player::RepeatMode::All => ("repeat", "All"),
            yutu_player::RepeatMode::One => ("repeat-1", "1"),
        };
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
                        SPOT_GREEN
                    } else {
                        MUTED
                    };
                    button(heart_icon(self.liked.contains(&t.id), 15.0, heart_color))
                        .style(hover_row)
                        .on_press(Message::Like(t.id.clone()))
                        .padding(6)
                        .into()
                }
                None => text("").size(1).into(),
            };
            let drawer_btn: iced::Element<'_, Message> = button(icon(
                if self.drawer_open {
                    "chevron-down"
                } else {
                    "chevron-up"
                },
                14.0,
                iced::Color::WHITE,
            ))
            .style(hover_row)
            .on_press(Message::ToggleDrawer)
            .padding(7)
            .into();
            let info = row![
                info_art,
                column![
                    text(title.clone()).size(13),
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
                        button(icon("shuffle", 14.0, iced::Color::WHITE))
                            .style(green_play)
                            .on_press(Message::ToggleShuffle)
                            .padding(5)
                            .into()
                    } else {
                        button(icon("shuffle", 14.0, MUTED))
                            .style(hover_row)
                            .on_press(Message::ToggleShuffle)
                            .padding(5)
                            .into()
                    };
                    shuffle_btn
                },
                button(icon("skip-back", 18.0, iced::Color::WHITE))
                    .style(hover_row)
                    .on_press(Message::Prev)
                    .padding(6),
                button(icon(play_icon, 18.0, iced::Color::WHITE))
                    .style(green_play)
                    .on_press(Message::TogglePause)
                    .padding(8),
                button(icon("skip-forward", 18.0, iced::Color::WHITE))
                    .style(hover_row)
                    .on_press(Message::Next)
                    .padding(6),
                button(
                    row![
                        icon(repeat_icon, 14.0, iced::Color::WHITE),
                        text(repeat_badge).size(11),
                    ]
                    .spacing(3)
                    .align_y(iced::alignment::Vertical::Center),
                )
                .style(hover_row)
                .on_press(Message::CycleRepeat)
                .padding(6),
            ]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center);
            let transport_centered: iced::Element<'_, Message> = container(transport)
                .width(Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .into();
            return container(
                column![
                    text(self.status_text()).size(12).color(MUTED),
                    info,
                    transport_centered,
                    row![
                        text(fmt_time(shown_pos)).size(11).color(MUTED),
                        slider(0.0..=slider_max as f32, shown_pos as f32, Message::SeekTo)
                            .step(1.0_f32)
                            .style(themed_slider)
                            .width(Fill),
                        text(fmt_time(duration)).size(11).color(MUTED),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                    row![
                        icon("volume", 14.0, MUTED),
                        slider(0.0..=1.0, self.volume, Message::SetVolume)
                            .step(0.01_f32)
                            .style(themed_slider)
                            .width(Fill),
                    ]
                    .spacing(6)
                    .align_y(iced::alignment::Vertical::Center),
                ]
                .spacing(6),
            )
            .style(player_bg)
            .padding(10)
            .into();
        }
        container(
            column![
                text(self.status_text()).size(12).color(MUTED),
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
                            Some(t) => button(text(artist).size(12).color(MUTED))
                                .style(row_click)
                                .on_press(Message::OpenTrackArtist(t.id.clone()))
                                .padding(0)
                                .into(),
                            None => text(artist).size(12).color(MUTED).into(),
                        };
                        let left = row![
                            art,
                            column![text(title).size(title_size), artist_el,].spacing(1),
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
                            button(icon("skip-back", 18.0, iced::Color::WHITE))
                                .style(hover_row)
                                .on_press(Message::Prev)
                                .padding(pad_side),
                            button(icon(play_icon, 18.0, iced::Color::WHITE))
                                .style(green_play)
                                .on_press(Message::TogglePause)
                                .padding(pad_play),
                            button(icon("skip-forward", 18.0, iced::Color::WHITE))
                                .style(hover_row)
                                .on_press(Message::Next)
                                .padding(pad_side),
                            button(
                                row![
                                    icon(repeat_icon, 14.0, iced::Color::WHITE),
                                    text(repeat_badge).size(11),
                                ]
                                .spacing(3)
                                .align_y(iced::alignment::Vertical::Center),
                            )
                            .style(hover_row)
                            .on_press(Message::CycleRepeat)
                            .padding(pad_side),
                        ]
                        .spacing(transport_gap)
                        .align_y(iced::alignment::Vertical::Center),
                        row![
                            text(fmt_time(shown_pos)).size(11).color(MUTED),
                            slider(0.0..=slider_max as f32, shown_pos as f32, Message::SeekTo)
                                .step(1.0_f32)
                                .style(themed_slider)
                                .width(Fill),
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
                        tooltip(
                            slider(0.0..=1.0, self.volume, Message::SetVolume)
                                .step(0.01_f32)
                                .style(themed_slider)
                                .width(volume_w),
                            text(format!("Volume {:.0}%", self.volume * 100.0)).size(12),
                            tooltip::Position::Top,
                        )
                        .gap(8),
                        button(icon(
                            if self.drawer_open {
                                "chevron-down"
                            } else {
                                "chevron-up"
                            },
                            14.0,
                            iced::Color::WHITE
                        ))
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
            ]
            .spacing(4),
        )
        .style(player_bg)
        .padding(10)
        .into()
    }

    /// Shared synced-lyrics body (lyrics page + Now Playing drawer):
    /// loading / instrumental / empty / plain states included.
    fn lyrics_content(&self, id: &str) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, row, text};
        match self.lyrics_cache.get(id) {
            Some(cached) if cached.instrumental => {
                text("[Instrumental]").size(14).color(MUTED).into()
            }
            Some(cached) if !cached.synced.is_empty() => {
                // +1ms truncation guard: click times and clock values pass
                // through floats, and a hair below the boundary would
                // highlight the previous verse.
                let pos_ms = (self.position_secs * 1000.0 + 1.0) as u64;
                // Empty-text stanza markers never win the highlight and
                // are not clickable: filter them, render gaps instead.
                let visible: Vec<&yutu_core::LyricLine> = cached
                    .synced
                    .iter()
                    .filter(|l| !l.text.trim().is_empty())
                    .collect();
                let owned: Vec<yutu_core::LyricLine> =
                    visible.iter().map(|l| (*l).clone()).collect();
                let at = yutu_core::line_at(&owned, pos_ms);
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
                            icon("music", 15.0, SPOT_GREEN),
                            text(l.text.clone())
                                .size(19)
                                .line_height(iced::widget::text::LineHeight::Relative(1.4),)
                                .font(iced::Font {
                                    weight: iced::font::Weight::Bold,
                                    ..iced::Font::default()
                                })
                                .color(SPOT_GREEN),
                        ]
                        .spacing(10)
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                    } else {
                        row![
                            iced::widget::Space::new().width(iced::Length::Fixed(25.0)),
                            text(l.text.clone())
                                .size(15)
                                .line_height(iced::widget::text::LineHeight::Relative(1.5),)
                                .color(iced::Color::from_rgb8(0xD6, 0xD6, 0xD6)),
                        ]
                        .align_y(iced::alignment::Vertical::Center)
                        .into()
                    };
                    let row_el: iced::Element<'_, Message> = button(line)
                        .style(hover_row)
                        .on_press(Message::SeekToMs(ms))
                        .padding(4)
                        .width(iced::Fill)
                        .into();
                    rows.push(row_el);
                }
                column(rows).spacing(6).into()
            }
            Some(cached) => column(
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
            None if self.lyrics_loading.as_deref() == Some(id) => {
                text("Loading lyrics").color(MUTED).into()
            }
            None => text("No lyrics for this track yet.").color(MUTED).into(),
        }
    }

    /// Now Playing drawer content (also the NowPlaying route): big art,
    /// centered song info, then the synced lyrics home. No queue lists
    /// (the sidebar owns those), no Lyrics/Radio buttons, no timings.
    fn drawer_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, mouse_area, row, scrollable, text};
        use iced::Fill;
        let Some(t) = self.queue.current().cloned() else {
            return container(
                column![
                    text("Nothing playing").size(22),
                    text("Search above, click a song, hear it.").color(MUTED),
                ]
                .spacing(8),
            )
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
                    SPOT_GREEN
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
        let body: iced::Element<'_, Message> = column![
            sticky,
            scrollable(
                column![
                    container(hero)
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
    const NAV_BTN_W: f32 = 42.0;
    /// Outer width of the settings gear (14px icon + 2x6 padding).
    const SETTINGS_W: f32 = 26.0;

    /// Window-relative search pill width (capped, floored).
    fn search_width(&self) -> f32 {
        self.window
            .map(|s| {
                (s.width * 0.42)
                    .clamp(200.0, 640.0)
                    .min((s.width - 220.0).max(200.0))
            })
            .unwrap_or(560.0)
    }

    /// Pill left edge in window coords (base pad + sidebar + gaps + navs).
    fn search_pill_left(&self) -> f32 {
        8.0 + self.sidebar_w + 8.0 + Self::NAV_BTN_W + 4.0 + Self::NAV_BTN_W + 4.0
    }

    /// Global header (nav + search + settings), shared by the center and
    /// the drawer-open state so the search bar never vanishes.
    fn header_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, container, row, text, text_input};
        use iced::Fill;
        let can_back = !self.nav_back.is_empty();
        let can_forward = !self.nav_forward.is_empty();
        let back_btn: iced::Element<'_, Message> = if can_back {
            button(icon("chevron-left", 22.0, iced::Color::WHITE))
                .style(nav_arrow)
                .on_press(Message::NavBack)
                .padding(10)
                .into()
        } else {
            button(icon("chevron-left", 22.0, MUTED))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let fwd_btn: iced::Element<'_, Message> = if can_forward {
            button(icon("chevron-right", 22.0, iced::Color::WHITE))
                .style(nav_arrow)
                .on_press(Message::NavForward)
                .padding(10)
                .into()
        } else {
            button(icon("chevron-right", 22.0, MUTED))
                .style(nav_arrow)
                .padding(10)
                .into()
        };
        let clear_btn: iced::Element<'_, Message> = if self.search_query.is_empty() {
            text("").size(1).into()
        } else {
            button(icon("x", 13.0, MUTED))
                .style(hover_row)
                .on_press(Message::HomeSearchChanged(String::new()))
                .padding(6)
                .into()
        };
        let search_state: iced::Element<'_, Message> = if self.searching {
            text("…").size(14).color(MUTED).into()
        } else {
            text("").size(1).into()
        };
        let search_w = self.search_width();
        let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
        let chrome = Self::NAV_BTN_W + 4.0 + Self::NAV_BTN_W + 4.0 + Self::SETTINGS_W + 4.0;
        // Main row gaps: sidebar|center plus center|right when visible.
        // Plus the header inner spacing (4 gaps of 4 in fixed layout).
        let gaps = if self.right_target() != 0.0 {
            16.0
        } else {
            8.0
        };
        let center_avail = ww - 16.0 - self.sidebar_w - gaps - self.right_target() - chrome - 16.0;
        let pill_w = if center_avail < search_w {
            iced::Length::Fill
        } else {
            iced::Length::Fixed(search_w)
        };
        let fixed_pill = center_avail >= search_w;
        let pill: iced::Element<'_, Message> = container(
            row![
                icon("search", 14.0, MUTED),
                text_input("Search YouTube Music…", &self.search_query)
                    .style(search_inner_input)
                    .id("yutu-search")
                    .on_input(Message::HomeSearchChanged)
                    .on_submit(Message::SearchStay)
                    .width(Fill)
                    .padding(10),
                clear_btn,
                search_state,
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
        let gear: iced::Element<'_, Message> = button(icon("settings", 14.0, iced::Color::WHITE))
            .style(hover_row)
            .on_press(Message::Navigate(Route::Settings))
            .padding(6)
            .into();
        // Fixed pill: Fill space docks the gear to the header right edge.
        // Fill pill: it already stretches, no space needed.
        let mut h = iced::widget::Row::with_children(vec![back_btn, fwd_btn, pill]);
        if fixed_pill {
            let spacer: iced::Element<'_, Message> = iced::widget::Space::new().width(Fill).into();
            h = h.push(spacer);
        }
        h = h.push(gear);
        h.spacing(4)
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }

    fn center_view(&self) -> iced::Element<'_, Message> {
        use iced::widget::text::Wrapping;
        use iced::widget::{
            button, checkbox, column, container, mouse_area, row, scrollable, slider, text,
            text_input,
        };
        use iced::Fill;
        let body: iced::Element<'_, Message> = match &self.route {
            Route::Home => {
                let mut seen = std::collections::HashSet::new();
                let mut recent: Vec<Track> = Vec::new();
                for id in self.history.iter().rev() {
                    if recent.len() >= 8 {
                        break;
                    }
                    if seen.insert(id.clone()) {
                        if let Some(t) = self.find_track(id) {
                            recent.push(t);
                        }
                    }
                }
                // Liked tracks fill an empty history so the grid is alive
                // (add order, deterministic even past 8).
                if recent.is_empty() {
                    recent = self.favorite_tracks().into_iter().take(8).collect();
                }
                let picks: Vec<Track> = recent;
                let grid: iced::Element<'_, Message> = if picks.is_empty() {
                    column![
                        text("Your recent plays will appear here.").color(MUTED),
                        text("Type in the search bar above: results land as you type.")
                            .size(12)
                            .color(MUTED),
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
                                button(
                                    column![
                                        self.thumb_hero(t, HOME_PICK_ART),
                                        column![
                                            text(truncate_title(&t.title, 20))
                                                .size(13)
                                                .width(Fill)
                                                .wrapping(Wrapping::None),
                                            text(truncate_title(&t.artist, 22))
                                                .size(11)
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
                        .on_scroll(|_| Message::Scrolled("home"))
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
                let home_hits: iced::Element<'_, Message> =
                    if self.searching && self.results.tracks.is_empty() {
                        text("Searching YouTube Music…").color(MUTED).into()
                    } else if self.search_query.trim().is_empty() {
                        text("").size(1).into()
                    } else if self.results.tracks.is_empty() {
                        text("No matches yet: keep typing.").color(MUTED).into()
                    } else {
                        let top: Vec<iced::Element<'_, Message>> = self
                            .results
                            .tracks
                            .iter()
                            .take(6)
                            .map(|t| self.track_row(t))
                            .collect();
                        column![
                            text("Top results").size(17),
                            column(top).spacing(2),
                            button(text("See all results →").size(12))
                                .style(hover_row)
                                .on_press(Message::Navigate(Route::Search))
                                .padding(4),
                        ]
                        .spacing(4)
                        .into()
                    };
                let home_search_results: iced::Element<'_, Message> = home_hits;
                let weekly_n = self.weekly_mix_tracks().len();
                let forgotten_n = self.forgotten_tracks().len();
                column![
                    text("Good evening").size(24),
                    home_search_results,
                    text("Jump back in").size(19),
                    grid,
                    row![
                        text("Made for you").size(19).width(Fill),
                        button(text("Discovery →").size(12))
                            .style(hover_row)
                            .on_press(Message::Navigate(Route::Discovery))
                            .padding(4),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    row![
                        button(
                            column![
                                text("Weekly Mix").size(15),
                                text(format!("{weekly_n} tracks")).size(12).color(MUTED),
                            ]
                            .spacing(2)
                            .padding(12)
                            .width(Fill),
                        )
                        .style(card_button)
                        .on_press(Message::PlayWeeklyMix)
                        .padding(0),
                        button(
                            column![
                                text("Forgotten Favorites").size(15),
                                text(format!("{forgotten_n} tracks")).size(12).color(MUTED),
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
                // Blank until a searchable query: single characters can
                // never have results (no network fires below 2 chars), so
                // they blank instead of flashing "no results".
                // Every branch below is page content only: the header
                // (search input) and scroll wrapper always render.
                let content: iced::Element<'_, Message> = if query.len() < 2 {
                    iced::widget::Space::new().width(Fill).height(Fill).into()
                } else if self.searching
                    && results_empty(&self.results)
                    && all_empty(&self.all_results)
                {
                    Self::search_loading()
                } else if results_empty(&self.results) && all_empty(&self.all_results) {
                    column![text("No results found: try another spelling.").color(MUTED),]
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
                                .map(|(i, t)| self.numbered_track_row(t, i + 1))
                                .collect();
                            let list: iced::Element<'_, Message> = if songs.is_empty() {
                                text(format!("No songs for “{query}”.")).color(MUTED).into()
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
                                text(format!("No albums for “{query}”."))
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
                                text(format!("No artists for “{query}”."))
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
                                text(format!("No playlists for “{query}”."))
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
            Route::Queue => {
                let pos = self.queue.position();
                let header: iced::Element<'_, Message> = match self.queue.current() {
                    Some(t) => column![
                        text("Now playing").size(12).color(MUTED),
                        text(format!("{} · {}", t.title, t.artist)).size(17),
                        text(format!(
                            "Track {} of {} · {} autoplay · Shuffle {} · Repeat {:?}",
                            pos.map(|p| p + 1).unwrap_or(0),
                            self.queue.len(),
                            self.autoplay.len(),
                            if self.queue.shuffle { "on" } else { "off" },
                            self.queue.repeat,
                        ))
                        .size(12)
                        .color(MUTED),
                    ]
                    .spacing(2)
                    .into(),
                    None => text("Queue is empty: search for music first.")
                        .color(MUTED)
                        .into(),
                };
                let rows: Vec<iced::Element<'_, Message>> = self
                    .queue
                    .items()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        use iced::widget::{button, mouse_area, row, text};
                        use iced::Fill;
                        let is_current = Some(i) == pos;
                        let marker = if is_current { "● " } else { "" };
                        let jump = t.id.clone();
                        let remove_id = t.id.clone();
                        // The remove button only exists on hover (placeholder holds
                        // the layout so rows never jump).
                        let remove_btn: iced::Element<'_, Message> =
                            if self.hovered_queue == Some(i) {
                                button(icon("x", 12.0, MUTED))
                                    .style(hover_row)
                                    .on_press(Message::RemoveFromQueue(remove_id))
                                    .padding(5)
                                    .into()
                            } else {
                                iced::widget::Space::new()
                                    .width(iced::Length::Fixed(24.0))
                                    .into()
                            };
                        mouse_area(
                            container(
                                row![
                                    button(self.thumb(t))
                                        .style(row_click)
                                        .on_press(Message::QueueJump(i))
                                        .padding(2),
                                    button(Self::track_text_single(
                                        &format!(
                                            "{marker}{}. {}",
                                            i + 1,
                                            truncate_title(&t.title, 55)
                                        ),
                                        &t.artist,
                                        14,
                                        if is_current {
                                            SPOT_GREEN
                                        } else {
                                            iced::Color::WHITE
                                        },
                                    ))
                                    .style(row_click)
                                    .on_press(Message::QueueJump(i))
                                    .padding(4)
                                    .width(Fill),
                                    button(icon("arrow-up", 12.0, MUTED))
                                        .style(hover_row)
                                        .on_press(Message::QueueUp(i))
                                        .padding(5),
                                    button(icon("arrow-down", 12.0, MUTED))
                                        .style(hover_row)
                                        .on_press(Message::QueueDown(i))
                                        .padding(5),
                                    button(
                                        text(fmt_time(t.duration_secs as f64))
                                            .size(12)
                                            .color(MUTED),
                                    )
                                    .style(row_click)
                                    .on_press(Message::QueueJump(i))
                                    .padding(5),
                                    remove_btn,
                                ]
                                .spacing(4)
                                .align_y(iced::alignment::Vertical::Center)
                                .padding(2),
                            )
                            .style(row_bg(self.hovered_queue == Some(i)))
                            .width(Fill)
                            .height(iced::Length::Fixed(TRACK_ROW_H))
                            .clip(true)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .on_right_press(Message::OpenContext(jump))
                        .on_enter(Message::QueueHover(Some(i)))
                        .on_exit(Message::QueueHover(None))
                        .into()
                    })
                    .collect();
                let auto_rows: Vec<iced::Element<'_, Message>> = self
                    .autoplay
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        use iced::widget::{button, container, mouse_area, row, text};
                        use iced::Fill;
                        let menu_id = t.id.clone();
                        mouse_area(
                            container(
                                row![
                                    button(Self::track_text_single(
                                        &format!("{}. {}", i + 1, truncate_title(&t.title, 55)),
                                        &t.artist,
                                        13,
                                        iced::Color::WHITE
                                    ))
                                    .style(row_click)
                                    .on_press(Message::PlayAutoplay(i))
                                    .padding(4)
                                    .width(Fill),
                                    text(fmt_time(t.duration_secs as f64)).size(11).color(MUTED),
                                    button(icon("x", 12.0, MUTED))
                                        .style(hover_row)
                                        .on_press(Message::RemoveAutoplay(i))
                                        .padding(5),
                                ]
                                .spacing(4)
                                .align_y(iced::alignment::Vertical::Center)
                                .padding(2),
                            )
                            .style(row_bg(self.hovered_auto == Some(i)))
                            .width(Fill)
                            .height(iced::Length::Fixed(AUTO_ROW_H))
                            .clip(true)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .on_right_press(Message::OpenContext(menu_id))
                        .on_enter(Message::AutoHover(Some(i)))
                        .on_exit(Message::AutoHover(None))
                        .into()
                    })
                    .collect();
                let auto_section: iced::Element<'_, Message> = if auto_rows.is_empty() {
                    text("").size(1).into()
                } else {
                    column![
                        text("Autoplay: similar, plays after your queue")
                            .size(13)
                            .color(MUTED),
                        column(auto_rows).spacing(2),
                    ]
                    .spacing(4)
                    .into()
                };
                column![
                    row![
                        text("Up Next").size(22).width(Fill),
                        button(
                            row![
                                icon("shuffle", 12.0, iced::Color::WHITE),
                                text(if self.queue.shuffle { "On" } else { "Off" }).size(12),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(hover_row)
                        .on_press(Message::ToggleShuffle)
                        .padding(6),
                        button(
                            row![
                                icon("repeat", 12.0, iced::Color::WHITE),
                                text(format!("{:?}", self.queue.repeat)).size(12),
                            ]
                            .spacing(4)
                            .align_y(iced::alignment::Vertical::Center),
                        )
                        .style(hover_row)
                        .on_press(Message::CycleRepeat)
                        .padding(6),
                        button(text("Clear").size(12))
                            .style(hover_row)
                            .on_press(Message::QueueClear)
                            .padding(6),
                    ]
                    .spacing(4)
                    .align_y(iced::alignment::Vertical::Center),
                    header,
                    text("Queue: your picks").size(13).color(MUTED),
                    column(with_dividers(rows)).spacing(0),
                    auto_section,
                ]
                .spacing(10)
                .into()
            }
            Route::Library => {
                // Add order like Favorites so both orders match.
                let liked_tracks: Vec<Track> = self.favorite_tracks();
                let liked_missing = self.liked.len().saturating_sub(liked_tracks.len());
                let liked: Vec<iced::Element<'_, Message>> =
                    liked_tracks.iter().map(|t| self.track_row(t)).collect();
                let liked_note: iced::Element<'_, Message> = if liked_missing > 0 {
                    text(format!(
                        "{liked_missing} liked song(s) not in cache: search for them."
                    ))
                    .size(12)
                    .color(MUTED)
                    .into()
                } else {
                    text("").size(1).into()
                };
                let playlists: Vec<iced::Element<'_, Message>> = self
                    .all_playlists()
                    .into_iter()
                    .map(|p| {
                        use iced::widget::{button, row, text};
                        use iced::Fill;
                        row![
                            column![
                                text(p.title.clone()).size(14),
                                text(format!("{} tracks", p.tracks.len()))
                                    .size(12)
                                    .color(MUTED),
                            ]
                            .spacing(2)
                            .width(Fill),
                            button(text("Open").size(12))
                                .style(hover_row)
                                .on_press(Message::OpenPlaylist(p.id.clone()))
                                .padding(6),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .padding(6)
                        .into()
                    })
                    .collect();
                let new_name = self.new_playlist_name.clone();
                column![
                    text("Your Library").size(22),
                    text("Liked").size(19),
                    liked_note,
                    column(liked).spacing(2),
                    Self::section("Playlists", playlists),
                    row![
                        text_input("New playlist name…", &self.new_playlist_name)
                            .style(rounded_input)
                            .on_input(Message::NewPlaylistNameChanged)
                            .width(Fill)
                            .padding(10),
                        button(text("Create").size(13))
                            .style(green_play)
                            .on_press(Message::CreatePlaylist(new_name))
                            .padding(8),
                    ]
                    .spacing(8)
                    .align_y(iced::alignment::Vertical::Center),
                ]
                .spacing(10)
                .into()
            }
            Route::Discovery => {
                let weekly: Vec<iced::Element<'_, Message>> = self
                    .weekly_mix_tracks()
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let forgotten: Vec<iced::Element<'_, Message>> = self
                    .forgotten_tracks()
                    .iter()
                    .map(|t| self.track_row(t))
                    .collect();
                let weekly_section: iced::Element<'_, Message> = if weekly.is_empty() {
                    text("Play and like some music first.").color(MUTED).into()
                } else {
                    column(weekly).spacing(2).into()
                };
                let forgotten_section: iced::Element<'_, Message> = if forgotten.is_empty() {
                    text("Like some music to fill this.").color(MUTED).into()
                } else {
                    column(forgotten).spacing(2).into()
                };
                column![
                    text("Made for you").size(22),
                    row![
                        text("Weekly Mix").size(17).width(Fill),
                        button(
                            row![
                                icon("play", 13.0, iced::Color::WHITE),
                                text("Play all").size(13)
                            ]
                            .spacing(6)
                        )
                        .style(green_play)
                        .on_press(Message::PlayWeeklyMix)
                        .padding(7),
                    ]
                    .align_y(iced::alignment::Vertical::Center),
                    weekly_section,
                    row![
                        text("Forgotten Favorites").size(17).width(Fill),
                        button(
                            row![
                                icon("play", 13.0, iced::Color::WHITE),
                                text("Play all").size(13)
                            ]
                            .spacing(6)
                        )
                        .style(green_play)
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
                let current_eq = self.eq_preset.clone();
                let eq_buttons: Vec<iced::Element<'_, Message>> = eq_names
                    .into_iter()
                    .map(|n| {
                        let label = if n == current_eq {
                            format!("● {n}")
                        } else {
                            n.clone()
                        };
                        button(text(label).size(12))
                            .style(hover_row)
                            .on_press(Message::SelectEqPreset(n))
                            .padding(5)
                            .into()
                    })
                    .collect();
                column![
                    text("Settings").size(22),
                    text("Playback").size(17),
                    column![
                        text(format!("Volume {:.0}%", self.volume * 100.0)).size(13),
                        slider(0.0..=1.0, self.volume, Message::SetVolume)
                            .step(0.01_f32)
                            .style(themed_slider)
                            .width(Fill),
                    ]
                    .spacing(4),
                    row(eq_buttons).spacing(4),
                    text(format!("EQ: {current_eq} · {}", self.eq_note()))
                        .size(12)
                        .color(MUTED),
                    text("Animations").size(17),
                    checkbox(self.animations_enabled)
                        .label("Enable animations (off = panels snap instantly)")
                        .on_toggle(Message::AnimationsToggled),
                    column![
                        text(format!("Animation duration: {} ms", self.anim_ms)).size(13),
                        slider(
                            ANIM_MIN_MS as f32..=ANIM_MAX_MS as f32,
                            self.anim_ms as f32,
                            Message::AnimMsChanged
                        )
                        .step(5.0_f32)
                        .style(themed_slider)
                        .width(Fill),
                    ]
                    .spacing(4),
                    text("Scrobbling (submits live when credentials are set)").size(17),
                    checkbox(self.scrobble_enabled)
                        .label("Enable scrobbling")
                        .on_toggle(Message::ScrobbleToggled),
                    text_input("Display name (informational)…", &self.scrobble_user)
                        .style(rounded_input)
                        .on_input(Message::ScrobbleUserChanged)
                        .padding(10),
                    text_input("ListenBrainz token…", &self.scrobble_lb_token)
                        .style(rounded_input)
                        .on_input(Message::ScrobbleLbTokenChanged)
                        .secure(true)
                        .padding(10),
                    text_input("Last.fm API key…", &self.scrobble_fm_key)
                        .style(rounded_input)
                        .on_input(Message::ScrobbleFmKeyChanged)
                        .padding(10),
                    text_input("Last.fm shared secret…", &self.scrobble_fm_secret)
                        .style(rounded_input)
                        .on_input(Message::ScrobbleFmSecretChanged)
                        .secure(true)
                        .padding(10),
                    text_input("Last.fm session key…", &self.scrobble_fm_sk)
                        .style(rounded_input)
                        .on_input(Message::ScrobbleFmSkChanged)
                        .secure(true)
                        .padding(10),
                    text("Google account (import your library)").size(17),
                    self.google_section(),
                    text("Cookie import (no OAuth client needed)").size(17),
                    self.cookie_section(),
                    text("Sync (web changes into the app)").size(17),
                    self.sync_section(),
                    self.error_section(),
                    text("Shortcuts: Space play/pause · n next · p prev · / search")
                        .size(12)
                        .color(MUTED),
                ]
                .spacing(10)
                .into()
            }
            Route::Artist(id) => {
                let name = self.resolve_artist_name(id);
                let tracks = self.artist_tracks(&name);
                let albums = self.artist_albums(&name);
                let total: u32 = tracks.iter().map(|t| t.duration_secs).sum();
                let popular: Vec<iced::Element<'_, Message>> = tracks
                    .iter()
                    .take(10)
                    .enumerate()
                    .map(|(i, t)| self.numbered_track_row(t, i + 1))
                    .collect();
                let cards: Vec<iced::Element<'_, Message>> =
                    albums.iter().take(5).map(|a| self.album_card(a)).collect();
                let radio_btn: iced::Element<'_, Message> = match tracks.first() {
                    None => text("").size(1).into(),
                    Some(seed) => button(
                        row![
                            icon("radio", 13.0, iced::Color::WHITE),
                            text("Radio").size(13),
                        ]
                        .spacing(6)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(hover_row)
                    .on_press(Message::StartRadio(seed.id.clone()))
                    .padding(8)
                    .into(),
                };
                column![
                    row![
                        self.art_thumb(self.artist_photo(&name).as_deref(), 160.0),
                        column![
                            text("Artist").size(12).color(MUTED),
                            text(name.clone()).size(30),
                            text(format!(
                                "{} songs · {} albums · {}",
                                tracks.len(),
                                albums.len(),
                                fmt_time(total as f64)
                            ))
                            .size(12)
                            .color(MUTED),
                            row![
                                Self::play_pill("PLAY".into(), Message::PlayAll),
                                Self::shuffle_pill("SHUFFLE".into(), Message::PlayAllShuffled),
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
                    Self::section("Popular", popular),
                    Self::section(
                        "Albums",
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
                let tracks = self.album_tracks(id);
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
                // One policy: the same hires key album_cover returns and
                // stage_search warms (raw would placeholder on some paths).
                let cover = tracks
                    .first()
                    .and_then(|t| t.thumbnail_url.clone())
                    .or_else(|| {
                        self.results
                            .albums
                            .iter()
                            .find(|a| a.title == title)
                            .and_then(|a| a.thumbnail_url.clone())
                    })
                    .map(|u| yutu_core::hires_thumb_url(&u, 544));
                let rows: Vec<iced::Element<'_, Message>> = tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.numbered_track_row(t, i + 1))
                    .collect();
                let meta = if year.is_empty() {
                    format!(
                        "{} · {} songs · {}",
                        artist,
                        tracks.len(),
                        fmt_time(total as f64)
                    )
                } else {
                    format!(
                        "{} · {} · {} songs · {}",
                        artist,
                        year,
                        tracks.len(),
                        fmt_time(total as f64)
                    )
                };
                column![
                    row![
                        self.art_thumb(cover.as_deref(), 180.0),
                        column![
                            text("Album").size(12).color(MUTED),
                            text(title.clone()).size(28),
                            text(meta).size(12).color(MUTED),
                            row![
                                Self::play_pill("PLAY".into(), Message::PlayAll),
                                Self::shuffle_pill("SHUFFLE".into(), Message::PlayAllShuffled),
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
                None => column![text(format!("Playlist not found: {id}")).size(20)]
                    .spacing(8)
                    .into(),
                Some(p) => {
                    let pid = p.id.clone();
                    let rows: Vec<iced::Element<'_, Message>> = p
                        .tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| self.playlist_track_row(&pid, t, i))
                        .collect();
                    column![
                        text(p.title.clone()).size(24),
                        text(format!("Playlist · {} tracks", p.tracks.len()))
                            .size(12)
                            .color(MUTED),
                        row![
                            button(
                                row![
                                    icon("play", 13.0, iced::Color::WHITE),
                                    text("Play all").size(13)
                                ]
                                .spacing(6)
                            )
                            .style(green_play)
                            .on_press(Message::PlayAll)
                            .padding(8),
                            button(
                                row![
                                    icon("shuffle", 13.0, iced::Color::WHITE),
                                    text("Shuffle").size(13)
                                ]
                                .spacing(6)
                            )
                            .style(green_play)
                            .on_press(Message::PlayAllShuffled)
                            .padding(8),
                            button(icon("ellipsis", 15.0, MUTED))
                                .style(hover_row)
                                .on_press(Message::OpenPlaylistMenu(p.id.clone()))
                                .padding(8),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                        column(rows).spacing(2),
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
                let content = self.lyrics_content(id);
                column![
                    text(title).size(22),
                    row![
                        button(
                            row![
                                icon("play", 13.0, iced::Color::WHITE),
                                text("Play").size(13)
                            ]
                            .spacing(6)
                        )
                        .style(green_play)
                        .on_press(Message::PlayNow(id.clone()))
                        .padding(7),
                        button(text("Radio").size(13))
                            .style(hover_row)
                            .on_press(Message::StartRadio(id.clone()))
                            .padding(7),
                    ]
                    .spacing(8),
                    content,
                ]
                .spacing(10)
                .into()
            }
            Route::NowPlaying => self.drawer_view(),
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
                    text("Radio").size(12).color(MUTED),
                    button(
                        row![
                            icon("play", 13.0, iced::Color::WHITE),
                            text("Start radio").size(13)
                        ]
                        .spacing(6)
                    )
                    .style(green_play)
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
            .on_scroll(|_| Message::Scrolled("body"))
            .height(Fill)
            .into()
        } else {
            scrollable(container(body).padding(20).width(Fill))
                .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                .style(move |t, s| slim_scroll_alpha(t, s, self.scroll_opacity("body")))
                .on_scroll(|_| Message::Scrolled("body"))
                .height(Fill)
                .into()
        };
        column![header, scrolled,].spacing(4).into()
    }

    /// Search popup content, if the gate holds (window-level layer above
    /// the drawer; dismissed by scrim, Esc, navigation, or any action).
    fn suggest_popup(&self) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{button, column, container, row, text};
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
                .map(|sug| {
                    button(
                        row![
                            icon("search", 13.0, MUTED),
                            text(sug.clone()).size(13).width(Fill),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center),
                    )
                    .style(hover_row)
                    .on_press(Message::HomeSearchChanged(sug.clone()))
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
                .map(|t| self.suggest_track_row(t))
                .collect();
            let progress: iced::Element<'_, Message> = if self.searching {
                text("Searching…").size(11).color(SPOT_GREEN).into()
            } else {
                text("Top tracks: Enter for all results")
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
        use iced::widget::{button, column, container, mouse_area, row, text, Row, Stack};
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
        // An empty collapsed rail reserves nothing: not even row spacing.
        let mut main_children = vec![self.sidebar_view(), center];
        if self.right_target() != 0.0 {
            main_children.push(self.right_panel_view());
        }
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
        // Search popup above everything in the center (including the
        // drawer), below the context menu. The scrim leaves a top strip
        // hole so the search input stays clickable; anything else
        // outside dismisses like a context menu.
        if let Some(popup) = self.suggest_popup() {
            let search_w = self.search_width();
            let ww = self.window.map(|s| s.width).unwrap_or(1280.0);
            // Same shrink contract as the header pill so narrow windows
            // never get a popup wider than the pill.
            let chrome = Self::NAV_BTN_W + 4.0 + Self::NAV_BTN_W + 4.0 + Self::SETTINGS_W + 4.0;
            let gaps = if self.right_target() != 0.0 {
                16.0
            } else {
                8.0
            };
            let center_avail =
                ww - 16.0 - self.sidebar_w - gaps - self.right_target() - chrome - 16.0;
            let pill_actual = if center_avail < search_w {
                center_avail.max(140.0)
            } else {
                search_w
            };
            // Aligned to the real pill edges, tracking sidebar width.
            let pop_w = pill_actual.min(ww).max(0.0);
            let pop_left = self.search_pill_left().clamp(0.0, (ww - pop_w).max(0.0));
            let suggest_scrim: iced::Element<'_, Message> = container(column![
                iced::widget::Space::new().height(iced::Length::Fixed(SUGGEST_SCRIM_TOP)),
                mouse_area(container(text("").size(1)).width(Fill).height(Fill))
                    .on_press(Message::CloseSuggest)
                    .on_enter(Message::ClearHovers)
                    .on_exit(Message::ClearHovers),
            ])
            .width(Fill)
            .height(Fill)
            .into();
            let suggest_placed: iced::Element<'_, Message> = container(column![
                iced::widget::Space::new().height(iced::Length::Fixed(66.0)),
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
                    (SPOT_GREEN, "Remove from favorites")
                } else {
                    (MUTED, "Add to favorites")
                };
                let (dl_icon, dl_label, dl_msg) = if self.downloaded.contains(&t.id) {
                    (
                        "trash",
                        "Remove download",
                        Message::RemoveFromPlaylist {
                            playlist_id: Self::DOWNLOADED_ID.into(),
                            track_id: t.id.clone(),
                        },
                    )
                } else {
                    (
                        "download",
                        "Download for offline",
                        Message::DownloadTrack(t.id.clone()),
                    )
                };
                let mut items: Vec<iced::Element<'_, Message>> = vec![
                    button(
                        row![
                            icon("play", 14.0, iced::Color::WHITE),
                            text("Play now").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::PlayNow(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("list-plus", 14.0, iced::Color::WHITE),
                            text("Play next").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::PlayNext(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("plus", 14.0, iced::Color::WHITE),
                            text("Add to queue").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::QueueTrack(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("radio", 14.0, iced::Color::WHITE),
                            text("Start mix").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::StartRadio(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("mic", 14.0, iced::Color::WHITE),
                            text("Lyrics").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenLyrics(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            heart_icon(self.liked.contains(&t.id), 14.0, heart_color),
                            text(heart).size(14)
                        ]
                        .spacing(8)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::Like(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    {
                        let disliked = self.disliked.contains(&t.id);
                        let (thumb_color, thumb_label) = if disliked {
                            (SPOT_GREEN, "Recommend again")
                        } else {
                            (MUTED, "Don't recommend")
                        };
                        button(
                            row![
                                thumbs_down_icon(disliked, 14.0, thumb_color),
                                text(thumb_label).size(14)
                            ]
                            .spacing(8)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::Dislike(t.id.clone()))
                        .padding(8)
                        .width(Fill)
                        .into()
                    },
                    button(
                        row![
                            icon(dl_icon, 14.0, iced::Color::WHITE),
                            text(dl_label).size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(dl_msg)
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("disc", 14.0, iced::Color::WHITE),
                            text("Go to album").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenTrackAlbum(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                    button(
                        row![
                            icon("user-round", 14.0, iced::Color::WHITE),
                            text("Go to artist").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::OpenTrackArtist(t.id.clone()))
                    .padding(8)
                    .width(Fill)
                    .into(),
                ];
                if self.queue.items().iter().any(|q| q.id == t.id) {
                    items.push(
                        button(
                            row![
                                icon("trash", 14.0, iced::Color::WHITE),
                                text("Remove from queue").size(14)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RemoveFromQueue(t.id.clone()))
                        .padding(8)
                        .width(Fill)
                        .into(),
                    );
                }
                if let Some(ai) = self.autoplay.iter().position(|q| q.id == t.id) {
                    items.push(
                        button(
                            row![
                                icon("trash", 14.0, iced::Color::WHITE),
                                text("Remove from autoplay").size(14)
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::RemoveAutoplay(ai))
                        .padding(8)
                        .width(Fill)
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
                // Its index below anchors the flyout height.
                let sub_id = t.id.clone();
                let sub_open = self.submenu_playlists_for.as_deref() == Some(t.id.as_str());
                let toggle_idx = items.len();
                items.push(
                    button(
                        row![
                            icon("queue", 14.0, iced::Color::WHITE),
                            text(format!("Playlists ({})", membership.len()))
                                .size(14)
                                .width(Fill),
                            icon("chevron-right", 14.0, MUTED),
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(if sub_open { selected_row } else { hover_row })
                    .on_press(Message::TogglePlaylistSubmenu(sub_id))
                    .padding(8)
                    .width(Fill)
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
                // Click-toggled (hover-open fights the frozen anchor); the
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
                                    SPOT_GREEN
                                } else {
                                    MUTED
                                }),
                            ]
                            .spacing(1)
                            .width(Fill)
                            .into(),
                        );
                        fly_rows.push(
                            button(
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
                            text("No playlists yet: create one in Library.")
                                .size(12)
                                .color(MUTED)
                                .into(),
                        );
                    }
                    let fly_h = ((fly_rows.len() as f32) * 44.0).min(340.0);
                    let flyout: iced::Element<'_, Message> = container(
                        iced::widget::scrollable(column(fly_rows).spacing(2).width(Fill))
                            .direction(iced::widget::scrollable::Direction::Vertical(slim_bar()))
                            .style(move |t, s| {
                                slim_scroll_alpha(t, s, self.scroll_opacity("flyout"))
                            })
                            .on_scroll(|_| Message::Scrolled("flyout"))
                            .height(iced::Length::Fixed(fly_h))
                            .width(Fill),
                    )
                    .style(card_bg)
                    .padding(10)
                    .width(iced::Length::Fixed(280.0))
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
                    // Aligned with the Playlists toggle row: menu header
                    // plus the rows above its recorded index. Bumped
                    // automatically since the index is measured, not magic.
                    let header_h = 78.0;
                    let row_h = 37.0;
                    let my2 = (my + header_h + toggle_idx as f32 * row_h)
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
                    button(
                        row![
                            icon("play", 14.0, iced::Color::WHITE),
                            text("Play").size(14),
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
                    button(
                        row![
                            icon("plus", 14.0, iced::Color::WHITE),
                            text("Add to queue").size(14),
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
                    button(
                        row![
                            icon("shuffle", 14.0, iced::Color::WHITE),
                            text("Shuffle play").size(14),
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
                if pid3 != Self::FAVORITES_ID && pid3 != Self::DOWNLOADED_ID {
                    let pid4 = pid3.clone();
                    pl_items.push(
                        button(
                            row![
                                icon("trash", 14.0, iced::Color::WHITE),
                                text("Delete playlist").size(14),
                            ]
                            .spacing(8)
                            .align_y(iced::alignment::Vertical::Center)
                            .width(Fill),
                        )
                        .style(hover_row)
                        .on_press(Message::DeletePlaylist(pid4))
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
                                text(format!("Playlist · {} tracks", p.tracks.len()))
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
        // Search collection menu: same frozen-anchor scrim pattern, no
        // flyout. The hit resolves live: results may have moved on since
        // the menu opened, and then nothing renders.
        if let Some(menu) = self.coll_menu.clone() {
            let hit: Option<(iced::Element<'_, Message>, String, String)> = match &menu {
                CollectionMenu::Album(id) => {
                    self.results.albums.iter().find(|a| &a.id == id).map(|a| {
                        let sub = match a.year {
                            Some(y) => format!("Album · {} · {}", a.artist, y),
                            None => format!("Album · {}", a.artist),
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
                            format!("Playlist · {} · {}", p.author, p.detail),
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
                                "Shuffle play".to_string(),
                                Message::PlayAlbumShuffled(id.clone()),
                            ));
                            items.push(menu_item(
                                "radio",
                                "Start mix".to_string(),
                                Message::StartRadio(seed),
                            ));
                        }
                        if !artist.is_empty() {
                            items.push(menu_item(
                                "user-round",
                                "Go to artist".to_string(),
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
                                "Shuffle play".to_string(),
                                Message::PlayArtistShuffled(name),
                            ));
                            items.push(menu_item(
                                "radio",
                                "Start mix".to_string(),
                                Message::StartRadio(seed),
                            ));
                        }
                    }
                    CollectionMenu::Playlist(id) => {
                        items.push(menu_item(
                            "play",
                            "Play".to_string(),
                            Message::PlaySearchPlaylist(id.clone()),
                        ));
                        items.push(menu_item(
                            "shuffle",
                            "Shuffle play".to_string(),
                            Message::PlaySearchPlaylistShuffled(id),
                        ));
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
        // Always a Stack (even with one layer): flipping between bare
        // content and Stack would rebuild all widget state (search focus
        // and cursor included) every time a popup appears or vanishes.
        Stack::with_children(layers).into()
    }

    /// Now Playing sheet that overlays the center column (drawer slide).
    /// Gated on `drawer_anim` by the caller.
    fn drawer_layer(&self) -> iced::Element<'_, Message> {
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
                    button(icon("chevron-down", 14.0, iced::Color::WHITE))
                        .style(hover_row)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    text("Now playing").size(13).color(MUTED).width(Fill),
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
    fn drawer_body_layer(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, container, row, text};
        use iced::Fill;
        let win_h = self.window.map(|s| s.height).unwrap_or(800.0);
        let center_h = (win_h - 148.0).max(300.0);
        let sheet_h = (center_h - DRAWER_TOP_RESERVE).max(0.0);
        let offset = (1.0 - self.drawer_anim).max(0.0) * sheet_h;
        let sheet: iced::Element<'_, Message> = container(
            column![
                row![
                    button(icon("chevron-down", 14.0, iced::Color::WHITE))
                        .style(hover_row)
                        .on_press(Message::ToggleDrawer)
                        .padding(7),
                    text("Now playing").size(13).color(MUTED).width(Fill),
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

pub fn run(mpris: Option<yutu_desktop::SharedPlayer>) -> iced::Result {
    iced::application(
        move || {
            let (mut state, task) = AppState::new();
            state.mpris = mpris.clone();
            state.publish_mpris();
            (state, task)
        },
        AppState::update,
        AppState::view,
    )
    .title("yutu-mux")
    .theme(AppState::theme)
    .subscription(AppState::subscription)
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DB_SEQ: AtomicU64 = AtomicU64::new(0);

    /// Isolated state per test: never touches the real library.db.
    /// Fabricated track (no fixtures needed for most update-logic tests).
    fn mk_track(id: &str, title: &str, artist: &str) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
        }
    }

    /// Search payload with an empty All half (filter-tab tests only
    /// need the filtered depth, not server order).
    fn bundle_of(filtered: SearchResults) -> SearchBundle {
        SearchBundle {
            all: SearchAll::default(),
            filtered,
        }
    }

    fn test_state() -> AppState {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("yutu-test-{}-{}.db", std::process::id(), n));
        let (mut s, _) = AppState::with_library_path(path.to_string_lossy().into_owned());
        // Unit-test scaffolding: seed the catalog so id-based flows resolve.
        // User-visible views never read fixtures.
        for t in yutu_core::MockBackend::fixture_tracks() {
            s.catalog.insert(t.id.clone(), t);
        }
        s
    }

    /// Offline backend for staged-import tests: a fabricated OAuth token
    /// builds the client with no network (mirrors the core fixture).
    #[cfg(feature = "innertube")]
    fn test_backend() -> yutu_core::InnertubeBackend {
        let token: yutu_core::OAuthToken = serde_json::from_str(
            r#"{"token_type": "Bearer", "access_token": "ya29.test", "refresh_token": "1//test", "expires_in": 3599, "request_time": {"secs_since_epoch": 1790000000, "nanos_since_epoch": 0}, "client_id": "test-id", "client_secret": "test-secret"}"#,
        )
        .unwrap();
        yutu_core::InnertubeBackend::connect_with_oauth(&yutu_core::OAuthPersist::from_token(token))
            .unwrap()
    }

    /// Staged base payload for tests (headers as id/title pairs).
    #[cfg(feature = "innertube")]
    fn staged(
        tracks: Vec<Track>,
        liked_ids: Vec<String>,
        headers: Vec<(&str, &str)>,
    ) -> StagedImport {
        StagedImport {
            backend: test_backend(),
            tracks,
            liked_ids,
            disliked_ids: Vec::new(),
            headers: headers
                .into_iter()
                .map(|(id, title)| yutu_core::ServerPlaylist {
                    id: id.into(),
                    title: title.into(),
                })
                .collect(),
            headers_clean: true,
            library_ok: true,
        }
    }

    #[test]
    fn navigate() {
        let mut s = test_state();
        s.navigate(Route::Search);
        assert_eq!(s.route, Route::Search);
    }

    #[test]
    fn search_updates_results() {
        let mut s = test_state();
        // Real flow: keystrokes set the query before the payload lands.
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        // A completed real search lands in results + catalog.
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Yellow Submarine", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert_eq!(s.results.tracks.len(), 2);
        assert!(s.find_track("v1").is_some());
        assert!(!s.searching);
        // Keystrokes filter the landed payload locally (no network).
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.results.tracks[0].id, "v1");
        // New keystrokes paint premature catalog hits instantly.
        let _ = s.update(Message::HomeSearchChanged("come".into()));
        assert!(s.results.tracks.iter().any(|t| t.id == "m1"));
        // Failures for the current query report without clobbering.
        let _ = s.update(Message::SearchCompleted {
            query: "come".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("offline"));
        assert!(s.results.tracks.iter().any(|t| t.id == "m1"));
        // Stale-query failures stay silent.
        let _ = s.update(Message::SearchCompleted {
            query: "ancient".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("offline"));
    }

    #[test]
    fn search_preserves_play_counts() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        let mut hit = mk_track("v1", "Yesterday", "The Beatles");
        hit.plays = Some("213M plays".into());
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![hit],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        // Counts survive staging into results + catalog (popup reads them).
        assert_eq!(s.results.tracks[0].plays.as_deref(), Some("213M plays"));
        assert_eq!(
            s.find_track("v1").and_then(|t| t.plays).as_deref(),
            Some("213M plays")
        );
        // Local keystroke filtering keeps the counts.
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.results.tracks[0].plays.as_deref(), Some("213M plays"));
        // Long names build every touched row shape with no panic.
        let long = "y".repeat(120);
        let mut wide = mk_track("v9", &long, &long);
        wide.plays = Some("1B plays".into());
        let _ = s.view();
        let _ = s.suggest_track_row(&wide);
        let _ = s.track_row(&wide);
        let _ = s.queue_row(0);
        // Queue text boxes hold 2 title lines + 1 artist line always.
        let _ = AppState::track_text(&long, &long, 12, iced::Color::WHITE);
        let _ = AppState::track_text_single(&long, &long, 14, iced::Color::WHITE);
        // Home pick strip renders with real picks (long names truncate).
        s.history.push("m1".into());
        s.history.push("m2".into());
        let _ = s.view();
    }

    #[test]
    fn server_search_keeps_mixed_order() {
        use yutu_core::{Album, Artist, MixedRef, SearchAll};
        // The web row order survives verbatim: no client re-sort, even
        // when a playlist title exactly matches and a cover sorts
        // textually first.
        let mixed = |kind: u8, id: &str| MixedRef {
            kind,
            id: id.into(),
        };
        let all = SearchAll {
            top: None,
            tracks: vec![
                mk_track("v1", "Bohemian Rhapsody", "Queen"),
                mk_track("v2", "Queens", "Ruby Darkrose"),
            ],
            artists: vec![Artist {
                id: "r1".into(),
                name: "Queen".into(),
                thumbnail_url: None,
            }],
            albums: vec![Album {
                id: "a1".into(),
                title: "Bohemian Rhapsody".into(),
                artist: "Queen".into(),
                year: Some(2011),
                thumbnail_url: None,
            }],
            playlists: vec![PlaylistHit {
                id: "PL1".into(),
                title: "Bohemian Rhapsody".into(),
                author: "Fan Club".into(),
                detail: "50 songs".into(),
                thumbnail_url: None,
            }],
            mixed: vec![
                mixed(0, "v1"),
                mixed(1, "r1"),
                mixed(0, "v2"),
                mixed(2, "a1"),
                mixed(3, "PL1"),
            ],
        };
        // No top hit: hero falls back to the first track, rows skip it.
        let rows = order_all_hits(&all);
        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0], SearchHit::Artist(a) if a.id == "r1"));
        assert!(matches!(rows[1], SearchHit::Song(t) if t.id == "v2"));
        assert!(matches!(rows[2], SearchHit::Album(a) if a.id == "a1"));
        assert!(matches!(rows[3], SearchHit::Playlist(p) if p.id == "PL1"));
    }

    #[test]
    fn server_search_top_hit_resolves_to_hero() {
        use yutu_core::{Artist, MixedRef, SearchAll, TopHit, TopKind};
        let mixed = |kind: u8, id: &str| MixedRef {
            kind,
            id: id.into(),
        };
        let all = SearchAll {
            top: Some(TopHit {
                kind: TopKind::Video,
                id: "official".into(),
                title: "Chop Suey!".into(),
                artist: "System Of A Down".into(),
                thumbnail_url: None,
            }),
            tracks: vec![
                mk_track("cover", "Chop Suey", "Mister Chicken"),
                mk_track("official", "Chop Suey!", "System Of A Down"),
            ],
            artists: vec![Artist {
                id: "r1".into(),
                name: "System Of A Down".into(),
                thumbnail_url: None,
            }],
            albums: vec![],
            playlists: vec![],
            mixed: vec![mixed(0, "cover"), mixed(1, "r1"), mixed(0, "official")],
        };
        // Id resolution beats text order: the cover lists first but the
        // official video heroes.
        let hero = top_hero(&all).expect("hero resolves");
        assert!(matches!(hero, SearchHit::Song(t) if t.id == "official"));
        let rows = order_all_hits(&all);
        assert!(rows
            .iter()
            .any(|h| matches!(h, SearchHit::Song(t) if t.id == "cover")));
        assert!(!rows
            .iter()
            .any(|h| matches!(h, SearchHit::Song(t) if t.id == "official")));
        // Stale card id falls back to title match, then first row.
        let stale = SearchAll {
            top: Some(TopHit {
                kind: TopKind::Song,
                id: "gone".into(),
                title: "Chop Suey".into(),
                artist: "Mister Chicken".into(),
                thumbnail_url: None,
            }),
            ..all.clone()
        };
        assert!(matches!(top_hero(&stale), Some(SearchHit::Song(t)) if t.id == "cover"));
        let unknown = SearchAll {
            top: Some(TopHit {
                kind: TopKind::Song,
                id: "gone".into(),
                title: "Unknown".into(),
                artist: "Nobody".into(),
                thumbnail_url: None,
            }),
            ..all.clone()
        };
        assert!(matches!(top_hero(&unknown), Some(SearchHit::Song(t)) if t.id == "cover"));
        // Empty response has no hero and no rows.
        let empty = SearchAll::default();
        assert!(top_hero(&empty).is_none());
        assert!(order_all_hits(&empty).is_empty());
        assert!(all_empty(&empty));
        assert!(!all_empty(&all));
    }

    #[test]
    fn server_search_caps_total_and_narrows() {
        use yutu_core::{MixedRef, SearchAll};
        let tracks: Vec<Track> = (0..40)
            .map(|i| mk_track(&format!("v{i}"), "Run", "A"))
            .collect();
        let mixed: Vec<MixedRef> = tracks
            .iter()
            .map(|t| MixedRef {
                kind: 0,
                id: t.id.clone(),
            })
            .collect();
        let all = SearchAll {
            tracks,
            mixed,
            ..SearchAll::default()
        };
        let rows = order_all_hits(&all);
        assert_eq!(rows.len(), MIXED_SEARCH_CAP);
        // Keystroke narrowing drops dangling refs at render.
        let base = SearchAll {
            tracks: vec![
                mk_track("v1", "Yesterday", "The Beatles"),
                mk_track("v2", "Yellow Submarine", "The Beatles"),
            ],
            mixed: vec![
                MixedRef {
                    kind: 0,
                    id: "v1".into(),
                },
                MixedRef {
                    kind: 0,
                    id: "v2".into(),
                },
            ],
            ..SearchAll::default()
        };
        let narrowed = filter_all(&base, "yesterday");
        assert_eq!(narrowed.tracks.len(), 1);
        // The survivor heroes, so rows are empty but rendering is not.
        assert!(matches!(top_hero(&narrowed), Some(SearchHit::Song(t)) if t.id == "v1"));
        assert!(order_all_hits(&narrowed).is_empty());
        assert!(filter_all(&base, "xyz").tracks.is_empty());
        assert!(order_all_hits(&filter_all(&base, "xyz")).is_empty());
        assert!(filter_all(&base, "").tracks.is_empty());
    }

    #[test]
    fn search_bundle_paints_both_halves() {
        use yutu_core::SearchAll;
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        let all = SearchAll {
            tracks: vec![mk_track("v9", "Hey Jude", "The Beatles")],
            ..SearchAll::default()
        };
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(SearchBundle {
                all,
                filtered: SearchResults {
                    tracks: vec![mk_track("v1", "Yesterday", "The Beatles")],
                    albums: vec![],
                    artists: vec![],
                    playlists: vec![],
                },
            })),
        });
        // Tabs read the filtered half, All reads the server half.
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.all_results.tracks.len(), 1);
        assert_eq!(s.all_results.tracks[0].id, "v9");
        // Both halves register, so track menus resolve off either list.
        assert!(s.find_track("v9").is_some());
        // Keystrokes narrow both halves with no network.
        let _ = s.update(Message::SearchChanged("yesterday".into()));
        assert_eq!(s.results.tracks.len(), 1);
        assert!(s.all_results.tracks.is_empty());
    }

    #[test]
    #[cfg(feature = "innertube")]
    fn all_from_filtered_keeps_server_order() {
        let filtered = SearchResults {
            tracks: vec![mk_track("v1", "Song", "A")],
            albums: vec![],
            artists: vec![],
            playlists: vec![PlaylistHit {
                id: "PL1".into(),
                title: "Mix".into(),
                author: "Fan".into(),
                detail: "5 songs".into(),
                thumbnail_url: None,
            }],
        };
        let all = all_from_filtered(&filtered);
        assert!(all.top.is_none());
        assert_eq!(
            all.mixed
                .iter()
                .map(|m| (m.kind, m.id.as_str()))
                .collect::<Vec<_>>(),
            vec![(0, "v1"), (3, "PL1")]
        );
        // Hero falls back to the first row, rows exclude it, no dups.
        let hero = top_hero(&all).expect("fallback hero");
        assert!(matches!(hero, SearchHit::Song(t) if t.id == "v1"));
        let rows = order_all_hits(&all);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], SearchHit::Playlist(_)));
    }

    #[test]
    fn empty_top_id_never_dupes_the_hero() {
        use yutu_core::{MixedRef, SearchAll, TopHit, TopKind};
        // Card without a tap target: title match still heroes right.
        let all = SearchAll {
            top: Some(TopHit {
                kind: TopKind::Song,
                id: String::new(),
                title: "Hey Jude".into(),
                artist: "The Beatles".into(),
                thumbnail_url: None,
            }),
            tracks: vec![mk_track("v9", "Hey Jude", "The Beatles")],
            artists: vec![],
            albums: vec![],
            playlists: vec![],
            mixed: vec![MixedRef {
                kind: 0,
                id: "v9".into(),
            }],
        };
        let hero = top_hero(&all).expect("title-match hero");
        assert!(matches!(hero, SearchHit::Song(t) if t.id == "v9"));
        assert!(order_all_hits(&all).is_empty());
    }

    #[test]
    fn search_filter_defaults_all_and_switches() {
        let mut s = test_state();
        assert_eq!(s.search_filter, SearchFilter::All);
        let _ = s.update(Message::SetSearchFilter(SearchFilter::Albums));
        assert_eq!(s.search_filter, SearchFilter::Albums);
        // Filtered views render with no panic, even with empty lists.
        s.search_query = "run".into();
        let _ = s.view();
        let _ = s.update(Message::SetSearchFilter(SearchFilter::Songs));
        let _ = s.view();
    }

    #[test]
    fn scrollbar_fade_lights_and_decays() {
        let mut s = test_state();
        assert_eq!(s.scroll_opacity("queue"), 0.0);
        assert!(!s.scroll_fading());
        assert!(!s.animating());
        // Scroll motion lights the bar at full strength (no tick needed
        // while fresh: it sits at 1).
        let _ = s.update(Message::Scrolled("queue"));
        assert_eq!(s.scroll_opacity("queue"), 1.0);
        assert!(!s.scroll_fading());
        // Other lists stay invisible.
        assert_eq!(s.scroll_opacity("body"), 0.0);
        // Aged activity needs ticker frames and decays on tick.
        if let Some(h) = s.scroll_hint.get_mut("queue") {
            h.0 = std::time::Instant::now() - std::time::Duration::from_millis(5_000);
        }
        assert!(s.scroll_fading());
        assert!(s.animating());
        let _ = s.update(Message::Animate);
        let faded = s.scroll_opacity("queue");
        assert!(faded < 1.0 && faded > 0.0);
        // Drawer scrolling lights its own key through its own message.
        let _ = s.update(Message::DrawerScrolled(10.0));
        assert_eq!(s.scroll_opacity("drawer"), 1.0);
    }

    #[test]
    fn radio_ready_queues_fetched_sequence() {
        let mut s = test_state();
        // Unknown seed: fetch is kicked off so the song appears without a
        // manual search, nothing queued yet.
        let _ = s.update(Message::StartRadio("unknown-id".into()));
        assert!(s.status.contains("Fetching info"));
        assert!(s.queue.is_empty());
        // Known seed (catalog): radio view entered; payload queues on ready.
        let _ = s.update(Message::StartRadio("m1".into()));
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("v9", "Come Together", "The Beatles")]),
        });
        assert_eq!(s.route, Route::Radio("m1".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.radio_tracks.len(), 1);
        // Failures report cleanly.
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("Radio failed"));
    }

    #[test]
    fn queue_jump_and_repeat_controls() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(1));
        assert_eq!(s.queue.position(), Some(1));
        let _ = s.update(Message::QueueJump(99));
        assert_eq!(s.queue.position(), Some(1));
        let _ = s.update(Message::ToggleShuffle);
        assert!(s.queue.shuffle);
        let _ = s.update(Message::ToggleShuffle);
        assert!(!s.queue.shuffle);
        let _ = s.update(Message::CycleRepeat);
        assert_eq!(s.queue.repeat, yutu_player::RepeatMode::All);
    }

    #[test]
    fn like_toggles() {
        let mut s = test_state();
        let _ = s.update(Message::Like("m1".into()));
        assert!(s.liked.contains("m1"));
        let _ = s.update(Message::Like("m1".into()));
        assert!(!s.liked.contains("m1"));
    }

    #[test]
    fn dislike_toggles_and_excludes_likes() {
        let mut s = test_state();
        let _ = s.update(Message::Dislike("m1".into()));
        assert!(s.disliked.contains("m1"));
        assert_eq!(s.disliked_order, vec!["m1".to_string()]);
        assert!(s.status.contains("Won't recommend"));
        let _ = s.update(Message::Dislike("m1".into()));
        assert!(!s.disliked.contains("m1"));
        assert!(s.disliked_order.is_empty());
        // Disliking a liked track clears the like (one status server-side).
        let _ = s.update(Message::Like("m2".into()));
        assert!(s.liked.contains("m2"));
        let _ = s.update(Message::Dislike("m2".into()));
        assert!(s.disliked.contains("m2"));
        assert!(!s.liked.contains("m2"));
        // Liking a disliked track clears the dislike.
        let _ = s.update(Message::Like("m2".into()));
        assert!(s.liked.contains("m2"));
        assert!(!s.disliked.contains("m2"));
    }

    #[test]
    fn server_playlist_remove_reports_without_local_only() {
        let mut s = test_state();
        s.user_playlists.push(yutu_core::Playlist {
            id: "PLserver1".into(),
            title: "Road".into(),
            tracks: vec![mk_track("m1", "Song", "Someone")],
        });
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: "PLserver1".into(),
            track_id: "m1".into(),
        });
        assert!(s.user_playlists[0].tracks.is_empty());
        assert!(s.status.contains("Removed m1 from Road"));
        assert!(!s.status.contains("local only"));
    }

    #[test]
    fn stored_browse_ids_migrate_to_edit_form() {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("yutu-test-mig-{}-{}.db", std::process::id(), n));
        let path_s = path.to_string_lossy().into_owned();
        {
            let store = LibraryStore::open(&path_s).unwrap();
            store
                .sync_server_playlist("VLPLabc123", "Road", &[mk_track("m1", "Song", "Someone")])
                .unwrap();
        }
        let (s, _) = AppState::with_library_path(path_s.clone());
        assert!(
            s.user_playlists.iter().any(|p| p.id == "PLabc123"),
            "browse id must migrate, got {:?}",
            s.user_playlists.iter().map(|p| &p.id).collect::<Vec<_>>()
        );
        assert!(
            !s.user_playlists.iter().any(|p| p.id == "VLPLabc123"),
            "stale browse row must go"
        );
        let pl = s
            .user_playlists
            .iter()
            .find(|p| p.id == "PLabc123")
            .unwrap();
        assert!(pl.tracks.iter().any(|t| t.id == "m1"));
        // Reopening does not duplicate or revive the stale row.
        let (s2, _) = AppState::with_library_path(path_s);
        assert_eq!(
            s2.user_playlists
                .iter()
                .filter(|p| p.id == "PLabc123")
                .count(),
            1
        );
        assert!(!s2.user_playlists.iter().any(|p| p.id == "VLPLabc123"));
    }

    #[test]
    fn disliked_tracks_stay_out_of_autoplay_and_mixes() {
        let mut s = test_state();
        let _ = s.update(Message::Dislike("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m2".into(),
            result: Ok(vec![
                mk_track("m1", "Nope", "Someone"),
                mk_track("m3", "Yep", "Someone"),
            ]),
        });
        // Seed mismatch: set the seed first via PlayNow-style state.
        s.autoplay_for = Some("m2".into());
        let _ = s.update(Message::AutoplayReady {
            seed: "m2".into(),
            result: Ok(vec![
                mk_track("m1", "Nope", "Someone"),
                mk_track("m3", "Yep", "Someone"),
            ]),
        });
        assert!(!s.autoplay.iter().any(|t| t.id == "m1"));
        assert!(s.autoplay.iter().any(|t| t.id == "m3"));
        // Mixes also skip disliked ids (Weekly Mix needs history; at least
        // the filter composes without panicking on the fixture catalog).
        for t in s.weekly_mix_tracks() {
            assert!(!s.disliked.contains(&t.id));
        }
        for t in s.forgotten_tracks() {
            assert!(!s.disliked.contains(&t.id));
        }
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn dislike_sync_results_report_honestly() {
        let mut s = test_state();
        let _ = s.update(Message::DislikeSynced {
            track_id: "m1".into(),
            disliked: true,
            result: Ok(()),
        });
        // Success stays silent (row already shows the state).
        let _ = s.update(Message::DislikeSynced {
            track_id: "m1".into(),
            disliked: true,
            result: Err("offline".into()),
        });
        assert!(s.status.contains("don't-recommend"));
        assert!(s.status.contains("offline"));
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_due_math() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        // Disabled never fires.
        assert!(!sync_due(
            false,
            None,
            start,
            start + Duration::from_secs(10_000)
        ));
        // No completed pull yet: grace period after launch.
        assert!(!sync_due(
            true,
            None,
            start,
            start + Duration::from_secs(10)
        ));
        assert!(sync_due(
            true,
            None,
            start,
            start + Duration::from_secs(AUTO_SYNC_STARTUP_GRACE_SECS + 1)
        ));
        // Afterwards the interval rules.
        let last = start + Duration::from_secs(1_000);
        assert!(!sync_due(
            true,
            Some(last),
            start,
            last + Duration::from_secs(60)
        ));
        assert!(sync_due(
            true,
            Some(last),
            start,
            last + Duration::from_secs(AUTO_SYNC_INTERVAL_SECS)
        ));
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_ago_labels() {
        assert_eq!(fmt_sync_ago(None, 1_000), "never");
        assert_eq!(fmt_sync_ago(Some(999), 1_000), "just now");
        assert_eq!(fmt_sync_ago(Some(700), 1_000), "5m ago");
        assert_eq!(fmt_sync_ago(Some(1_000 - 7_200), 1_000), "2h ago");
        assert_eq!(fmt_sync_ago(Some(1_000 - 3 * 86_400), 1_000), "3d ago");
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn like_action_records_pending_and_success_clears() {
        let mut s = test_state();
        let _ = s.update(Message::Like("m1".into()));
        let store = LibraryStore::open(&s.library_path).unwrap();
        assert_eq!(store.pending_ops().unwrap().len(), 1);
        let _ = s.update(Message::LikeSynced {
            track_id: "m1".into(),
            liked: true,
            result: Ok(()),
        });
        let store = LibraryStore::open(&s.library_path).unwrap();
        assert!(store.pending_ops().unwrap().is_empty());
        // A stale success for the older op cannot clear the newer row.
        let _ = s.update(Message::Like("m2".into()));
        let _ = s.update(Message::Like("m2".into()));
        let _ = s.update(Message::LikeSynced {
            track_id: "m2".into(),
            liked: true,
            result: Ok(()),
        });
        let store = LibraryStore::open(&s.library_path).unwrap();
        let ops = store.pending_ops().unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].op, "unlike");
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_reconciles_likes_server_wins_with_pending_override() {
        let mut s = test_state();
        s.like_id("stale-local");
        // Server knows only w1; the user unliked w1 while offline.
        let store = LibraryStore::open(&s.library_path).unwrap();
        store.like("w1").unwrap();
        store
            .record_pending("unlike", None, Some("w1"), None)
            .unwrap();
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Sync,
            result: Ok(StagedImport {
                backend: test_backend(),
                tracks: vec![],
                liked_ids: vec!["w1".into()],
                disliked_ids: vec![],
                headers: vec![],
                headers_clean: true,
                library_ok: true,
            }),
        });
        // Server-wins dropped the stale id; the pending unlike dropped w1.
        assert!(!s.liked.contains("stale-local"));
        assert!(!s.liked.contains("w1"));
        assert!(s.liked_order.is_empty());
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_prunes_deleted_lists_only_on_clean_headers() {
        let mut s = test_state();
        s.user_playlists.push(yutu_core::Playlist {
            id: "PLgone".into(),
            title: "Gone".into(),
            tracks: vec![],
        });
        s.user_playlists.push(yutu_core::Playlist {
            id: "PLkept".into(),
            title: "Kept".into(),
            tracks: vec![],
        });
        // Clean headers without PLgone: pruned.
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Sync,
            result: Ok(StagedImport {
                backend: test_backend(),
                tracks: vec![],
                liked_ids: vec![],
                disliked_ids: vec![],
                headers: vec![yutu_core::ServerPlaylist {
                    id: "PLkept".into(),
                    title: "Kept".into(),
                }],
                headers_clean: true,
                library_ok: false,
            }),
        });
        assert!(s.user_playlists.iter().all(|p| p.id != "PLgone"));
        assert!(s.user_playlists.iter().any(|p| p.id == "PLkept"));
        // Dirty headers: nothing pruned even when absent.
        s.user_playlists.push(yutu_core::Playlist {
            id: "PLgone2".into(),
            title: "Gone2".into(),
            tracks: vec![],
        });
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Sync,
            result: Ok(StagedImport {
                backend: test_backend(),
                tracks: vec![],
                liked_ids: vec![],
                disliked_ids: vec![],
                headers: vec![],
                headers_clean: false,
                library_ok: false,
            }),
        });
        assert!(s.user_playlists.iter().any(|p| p.id == "PLgone2"));
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_skips_lists_with_unpushed_edits() {
        let mut s = test_state();
        s.user_playlists.push(yutu_core::Playlist {
            id: "PLedit".into(),
            title: "Edit".into(),
            tracks: vec![mk_track("m1", "Song", "Someone")],
        });
        let store = LibraryStore::open(&s.library_path).unwrap();
        store
            .record_pending("add", Some("PLedit"), Some("m2"), None)
            .unwrap();
        // The pull carries a depleted server copy: the pending list keeps
        // its local rows instead of being replaced.
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Sync,
            gen: s.import_gen,
            title: "Edit".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PLedit".into(),
                title: "Edit".into(),
                tracks: vec![],
            }),
        });
        let pl = s.user_playlists.iter().find(|p| p.id == "PLedit").unwrap();
        assert!(pl.tracks.iter().any(|t| t.id == "m1"));
        // Headers with pending deletes never fan out fetches.
        let store = LibraryStore::open(&s.library_path).unwrap();
        store
            .record_pending("delete", Some("PLdel"), None, None)
            .unwrap();
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Sync,
            result: Ok(staged(vec![], vec![], vec![("PLa", "A"), ("PLdel", "Del")])),
        });
        assert_eq!(s.import_total, 1);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn sync_now_needs_sign_in_and_toggle_persists() {
        let mut s = test_state();
        let _ = s.update(Message::SyncNow);
        assert!(s.status.contains("sign-in") || s.status.contains("Sync"));
        let _ = s.update(Message::AutoSyncToggled(false));
        assert!(!s.auto_sync);
        let store = LibraryStore::open(&s.library_path).unwrap();
        assert_eq!(store.get_pref("auto_sync").unwrap().as_deref(), Some("0"));
        let _ = s.update(Message::AutoSyncToggled(true));
        assert!(s.auto_sync);
        // Busy pipelines ignore the button silently.
        s.import_active = true;
        let before = s.status.clone();
        let _ = s.update(Message::SyncNow);
        assert_eq!(s.status, before);
    }

    #[test]
    fn playlist_crud() {
        let mut s = test_state();
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        assert_eq!(s.user_playlists.len(), 1);
        let pid = s.user_playlists[0].id.clone();
        assert_eq!(s.user_playlists[0].title, "Gym");
        // Empty names are rejected.
        let _ = s.update(Message::CreatePlaylist("   ".into()));
        assert_eq!(s.user_playlists.len(), 1);
        // Add + dedupe + unknown track.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "nope".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        // Unknown playlists are rejected.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "nope".into(),
            track_id: "m1".into(),
        });
        assert_eq!(s.user_playlists[0].tracks.len(), 1);
        // Remove + delete.
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert!(s.user_playlists[0].tracks.is_empty());
        let _ = s.update(Message::OpenPlaylist(pid.clone()));
        let _ = s.update(Message::DeletePlaylist(pid.clone()));
        assert!(s.user_playlists.is_empty());
        assert_eq!(s.route, Route::Library);
        // Deleting an unknown playlist is a no-op with a message.
        let _ = s.update(Message::DeletePlaylist("nope".into()));
        assert!(s.user_playlists.is_empty());
    }

    #[test]
    fn playlist_submenu_toggles_and_closes_with_menu() {
        let mut s = test_state();
        let _ = s.update(Message::OpenContext("m1".into()));
        assert!(s.submenu_playlists_for.is_none());
        let _ = s.update(Message::TogglePlaylistSubmenu("m1".into()));
        assert_eq!(s.submenu_playlists_for.as_deref(), Some("m1"));
        let _ = s.update(Message::TogglePlaylistSubmenu("m1".into()));
        assert!(s.submenu_playlists_for.is_none());
        // Dismissing the menu drops the flyout too.
        let _ = s.update(Message::TogglePlaylistSubmenu("m1".into()));
        let _ = s.update(Message::CloseContext);
        assert!(s.context_menu.is_none());
        assert!(s.submenu_playlists_for.is_none());
    }

    #[test]
    fn playlist_has_tracks_membership() {
        let mut s = test_state();
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        assert!(!s.playlist_has(&pid, "m1"));
        assert!(!s.playlist_has("nope", "m1"));
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert!(s.playlist_has(&pid, "m1"));
        assert!(!s.playlist_has(&pid, "m2"));
        // Menu stays open for multi-toggle.
        let _ = s.update(Message::OpenContext("m1".into()));
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        assert!(!s.playlist_has(&pid, "m1"));
        assert!(s.context_menu.is_some());
    }

    #[test]
    fn downloaded_is_a_default_playlist() {
        let mut s = test_state();
        // Both built-ins lead the sidebar/library lists.
        let all = s.all_playlists();
        assert_eq!(all[0].id, "__favorites");
        assert_eq!(all[1].id, "__downloaded");
        assert_eq!(all[1].title, "Downloaded");
        assert!(all[1].tracks.is_empty());
        assert!(s.find_playlist("__downloaded").is_some());
        assert!(!s.playlist_has("__downloaded", "m1"));
        // A completed download joins the list in add order.
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.downloaded.contains("m1"));
        assert_eq!(s.downloaded_order, vec!["m1".to_string()]);
        assert!(s.playlist_has("__downloaded", "m1"));
        let _ = s.update(Message::Downloaded {
            track_id: "m2".into(),
            result: Ok(()),
        });
        assert_eq!(s.find_playlist("__downloaded").unwrap().tracks.len(), 2);
        // Removing deletes the membership (file removal is best-effort).
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: "__downloaded".into(),
            track_id: "m1".into(),
        });
        assert!(!s.downloaded.contains("m1"));
        assert!(!s.playlist_has("__downloaded", "m1"));
        assert!(s.status.contains("Downloaded"));
        // Built-in cannot be deleted.
        let _ = s.update(Message::DeletePlaylist("__downloaded".into()));
        assert!(s.find_playlist("__downloaded").is_some());
        assert!(s.status.contains("can't be deleted"));
    }

    #[test]
    fn add_to_downloaded_starts_a_download() {
        let mut s = test_state();
        // Already-downloaded is a no-op with a message.
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "__downloaded".into(),
            track_id: "m1".into(),
        });
        assert!(s.status.contains("Already in Downloaded"));
        // Unknown track still goes through the fetch path.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "__downloaded".into(),
            track_id: "ghost".into(),
        });
        assert!(s.status.contains("Fetching info"));
    }

    /// Unique temp dir per test for the offline bundle files.
    fn bundle_dir(name: &str) -> String {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "yutu-test-bundle-{}-{}-{}",
            std::process::id(),
            name,
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    /// Minimal 0.1s mono 16-bit 44.1kHz WAV (decodable without network).
    fn wav_bytes() -> Vec<u8> {
        let rate: u32 = 44_100;
        let n: usize = (rate as usize) / 10;
        let mut data = Vec::with_capacity(44 + n * 2);
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&((36 + n * 2) as u32).to_le_bytes());
        data.extend_from_slice(b"WAVEfmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&rate.to_le_bytes());
        data.extend_from_slice(&(rate * 2).to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(b"data");
        data.extend_from_slice(&((n * 2) as u32).to_le_bytes());
        for i in 0..n {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            data.extend_from_slice(&((s * 0.2 * i16::MAX as f32) as i16).to_le_bytes());
        }
        data
    }

    #[test]
    fn offline_bundle_files_round_trip() {
        let dir = bundle_dir("roundtrip");
        // Audio bytes through the same tracker the app uses.
        let mut cache = yutu_player::OfflineCache::new(10 * 1024 * 1024);
        cache.set_dir(dir.clone());
        cache.store_bytes("m1", &wav_bytes()).unwrap();
        assert_eq!(offline_bytes_in(&dir, "m1").unwrap(), wav_bytes());
        assert!(offline_bytes_in(&dir, "ghost").is_none());
        // Lyrics JSON round-trips (serde derives on the core type).
        let lyrics = yutu_core::LyricsResult {
            plain: "hello".into(),
            synced: vec![yutu_core::LyricLine {
                start_ms: 100,
                text: "hello".into(),
            }],
            instrumental: false,
        };
        save_lyrics_in(&dir, "m1", &lyrics);
        #[cfg(feature = "lyrics")]
        {
            assert!(std::path::Path::new(&lyrics_file_in(&dir, "m1")).exists());
            assert!(load_lyrics_in(&dir, "m1").is_some());
            assert!(load_lyrics_in(&dir, "ghost").is_none());
        }
        // Artwork keyed by URL hash with a sniffed image extension
        // (the decoder keys off the extension, so `.bin` never renders).
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        save_artwork_in(&dir, "https://example.invalid/a", &jpeg);
        save_artwork_in(&dir, "https://example.invalid/b", &[0x89, b'P', b'N', b'G']);
        let pa = artwork_path_in(&dir, "https://example.invalid/a").unwrap();
        let pb = artwork_path_in(&dir, "https://example.invalid/b").unwrap();
        assert!(pa.ends_with(".jpg"), "unexpected: {pa}");
        assert!(pb.ends_with(".png"), "unexpected: {pb}");
        assert_ne!(pa, pb);
        assert_eq!(
            artwork_path_in(&dir, "https://example.invalid/a").unwrap(),
            pa
        );
        assert!(artwork_path_in(&dir, "https://example.invalid/ghost").is_none());
        // Legacy extensionless files migrate on first lookup.
        let legacy_stem = artwork_stem_in(&dir, "https://example.invalid/old");
        std::fs::write(format!("{legacy_stem}.bin"), jpeg).unwrap();
        let migrated = artwork_path_in(&dir, "https://example.invalid/old").unwrap();
        assert!(migrated.ends_with(".jpg"), "unexpected: {migrated}");
        assert!(!std::path::Path::new(&format!("{legacy_stem}.bin")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn artwork_ext_sniffs_magic_bytes() {
        assert_eq!(artwork_ext(&[0xFF, 0xD8, 0xFF, 0x00]), "jpg");
        assert_eq!(artwork_ext(&[0x89, b'P', b'N', b'G', 0x0D]), "png");
        let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        assert_eq!(artwork_ext(&webp), "webp");
        webp[11] = b'X';
        assert_eq!(artwork_ext(&webp), "jpg");
        assert_eq!(artwork_ext(b"GIF89a"), "gif");
        assert_eq!(artwork_ext(b""), "jpg");
        assert_eq!(artwork_ext(b"whatever"), "jpg");
    }

    #[test]
    fn sanitize_filename_maps_like_the_audio_cache() {
        assert_eq!(sanitize_filename("abcXYZ012"), "abcXYZ012");
        assert_eq!(sanitize_filename("a/b-c_d e"), "a_b_c_d_e");
        assert_eq!(sanitize_filename(""), "unknown");
        assert_eq!(lyrics_file_in("/tmp/x", "a/b"), "/tmp/x/a_b.json");
    }

    #[test]
    fn offline_playback_uses_disk_bytes_without_network() {
        let mut s = test_state();
        let dir = bundle_dir("play");
        let t = mk_track("m1", "Come Together", "The Beatles");
        s.catalog.insert(t.id.clone(), t.clone());
        // Nothing on disk: honest miss, no status change, no network.
        assert!(!s.try_play_offline_in(&dir, &t));
        // Store decodable bytes: a live backend plays with no resolve
        // task involved; headless (or a broken backend) reports an
        // honest failure instead of hanging.
        let mut cache = yutu_player::OfflineCache::new(10 * 1024 * 1024);
        cache.set_dir(dir.clone());
        cache.store_bytes("m1", &wav_bytes()).unwrap();
        let played = s.try_play_offline_in(&dir, &t);
        if played {
            assert!(s.status.contains("(offline)"));
            assert!(s.loading_track.is_none());
        } else {
            assert!(s.audio.is_none() || s.status.contains("Playback failed"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn downloaded_tracks_prefer_offline_in_play_current() {
        let mut s = test_state();
        // Mark downloaded but store no bytes: the disk read misses, so
        // playback falls through to the network resolve instead of
        // claiming offline playback.
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.downloaded.contains("m1"));
        let missing_dir = bundle_dir("missing");
        assert!(!s.try_play_offline_in(&missing_dir, &mk_track("m1", "T", "A")));
    }

    #[test]
    fn remove_download_deletes_the_whole_bundle() {
        let base = bundle_dir("unbundle");
        let cache_dir = format!("{base}/audio");
        let lyrics_dir = format!("{base}/lyrics");
        let art_dir = format!("{base}/art");
        let track = Track {
            id: "m1".into(),
            title: "Come Together".into(),
            artist: "The Beatles".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: Some("https://example.invalid/s".into()),
            thumbnail_large_url: Some("https://example.invalid/l".into()),
            plays: None,
        };
        // Fake a complete bundle on disk.
        let mut cache = yutu_player::OfflineCache::new(10 * 1024 * 1024);
        cache.set_dir(cache_dir.clone());
        cache.store_bytes("m1", b"audio").unwrap();
        save_lyrics_in(
            &lyrics_dir,
            "m1",
            &yutu_core::LyricsResult {
                plain: "hello".into(),
                synced: Vec::new(),
                instrumental: false,
            },
        );
        save_artwork_in(&art_dir, "https://example.invalid/s", &[0xFF, 0xD8, 0xFF]);
        assert!(offline_bytes_in(&cache_dir, "m1").is_some());
        assert!(artwork_path_in(&art_dir, "https://example.invalid/s").is_some());
        // Removal drops audio + lyrics + art, and is idempotent.
        AppState::remove_download_bundle_in(&cache_dir, &lyrics_dir, &art_dir, "m1", Some(&track));
        assert!(offline_bytes_in(&cache_dir, "m1").is_none());
        assert!(!std::path::Path::new(&lyrics_file_in(&lyrics_dir, "m1")).exists());
        assert!(artwork_path_in(&art_dir, "https://example.invalid/s").is_none());
        AppState::remove_download_bundle_in(&cache_dir, &lyrics_dir, &art_dir, "m1", Some(&track));
        // Unknown tracks still clear audio + lyrics (art needs URLs).
        AppState::remove_download_bundle_in(&cache_dir, &lyrics_dir, &art_dir, "ghost", None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn remove_from_downloaded_unmarks_and_reports() {
        let mut s = test_state();
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: "__downloaded".into(),
            track_id: "m1".into(),
        });
        assert!(s.status.contains("not in Downloaded"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.downloaded.contains("m1"));
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: "__downloaded".into(),
            track_id: "m1".into(),
        });
        assert!(!s.downloaded.contains("m1"));
        assert!(s.status.contains("Removed"));
    }

    #[test]
    fn radio_generation_is_deterministic() {
        let seq = radio_for("m3");
        assert_eq!(seq.len(), 5);
        assert_eq!(seq[0].id, "m3");
        let ids: Vec<&str> = seq.iter().map(|t| t.id.as_str()).collect();
        for want in ["m1", "m2", "m3", "m4", "m5"] {
            assert!(ids.contains(&want), "missing {want}");
        }
        // Unknown seed falls back to fixture order.
        let seq = radio_for("zzz");
        assert_eq!(seq[0].id, "m1");
        assert_eq!(seq.len(), 5);
    }

    #[test]
    fn route_navigation() {
        let mut s = test_state();
        let _ = s.update(Message::OpenArtist("The Beatles".into()));
        assert_eq!(s.route, Route::Artist("The Beatles".into()));
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::OpenPlaylist(pid.clone()));
        assert_eq!(s.route, Route::PlaylistDetail(pid));
        let _ = s.update(Message::OpenLyrics("m4".into()));
        assert_eq!(s.route, Route::Lyrics("m4".into()));
        let _ = s.update(Message::StartRadio("m1".into()));
        #[cfg(feature = "innertube")]
        assert_eq!(s.route, Route::Radio("m1".into()));
        #[cfg(not(feature = "innertube"))]
        assert_eq!(s.route, Route::Lyrics("m4".into()));
        assert!(s.queue.is_empty());
        let _ = s.update(Message::RadioReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("v1", "Yesterday", "The Beatles")]),
        });
        assert_eq!(s.queue.len(), 1);
    }

    #[test]
    fn play_all_shuffled_turns_on_shuffle_first() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Hey Jude", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert!(!s.queue.shuffle);
        let _ = s.update(Message::PlayAllShuffled);
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 2);
    }

    #[test]
    fn shuffle_play_starts_random_and_always_reshuffles() {
        let mut s = test_state();
        let ids: Vec<String> = (0..6).map(|i| format!("v{i}")).collect();
        s.results.tracks = ids
            .iter()
            .map(|id| mk_track(id, "Song", "Artist"))
            .collect();
        s.route = Route::Search;
        // Random first track: 20 plays never start on the same song.
        let mut firsts = std::collections::HashSet::new();
        for _ in 0..20 {
            let _ = s.update(Message::PlayAllShuffled);
            firsts.insert(s.queue.current().unwrap().id.clone());
        }
        assert!(firsts.len() > 1, "shuffle play should start anywhere");
        // Snapshot is the playlist order (toggle-off restores exactly it).
        let ordered: Vec<String> = ids.clone();
        assert_eq!(s.pre_shuffle, Some(ordered.clone()));
        assert!(s.queue.shuffle);
        let _ = s.update(Message::ToggleShuffle);
        assert!(!s.queue.shuffle);
        assert_eq!(
            s.queue
                .items()
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            ordered
        );
        // Shuffle-play while already on reshuffles (no early-return).
        let _ = s.update(Message::PlayAllShuffled);
        assert!(s.queue.shuffle);
        assert_eq!(s.pre_shuffle, Some(ordered));
        // The fetch targets the shuffled head, never the pre-shuffle
        // first song (a stale fetch would drop and play nothing).
        let head = s.queue.current().unwrap().id.clone();
        assert_eq!(s.loading_track.as_deref(), Some(head.as_str()));
    }

    #[test]
    fn playlist_click_plays_from_clicked_row() {
        let mut s = test_state();
        let tracks: Vec<Track> = (0..4)
            .map(|i| mk_track(&format!("t{i}"), "Song", "Artist"))
            .collect();
        s.user_playlists.push(Playlist {
            id: "u1".into(),
            title: "Gym".into(),
            tracks: tracks.clone(),
        });
        let _ = s.update(Message::PlayPlaylistAt {
            playlist_id: "u1".into(),
            index: 2,
        });
        // Clicked song plus the rest from that point replace the queue.
        assert_eq!(
            s.queue
                .items()
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            vec!["t2".to_string(), "t3".to_string()]
        );
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("t2"));
        assert!(s.playing_from.contains("Gym"));
        // Detail page renders the new rows with no panic.
        s.route = Route::PlaylistDetail("u1".into());
        let _ = s.view();
        // Past-the-end index reports honestly with no queue change.
        let _ = s.update(Message::PlayPlaylistAt {
            playlist_id: "u1".into(),
            index: 9,
        });
        assert_eq!(s.queue.len(), 2);
        assert!(s.status.contains("nothing to play"));
        // Unknown playlist reports honestly too.
        let _ = s.update(Message::PlayPlaylistAt {
            playlist_id: "nope".into(),
            index: 0,
        });
        assert!(s.status.contains("not found"));
    }

    #[test]
    fn bulk_queue_caches_everything_in_one_shot() {
        let mut s = test_state();
        let tracks: Vec<Track> = (0..250)
            .map(|i| mk_track(&format!("v{i}"), "Song", "Artist"))
            .collect();
        let start = std::time::Instant::now();
        let _ = s.queue_many(tracks, "Probe");
        assert!(
            start.elapsed().as_secs() < 10,
            "bulk queue must never stall the GUI thread"
        );
        assert_eq!(s.queue.len(), 250);
        let store = LibraryStore::open(&s.library_path).unwrap();
        let cached = store.all_cached_tracks().unwrap();
        assert!(cached.iter().any(|t| t.id == "v0"));
        assert!(cached.iter().any(|t| t.id == "v249"));
    }

    #[test]
    fn hero_shuffle_plays_known_tracks_shuffled() {
        use yutu_core::{Album, Artist};
        let mut s = test_state();
        assert!(!s.queue.shuffle);
        // Artist hero shuffle: all known catalog tracks, shuffle on.
        let _ = s.update(Message::PlayArtistShuffled("The Beatles".into()));
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 5);
        // Album hero shuffle by title: the Abbey Road tracks.
        let _ = s.update(Message::PlayAlbumShuffled("Abbey Road".into()));
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 3);
        // Unknown names report honestly with no queue change.
        let _ = s.update(Message::PlayArtistShuffled("Nobody".into()));
        assert_eq!(s.queue.len(), 3);
        assert!(s.status.contains("nothing to play"));
        // Heroes render their actions with no panic.
        s.search_query = "beatles".into();
        s.results = SearchResults {
            tracks: vec![mk_track("v1", "Yesterday", "The Beatles")],
            albums: vec![Album {
                id: "a1".into(),
                title: "Abbey Road".into(),
                artist: "The Beatles".into(),
                year: Some(1969),
                thumbnail_url: None,
            }],
            artists: vec![Artist {
                id: "r1".into(),
                name: "The Beatles".into(),
                thumbnail_url: None,
            }],
            playlists: vec![PlaylistHit {
                id: "PL1".into(),
                title: "Beatles Mix".into(),
                author: "Fan".into(),
                detail: "25 songs".into(),
                thumbnail_url: None,
            }],
        };
        let _ = s.view();
        let album = s.results.albums[0].clone();
        let _ = s.search_album_hero(&album);
        let artist = s.results.artists[0].clone();
        let _ = s.search_artist_hero(&artist);
        let playlist = s.results.playlists[0].clone();
        let _ = s.search_playlist_hero(&playlist);
        let _ = s.search_playlist_row(&playlist);
        let _ = s.update(Message::SetSearchFilter(SearchFilter::Playlists));
        let _ = s.view();
    }

    #[test]
    fn search_playlist_ready_queues_or_reports() {
        let mut s = test_state();
        // Success queues the fetched tracks from a fresh context.
        let _ = s.update(Message::SearchPlaylistReady {
            id: "PL1".into(),
            label: "Beatles Mix".into(),
            shuffle: false,
            result: Ok(vec![
                mk_track("v1", "Yesterday", "The Beatles"),
                mk_track("v2", "Hey Jude", "The Beatles"),
            ]),
        });
        assert_eq!(s.queue.len(), 2);
        assert!(!s.queue.shuffle);
        assert!(s.playing_from.contains("Beatles Mix"));
        // Shuffle variant turns shuffle on.
        let _ = s.update(Message::SearchPlaylistReady {
            id: "PL1".into(),
            label: "Beatles Mix".into(),
            shuffle: true,
            result: Ok(vec![mk_track("v1", "Yesterday", "The Beatles")]),
        });
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 1);
        // Failures report honestly with no queue change.
        let _ = s.update(Message::SearchPlaylistReady {
            id: "PL1".into(),
            label: "Beatles Mix".into(),
            shuffle: false,
            result: Err("offline".into()),
        });
        assert_eq!(s.queue.len(), 1);
        assert!(s.status.contains("offline"));
        // Play actions label known playlists and fire without panic.
        s.results.playlists.push(PlaylistHit {
            id: "PL9".into(),
            title: "Beatles Mix".into(),
            author: "Fan".into(),
            detail: "25 songs".into(),
            thumbnail_url: None,
        });
        assert_eq!(s.search_playlist_label("PL9"), "Beatles Mix");
        assert_eq!(s.search_playlist_label("unknown"), "unknown");
        let _ = s.update(Message::PlaySearchPlaylist("PL9".into()));
        let _ = s.update(Message::PlaySearchPlaylistShuffled("PL9".into()));
    }

    #[test]
    fn search_rows_hover_and_collection_menus() {
        use yutu_core::{Album, Artist};
        let mut s = test_state();
        s.results.albums.push(Album {
            id: "al1".into(),
            title: "Toxicity".into(),
            artist: "System of a Down".into(),
            year: Some(2001),
            thumbnail_url: None,
        });
        s.results.artists.push(Artist {
            id: "ar1".into(),
            name: "System of a Down".into(),
            thumbnail_url: None,
        });
        s.results.playlists.push(PlaylistHit {
            id: "PL1".into(),
            title: "Mix".into(),
            author: "Fan".into(),
            detail: "25 songs".into(),
            thumbnail_url: None,
        });
        // Every row shape builds plain and hovered with no panic.
        let album = s.results.albums[0].clone();
        let artist = s.results.artists[0].clone();
        let pl = s.results.playlists[0].clone();
        let t = mk_track("v1", "Chop Suey!", "System of a Down");
        let _ = s.search_album_row(&album);
        let _ = s.search_artist_row(&artist);
        let _ = s.search_playlist_row(&pl);
        let _ = s.search_song_row(&t);
        let _ = s.numbered_track_row(&t, 1);
        s.hovered_album = Some("al1".into());
        s.hovered_artist = Some("ar1".into());
        s.hovered_search_playlist = Some("PL1".into());
        s.hovered_track = Some("v1".into());
        let _ = s.search_album_row(&album);
        let _ = s.search_artist_row(&artist);
        let _ = s.search_playlist_row(&pl);
        let _ = s.search_song_row(&t);
        let _ = s.numbered_track_row(&t, 1);
        // Liked heart stays on the row at rest.
        s.liked.insert("v1".into());
        s.hovered_track = None;
        let _ = s.search_song_row(&t);
        // Hover messages latch, clear, and freeze behind menus.
        let _ = s.update(Message::AlbumHover(Some("al1".into())));
        assert_eq!(s.hovered_album.as_deref(), Some("al1"));
        let _ = s.update(Message::ArtistHover(Some("ar1".into())));
        assert_eq!(s.hovered_artist.as_deref(), Some("ar1"));
        let _ = s.update(Message::SearchPlaylistHover(Some("PL1".into())));
        assert_eq!(s.hovered_search_playlist.as_deref(), Some("PL1"));
        let _ = s.update(Message::ClearHovers);
        assert!(
            s.hovered_album.is_none()
                && s.hovered_artist.is_none()
                && s.hovered_search_playlist.is_none()
        );
        // Menus open, freeze hovers, render, and close.
        let _ = s.update(Message::OpenAlbumMenu("al1".into()));
        assert_eq!(s.coll_menu, Some(CollectionMenu::Album("al1".into())));
        let _ = s.update(Message::AlbumHover(Some("al1".into())));
        assert!(s.hovered_album.is_none());
        let _ = s.update(Message::ArtistHover(Some("ar1".into())));
        assert!(s.hovered_artist.is_none());
        let _ = s.update(Message::SearchPlaylistHover(Some("PL1".into())));
        assert!(s.hovered_search_playlist.is_none());
        let _ = s.view();
        let _ = s.update(Message::OpenArtistMenu("ar1".into()));
        assert_eq!(s.coll_menu, Some(CollectionMenu::Artist("ar1".into())));
        let _ = s.view();
        let _ = s.update(Message::OpenSearchPlaylistMenu("PL1".into()));
        assert_eq!(s.coll_menu, Some(CollectionMenu::Playlist("PL1".into())));
        let _ = s.view();
        // Menu actions dismiss the menu.
        let _ = s.update(Message::OpenAlbum("al1".into()));
        assert!(s.coll_menu.is_none());
        let _ = s.update(Message::OpenSearchPlaylistMenu("PL1".into()));
        let _ = s.update(Message::PlaySearchPlaylistShuffled("PL1".into()));
        assert!(s.coll_menu.is_none());
        let _ = s.update(Message::OpenAlbumMenu("al1".into()));
        let _ = s.update(Message::CloseContext);
        assert!(s.coll_menu.is_none());
        // Unknown ids never open a scrim-less modal.
        let _ = s.update(Message::OpenArtistMenu("ghost".into()));
        assert!(s.coll_menu.is_none());
        assert!(s.status.contains("gone"));
        let _ = s.view();
        // One menu kind at a time, either direction.
        let _ = s.update(Message::OpenAlbumMenu("al1".into()));
        let _ = s.update(Message::OpenContext("v1".into()));
        assert!(s.coll_menu.is_none());
        assert_eq!(s.context_menu.as_deref(), Some("v1"));
        let _ = s.update(Message::OpenAlbumMenu("al1".into()));
        assert!(s.context_menu.is_none());
        assert_eq!(s.coll_menu, Some(CollectionMenu::Album("al1".into())));
        let _ = s.update(Message::CloseContext);
        // Filter cards build and right-click through to the same menus.
        s.route = Route::Search;
        s.search_query = "system of a down".into();
        s.search_filter = SearchFilter::Albums;
        let _ = s.view();
        let _ = s.update(Message::OpenAlbumMenu("al1".into()));
        assert_eq!(s.coll_menu, Some(CollectionMenu::Album("al1".into())));
        let _ = s.update(Message::CloseContext);
        s.search_filter = SearchFilter::Artists;
        let _ = s.view();
        let _ = s.update(Message::OpenArtistMenu("ar1".into()));
        assert_eq!(s.coll_menu, Some(CollectionMenu::Artist("ar1".into())));
        let _ = s.update(Message::CloseContext);
        // Fresh results without the hit prune the open menu.
        let _ = s.update(Message::OpenArtistMenu("ar1".into()));
        s.results.artists.clear();
        let _ = s.update(Message::SearchCompleted {
            query: "system of a down".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert!(s.coll_menu.is_none());
    }

    #[test]
    fn album_cover_prefers_api_art_then_track() {
        use yutu_core::Album;
        let mut s = test_state();
        // API art wins when present.
        let api = Album {
            id: "a1".into(),
            title: "Abbey Road".into(),
            artist: "The Beatles".into(),
            year: Some(1969),
            thumbnail_url: Some("https://example.invalid/cover.jpg".into()),
        };
        assert_eq!(
            s.album_cover(&api).as_deref(),
            Some("https://example.invalid/cover.jpg")
        );
        // Otherwise the first known track's art fills in.
        let mut t = mk_track("v9", "Come Together", "The Beatles");
        t.album = Some("Past Masters".into());
        t.thumbnail_url = Some("https://example.invalid/track.jpg".into());
        s.catalog.insert(t.id.clone(), t);
        let derived = Album {
            id: "ax".into(),
            title: "Past Masters".into(),
            artist: "The Beatles".into(),
            year: None,
            thumbnail_url: None,
        };
        assert_eq!(
            s.album_cover(&derived).as_deref(),
            Some("https://example.invalid/track.jpg")
        );
        // YTM-style URLs rewrite to the 544px hero key.
        let ytm = Album {
            id: "a2".into(),
            title: "A".into(),
            artist: "B".into(),
            year: None,
            thumbnail_url: Some("https://x=w60-h60-l90-rj".into()),
        };
        let url = s.album_cover(&ytm).expect("hires");
        assert!(url.contains("=w544-h544"), "{url}");
        // Artist photos rewrite the same way.
        s.results.artists.push(yutu_core::Artist {
            id: "r1".into(),
            name: "Queen".into(),
            thumbnail_url: Some("https://y=w60-h60-l90-rj".into()),
        });
        let photo = s.artist_photo("Queen").expect("photo");
        assert!(photo.contains("=w544-h544"), "{photo}");
        // Unknown album: no cover, honest None.
        let ghost = Album {
            id: "ax".into(),
            title: "Nope".into(),
            artist: "Nobody".into(),
            year: None,
            thumbnail_url: None,
        };
        assert!(s.album_cover(&ghost).is_none());
        assert!(s.artist_photo("Nobody").is_none());
    }

    #[test]
    fn play_all_queues_current_detail_view() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Hey Jude", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        let _ = s.update(Message::PlayAll);
        assert_eq!(s.queue.len(), 2);
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "v1".into(),
        });
        let _ = s.update(Message::OpenPlaylist(pid));
        let _ = s.update(Message::PlayAll);
        // Play-all is a fresh context: clears, then plays from the top.
        assert_eq!(s.queue.len(), 1);
    }

    #[test]
    fn enqueue_track_and_toggle_pause() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.queue.len(), 1);
        assert!(s.history.contains(&"m2".to_string()));
        let _ = s.update(Message::EnqueueTrack("nope".into()));
        assert_eq!(s.queue.len(), 1);
        let _ = s.update(Message::TogglePause);
        assert!(s.paused);
        let _ = s.update(Message::TogglePause);
        assert!(!s.paused);
    }

    #[test]
    fn queue_session_persists_and_restores_across_restart() {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir()
            .join(format!("yutu-queue-{}-{}.db", std::process::id(), n))
            .to_string_lossy()
            .into_owned();
        let (mut s, _) = AppState::with_library_path(path.clone());
        let tracks = yutu_core::MockBackend::fixture_tracks();
        s.register_tracks(tracks.clone());
        for t in &tracks {
            s.queue.push(t.clone());
        }
        s.queue.jump_to(2);
        s.position_secs = 42.0;
        s.queue.shuffle = true;
        s.queue.repeat = yutu_player::RepeatMode::All;
        s.volume = 0.5;
        s.persist_queue();
        drop(s);
        // Reopen: queue, index, position and modes come back paused.
        let (s2, _) = AppState::with_library_path(path);
        assert_eq!(s2.queue.len(), 5);
        assert_eq!(s2.queue.position(), Some(2));
        assert_eq!(s2.queue.current().map(|t| t.id.as_str()), Some("m3"));
        assert!((s2.position_secs - 42.0).abs() < 1e-6);
        assert!(s2.queue.shuffle);
        assert_eq!(s2.queue.repeat, yutu_player::RepeatMode::All);
        assert!((s2.volume - 0.5).abs() < f32::EPSILON);
        assert!(s2.paused);
        assert!(s2.restored_pending);
        assert!(s2.status.contains("Restored queue"));
    }

    #[test]
    fn queue_restore_drops_unknown_ids() {
        let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir()
            .join(format!("yutu-queue-{}-{}.db", std::process::id(), n))
            .to_string_lossy()
            .into_owned();
        let store = LibraryStore::open(&path).unwrap();
        store
            .save_queue(&QueueSnapshot {
                ids: vec!["ghost".into()],
                index: Some(0),
                position_secs: 10.0,
                shuffle: false,
                repeat: "off".into(),
                volume: 0.8,
            })
            .unwrap();
        let (s, _) = AppState::with_library_path(path);
        assert!(s.queue.is_empty());
        assert!(!s.restored_pending);
        assert_eq!(s.status, "Ready: search for music.");
    }

    #[test]
    fn restored_play_loads_saved_track_then_seeks() {
        let mut s = test_state();
        s.queue.push(mk_track("v1", "Song", "A"));
        s.position_secs = 42.0;
        s.restored_pending = true;
        s.paused = true;
        // No audio backend in tests: first play loads the saved track and
        // arms the saved-position seek.
        let _ = s.update(Message::TogglePause);
        assert_eq!(s.loading_track.as_deref(), Some("v1"));
        assert_eq!(s.pending_resume, Some(42.0));
        assert!(!s.restored_pending);
        // Failure drops the pending seek instead of firing it later.
        let _ = s.update(Message::StreamResolved {
            video_id: "v1".into(),
            result: Err("offline".into()),
        });
        assert_eq!(s.pending_resume, None);
    }

    #[test]
    fn tick_saver_writes_queue_changes() {
        let mut s = test_state();
        s.queue.push(mk_track("v1", "Song", "A"));
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        let store = LibraryStore::open(&s.library_path).unwrap();
        let snap = store.load_queue().unwrap().expect("saved");
        assert_eq!(snap.ids, vec!["v1".to_string()]);
        assert_eq!(snap.index, Some(0));
    }

    #[test]
    fn eq_preset_selection_valid_and_invalid() {
        let mut s = test_state();
        assert_eq!(s.eq_preset, "Flat");
        let _ = s.update(Message::SelectEqPreset("Rock".into()));
        assert_eq!(s.eq_preset, "Rock");
        assert!(s.status.contains("EQ: Rock"));
        assert!(s.status.contains("EQ: Rock"));
        // Case-insensitive lookup keeps the canonical name.
        let _ = s.update(Message::SelectEqPreset("pop".into()));
        assert_eq!(s.eq_preset, "Pop");
        // Unknown names leave the selection unchanged.
        let _ = s.update(Message::SelectEqPreset("Nope".into()));
        assert_eq!(s.eq_preset, "Pop");
        assert!(s.status.contains("Unknown EQ preset"));
    }

    #[test]
    fn discovery_sections_nonempty_with_seeded_history() {
        let mut s = test_state();
        let _ = s.update(Message::Like("m1".into()));
        let _ = s.update(Message::Like("m2".into()));
        // Seed plays older than 24h so the Weekly Mix doesn't skip them.
        let old = now_unix() - 5 * 86_400;
        let store = LibraryStore::open(&s.library_path).unwrap();
        store.record_play("m2", old).unwrap();
        store.record_play("m3", old).unwrap();
        let weekly = s.weekly_mix_tracks();
        assert!(!weekly.is_empty(), "weekly mix should be non-empty");
        let forgotten = s.forgotten_tracks();
        // m1 is liked and never played -> forgotten.
        assert!(forgotten.iter().any(|t| t.id == "m1"));
        // Play-all appends via the existing queue path.
        let _ = s.update(Message::PlayWeeklyMix);
        assert_eq!(s.queue.len(), weekly.len());
        assert!(s.status.contains("Weekly Mix"));
        let _ = s.update(Message::PlayForgotten);
        assert_eq!(s.queue.len(), weekly.len() + forgotten.len());
    }

    #[test]
    fn discovery_play_all_empty_is_graceful() {
        let s = test_state();
        assert!(s.weekly_mix_tracks().is_empty());
        assert!(s.forgotten_tracks().is_empty());
        let mut s = s;
        let _ = s.update(Message::PlayWeeklyMix);
        assert!(s.status.contains("nothing to queue"));
        let _ = s.update(Message::PlayForgotten);
        assert!(s.status.contains("nothing to queue"));
    }

    #[test]
    fn scrobble_status_on_enqueue_when_enabled() {
        let mut s = test_state();
        assert!(!s.scrobble_enabled);
        let _ = s.update(Message::ScrobbleToggled(true));
        assert!(s.scrobble_enabled);
        assert!(s.status.contains("credentials"));
        let _ = s.update(Message::ScrobbleUserChanged("alice".into()));
        assert_eq!(s.scrobble_user, "alice");
        // No credentials: eligibility is reported, nothing submitted.
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert!(s.status.contains("Scrobble-ready"));
        #[cfg(feature = "submit")]
        assert!(s.status.contains("Settings"));
        #[cfg(not(feature = "submit"))]
        assert!(s.status.contains("--features submit"));
        // ListenBrainz token set: a submit task is produced (lazy: no
        // network runs until the Iced runtime polls it).
        let _ = s.update(Message::ScrobbleLbTokenChanged("tok".into()));
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        #[cfg(feature = "submit")]
        {
            assert!(s.status.contains("Loading"));
            assert!(!s.status.contains("Scrobble-ready"));
        }
        #[cfg(not(feature = "submit"))]
        assert!(s.status.contains("--features submit"));
        // Credential inputs round-trip.
        let _ = s.update(Message::ScrobbleFmKeyChanged("k".into()));
        let _ = s.update(Message::ScrobbleFmSecretChanged("sec".into()));
        let _ = s.update(Message::ScrobbleFmSkChanged("sk".into()));
        assert_eq!(s.scrobble_fm_key, "k");
        // Disabled by default: no scrobble text on enqueue.
        let mut s2 = test_state();
        let _ = s2.update(Message::EnqueueTrack("m1".into()));
        assert!(!s2.status.contains("Scrobble-ready"));
        // Submit results update the status line honestly.
        let _ = s.update(Message::ScrobbleSubmitted {
            target: "ListenBrainz".into(),
            result: Ok(()),
        });
        assert!(s.status.contains("Scrobbled to ListenBrainz"));
        let _ = s.update(Message::ScrobbleSubmitted {
            target: "Last.fm".into(),
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed: 403"));
    }

    #[test]
    fn mpris_publish_never_panics_headless() {
        // No handle (tests): publish is a no-op.
        let s = test_state();
        s.publish_mpris();
        // Volume clamping + status mapping are covered by publish internals;
        // reaching here without a handle or bus is the assertion.
    }

    #[test]
    fn mpris_publishes_track_and_status_to_shared_handle() {
        let handle: yutu_desktop::SharedPlayer =
            std::sync::Arc::new(std::sync::Mutex::new(yutu_desktop::MprisPlayer::default()));
        let mut s = test_state();
        s.mpris = Some(handle.clone());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let player = handle.lock().unwrap();
        assert_eq!(player.now_playing().title, "Come Together");
        assert_eq!(player.now_playing().artist, "The Beatles");
        assert_eq!(player.status(), yutu_desktop::PlaybackStatus::Playing);
        assert_eq!(player.volume(), f64::from(s.volume));
        drop(player);
        // Pause propagates too.
        let _ = s.update(Message::TogglePause);
        assert_eq!(
            handle.lock().unwrap().status(),
            yutu_desktop::PlaybackStatus::Paused
        );
    }

    #[test]
    fn click_to_play_loads_then_resolves_or_reports() {
        let mut s = test_state();
        // Clicking a song marks it loading (fetch runs async, no network here).
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        assert!(s.status.contains("Loading"));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        // Stale arrival (user moved on): dropped silently.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Ok(vec![1, 2, 3]),
        });
        assert_eq!(s.loading_track, None);
        assert!(s.queue.current().map(|t| t.id.as_str()) == Some("m2"));
        // Fetch failure for the current track reports honestly.
        let _ = s.update(Message::StreamResolved {
            video_id: "m2".into(),
            result: Err("403 Forbidden".into()),
        });
        assert!(s.status.contains("m2") || s.status.contains("Playback failed"));
    }

    #[test]
    fn stream_resolved_reports_errors() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Err("403 Forbidden".into()),
        });
        assert!(s.status.contains("m1") && s.status.contains("403"));
        assert_eq!(s.loading_track, None);
        // Empty bytes: whatever the backend does (play async / refuse /
        // no device), it must settle loading and report: never panic.
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Ok(Vec::new()),
        });
        assert_eq!(s.loading_track, None);
        assert!(
            s.status.contains("Playing:")
                || s.status.contains("no audio device")
                || s.status.contains("failed"),
            "unexpected status: {}",
            s.status
        );
    }

    #[test]
    fn queue_many_merges_catalog_art() {
        let mut s2 = test_state();
        s2.catalog.insert(
            "m1".into(),
            Track {
                id: "m1".into(),
                title: "M1".into(),
                artist: "A".into(),
                album: None,
                duration_secs: 1,
                thumbnail_url: Some("https://example.invalid/cover".into()),
                thumbnail_large_url: None,
                plays: None,
            },
        );
        let _ = s2.update(Message::CreatePlaylist("Gym".into()));
        let pid = s2.user_playlists[0].id.clone();
        s2.user_playlists[0].tracks.push(Track {
            id: "m1".into(),
            title: "M1".into(),
            artist: "A".into(),
            album: None,
            duration_secs: 1,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
        });
        let _ = s2.update(Message::PlayPlaylist(pid));
        assert_eq!(
            s2.queue.items()[0].thumbnail_url.as_deref(),
            Some("https://example.invalid/cover")
        );
    }

    #[test]
    fn user_playlists_survive_restart() {
        let path =
            std::env::temp_dir().join(format!("yutu-test-restart-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let p = path.to_string_lossy().into_owned();
        let (mut s, _) = AppState::with_library_path(p.clone());
        for t in yutu_core::MockBackend::fixture_tracks() {
            s.catalog.insert(t.id.clone(), t);
        }
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: "m1".into(),
        });
        drop(s);
        let (s2, _) = AppState::with_library_path(p);
        assert_eq!(s2.user_playlists.len(), 1);
        assert_eq!(s2.user_playlists[0].title, "Gym");
        assert_eq!(s2.user_playlists[0].tracks.len(), 1);
        assert_eq!(s2.user_playlists[0].tracks[0].id, "m1");
        // Loaded ids reserve the sequence: next create must not collide.
        // (seq resumes from max uN.)
        // Startup seeding: the persisted track resolves with no search.
        assert!(s2.find_track("m1").is_some());
    }

    #[test]
    fn favorites_survive_restart_without_search() {
        let path = std::env::temp_dir().join(format!(
            "yutu-test-fav-restart-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let p = path.to_string_lossy().into_owned();
        let (mut s, _) = AppState::with_library_path(p.clone());
        // Like via the full catalog row (search result shape with art).
        let liked = Track {
            id: "stone-cold".into(),
            title: "Stone Cold Crazy".into(),
            artist: "Queen".into(),
            album: Some("Sheer Heart Attack".into()),
            duration_secs: 135,
            thumbnail_url: Some("https://example.invalid/s".into()),
            thumbnail_large_url: Some("https://example.invalid/l".into()),
            plays: None,
        };
        s.catalog.insert(liked.id.clone(), liked.clone());
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: AppState::FAVORITES_ID.into(),
            track_id: liked.id.clone(),
        });
        assert!(s.liked.contains("stone-cold"));
        drop(s);
        // Fresh launch with no search: Favorites renders from the cache.
        let (s2, _) = AppState::with_library_path(p);
        assert!(s2.liked.contains("stone-cold"));
        let favs = s2.favorite_tracks();
        assert_eq!(favs.len(), 1, "liked song must survive restart");
        assert_eq!(favs[0].title, "Stone Cold Crazy");
        assert_eq!(favs[0].artist, "Queen");
        assert!(s2.find_track("stone-cold").is_some());
        // Art seeded too (no placeholder after restart).
        assert!(s2.find_track("stone-cold").unwrap().thumbnail_url.is_some());
    }

    #[test]
    fn register_tracks_writes_through_to_cache() {
        let mut s = test_state();
        let t = mk_track("cache-me", "Cached", "Someone");
        let _ = s.register_tracks(vec![t]);
        let store = LibraryStore::open(&s.library_path).unwrap();
        assert!(store.cached_track("cache-me").unwrap().is_some());
    }

    #[test]
    fn missing_ids_covers_bare_likes_and_artless_rows() {
        let mut s = test_state();
        // Bare like with no catalog entry at all.
        s.like_id("ghost-like");
        assert!(s.missing_track_ids().contains(&"ghost-like".to_string()));
        // Catalog row with no art at all needs hydration for covers.
        s.catalog.insert(
            "artless".into(),
            Track {
                id: "artless".into(),
                title: "Artless".into(),
                artist: "Someone".into(),
                album: None,
                duration_secs: 200,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
        );
        s.liked_order.push("artless".into());
        s.liked.insert("artless".into());
        assert!(s.missing_track_ids().contains(&"artless".to_string()));
        // Row with art needs nothing.
        s.catalog.insert(
            "artful".into(),
            Track {
                id: "artful".into(),
                title: "Artful".into(),
                artist: "Someone".into(),
                album: None,
                duration_secs: 200,
                thumbnail_url: Some("https://example.invalid/s".into()),
                thumbnail_large_url: None,
                plays: None,
            },
        );
        s.liked_order.push("artful".into());
        s.liked.insert("artful".into());
        assert!(!s.missing_track_ids().contains(&"artful".to_string()));
    }

    #[test]
    fn hydrated_tracks_register_cache_and_warm() {
        let mut s = test_state();
        s.like_id("hydra");
        assert!(s.find_track("hydra").is_none());
        let incoming = Track {
            id: "hydra".into(),
            title: "Hydra Song".into(),
            artist: "Queen".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: Some("https://example.invalid/s".into()),
            thumbnail_large_url: None,
            plays: None,
        };
        let _ = s.update(Message::TracksHydrated {
            requested: vec!["hydra".into()],
            tracks: vec![incoming],
        });
        // Catalog + Favorites now resolve with no search.
        assert!(s.find_track("hydra").is_some());
        assert_eq!(s.favorite_tracks().len(), 1);
        assert_eq!(s.favorite_tracks()[0].title, "Hydra Song");
        // Cache write-through: a restart would keep it.
        let store = LibraryStore::open(&s.library_path).unwrap();
        assert!(store.cached_track("hydra").unwrap().is_some());
    }

    #[test]
    fn unknown_ids_fetch_instead_of_not_found() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("ghost".into()));
        assert!(s.status.contains("Fetching info"));
        let _ = s.update(Message::QueueTrack("ghost2".into()));
        assert!(s.status.contains("Fetching info"));
        let _ = s.update(Message::PlayNext("ghost3".into()));
        assert!(s.status.contains("Fetching info"));
    }

    #[test]
    fn startup_does_zero_network_work() {
        // A bare liked id (old install, no cached row) must not trigger
        // any fetch or write during open: the window appears instantly,
        // cached titles plus disk covers paint first.
        let path = std::env::temp_dir().join(format!(
            "yutu-test-lazy-start-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let p = path.to_string_lossy().into_owned();
        {
            let store = LibraryStore::open(&p).unwrap();
            store.like("bare-like").unwrap();
        }
        let (mut s, _) = AppState::with_library_path(p);
        assert!(s.liked.contains("bare-like"));
        // Nothing resolved yet (no eager sweep), nothing in flight.
        assert!(s.find_track("bare-like").is_none());
        assert!(s.hydrating.is_empty());
        assert!(s.artwork_loading.is_empty());
        assert!(!s.lazy_init_done);
        // First Tick after the window shows: only the Home screenful.
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!(s.lazy_init_done);
        assert!(s.hydrating.contains("bare-like"));
        assert!(s.hydrating.len() <= 8);
    }

    #[test]
    fn first_tick_hydrates_only_home_screenful() {
        let mut s = test_state();
        // 20 bare liked ids: Home shows at most 8 (fallback grid).
        for i in 0..20 {
            s.like_id(&format!("bare-{i}"));
        }
        assert_eq!(s.route, Route::Home);
        let _ = s.ensure_visible_tasks();
        // Capped small batch, deduped in flight.
        assert!(!s.hydrating.is_empty());
        assert!(s.hydrating.len() <= 8);
        let n = s.hydrating.len();
        // Second call while in flight: no duplicates, no growth.
        let _ = s.ensure_visible_tasks();
        assert_eq!(s.hydrating.len(), n);
    }

    #[test]
    fn navigation_hydrates_playlist_head_only() {
        let mut s = test_state();
        // Playlist with 30 bare ids (no catalog rows at all).
        let tracks: Vec<Track> = (0..30)
            .map(|i| Track {
                id: format!("pl-{i}"),
                title: format!("Song {i}"),
                artist: "Someone".into(),
                album: None,
                duration_secs: 200,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            })
            .collect();
        s.user_playlists.push(Playlist {
            id: "u9".into(),
            title: "Big".into(),
            tracks,
        });
        let _ = s.update(Message::OpenPlaylist("u9".into()));
        assert_eq!(s.route, Route::PlaylistDetail("u9".into()));
        // Visible scope is the head (15), metadata batch capped at 8.
        assert!(s.hydrating.len() <= 8);
        assert!(!s.hydrating.is_empty());
    }

    #[test]
    fn visible_ids_capped_per_route() {
        let mut s = test_state();
        for i in 0..30 {
            s.like_id(&format!("v-{i}"));
        }
        s.route = Route::Library;
        assert!(s.visible_ids().len() <= 12);
        s.route = Route::Home;
        // Home fallback grid shows at most 8 favorites.
        assert!(s.visible_ids().len() <= 8);
    }

    #[test]
    fn hydrated_empty_cools_down_before_navigation_retries() {
        let mut s = test_state();
        s.hydrating.insert("retry-me".into());
        let _ = s.update(Message::TracksHydrated {
            requested: vec!["retry-me".into()],
            tracks: vec![],
        });
        assert!(!s.hydrating.contains("retry-me"));
        // Unresolvable ids cool down: an immediate re-scan (art arrival
        // chaining) must not re-probe at once.
        s.like_id("retry-me");
        s.route = Route::Library;
        let _ = s.ensure_visible_tasks();
        assert!(!s.hydrating.contains("retry-me"));
    }

    #[test]
    fn lazy_warmer_covers_hero_keys_for_home_cards() {
        // Regression: Home "Jump back in" cards read the 544px hero key
        // (thumb_hero), but the lazy warmer only queued the raw thumb, so
        // covers stayed placeholder until a play warmed both keys.
        let mut s = test_state();
        let t = Track {
            id: "hero-card".into(),
            title: "Hero Card".into(),
            artist: "Someone".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: Some("https://example.invalid/s=w60-h60-l90-rj".into()),
            thumbnail_large_url: Some("https://example.invalid/l=w120-h120-l90-rj".into()),
            plays: None,
        };
        let hero = yutu_core::hires_thumb_url(t.thumbnail_large_url.as_deref().unwrap(), 544);
        s.catalog.insert(t.id.clone(), t);
        s.history.push("hero-card".into());
        s.route = Route::Home;
        let _ = s.ensure_visible_tasks();
        assert!(s
            .artwork_loading
            .contains("https://example.invalid/s=w60-h60-l90-rj"));
        assert!(
            s.artwork_loading.contains(&hero),
            "hero key must warm or Home cards never leave placeholder"
        );
    }

    #[test]
    fn failed_artwork_cools_down_against_chained_rescans() {
        let mut s = test_state();
        let url = "https://example.invalid/dead.jpg".to_string();
        let t = Track {
            id: "dead-art".into(),
            title: "Dead Art".into(),
            artist: "Someone".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: Some(url.clone()),
            thumbnail_large_url: None,
            plays: None,
        };
        s.catalog.insert(t.id.clone(), t);
        s.history.push("dead-art".into());
        s.route = Route::Home;
        let _ = s.ensure_visible_tasks();
        assert!(s.artwork_loading.contains(&url));
        // The fetch fails: completion chaining must not requeue at once.
        let _ = s.update(Message::ArtworkReady {
            url: url.clone(),
            result: Err("404".into()),
        });
        assert!(!s.artwork_loading.contains(&url));
        let _ = s.ensure_visible_tasks();
        assert!(
            !s.artwork_loading.contains(&url),
            "failed covers cool down instead of refetch-looping"
        );
    }

    #[test]
    fn artwork_ready_chains_the_capped_remainder() {
        // Batch cap (12 tasks for ~6 two-key tracks) leaves remainder;
        // each arrival re-scans visible so the rest still loads.
        let mut s = test_state();
        for i in 0..8 {
            let id = format!("chain-{i}");
            s.catalog.insert(
                id.clone(),
                Track {
                    id: id.clone(),
                    title: format!("Chain {i}"),
                    artist: "Someone".into(),
                    album: None,
                    duration_secs: 200,
                    thumbnail_url: Some(format!("https://example.invalid/c{i}.jpg")),
                    thumbnail_large_url: None,
                    plays: None,
                },
            );
            s.history.push(id);
        }
        s.route = Route::Home;
        let _ = s.ensure_visible_tasks();
        let first_batch = s.artwork_loading.len();
        assert!(first_batch > 0);
        // One arrival chains the remainder: still progress, no duplicates.
        let url = s.artwork_loading.iter().next().unwrap().clone();
        let _ = s.update(Message::ArtworkReady {
            url: url.clone(),
            result: Ok(vec![0u8; 1024]),
        });
        assert!(s.artwork.contains_key(&url));
        assert!(
            !s.artwork_loading.is_empty(),
            "chained scan picks up the capped remainder"
        );
    }

    /// Full click-to-play path (minus audible sound): catalog insert →
    /// EnqueueTrack marks loading → resolve+fetch real bytes → StreamResolved
    /// hands them to the backend → status flips to playing.
    #[tokio::test]
    #[ignore = "live: needs yt-dlp + network + an audio backend (mpv loads headless)"]
    async fn live_click_to_play_real_song() {
        let Some(_) = yutu_core::find_yt_dlp() else {
            eprintln!("SKIP: yt-dlp not installed (or set YUTU_YTDLP_BIN)");
            return;
        };
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "JWW8KK8kd1I".into());
        let mut s = test_state();
        let Some(_) = &s.audio else {
            eprintln!("SKIP: no audio backend on this machine");
            return;
        };
        s.catalog.insert(
            video.clone(),
            Track {
                id: video.clone(),
                title: "live-proof".into(),
                artist: "yutu".into(),
                album: None,
                duration_secs: 200,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
        );
        let _ = s.update(Message::EnqueueTrack(video.clone()));
        assert_eq!(s.loading_track.as_deref(), Some(video.as_str()));
        let bytes = super::resolve_and_fetch(&video)
            .await
            .expect("yt-dlp resolve + chunked fetch");
        assert!(bytes.len() > 100_000, "suspiciously small stream");
        let _ = s.update(Message::StreamResolved {
            video_id: video,
            result: Ok(bytes),
        });
        assert_eq!(s.loading_track, None);
        assert!(
            s.status.contains("Playing:"),
            "expected playing status, got: {}",
            s.status
        );
        assert!(s.audio_position() >= 0.0);
        assert!(s.audio_duration() > 0.0);
    }

    /// Full offline-bundle path: download stores audio bytes on disk, the
    /// Downloaded mark persists, and the bytes play back with no resolve
    /// task (the offline-first branch of the play path).
    #[tokio::test]
    #[ignore = "live: needs yt-dlp + network (writes the real offline cache)"]
    async fn live_download_bundle_then_offline_play() {
        let Some(_) = yutu_core::find_yt_dlp() else {
            eprintln!("SKIP: yt-dlp not installed (or set YUTU_YTDLP_BIN)");
            return;
        };
        let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "JWW8KK8kd1I".into());
        let track = Track {
            id: video.clone(),
            title: "live-bundle-proof".into(),
            artist: "yutu".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
        };
        // Audio leg of the bundle task (lyrics/art are best-effort
        // sidecars over the same fetch helpers, covered offline above).
        let bytes = super::resolve_and_fetch(&video)
            .await
            .expect("yt-dlp resolve + fetch");
        assert!(bytes.len() > 100_000, "suspiciously small stream");
        let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
        cache.set_dir(super::offline_cache_dir());
        cache.store_bytes(&video, &bytes).unwrap();
        // Bytes are readable straight from disk (no tracker state: the
        // in-memory tracker is always fresh after a restart).
        let back = super::offline_bytes_in(&super::offline_cache_dir(), &video)
            .expect("downloaded bytes must read back");
        assert_eq!(back.len(), bytes.len());
        // Marking joins Downloaded; the offline branch then plays them.
        let mut s = test_state();
        s.catalog.insert(video.clone(), track.clone());
        let _ = s.update(Message::Downloaded {
            track_id: video.clone(),
            result: Ok(()),
        });
        assert!(s.downloaded.contains(&video));
        let played = s.try_play_offline(&track);
        if played {
            assert!(s.status.contains("(offline)"));
        } else {
            assert!(s.audio.is_none() || s.status.contains("Playback failed"));
        }
    }

    #[test]
    fn lyrics_ready_caches_and_open_lyrics_fetches_once() {
        use yutu_core::{LyricLine, LyricsResult};
        let mut s = test_state();
        // Unknown id: hydration kicked off, no lyrics fetch.
        let _ = s.update(Message::OpenLyrics("ghost".into()));
        assert_eq!(s.route, Route::Lyrics("ghost".into()));
        assert!(s.lyrics_loading.is_none());
        // Known track: fetch marked in flight (task runs on the runtime).
        let _ = s.update(Message::OpenLyrics("m1".into()));
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        #[cfg(not(feature = "lyrics"))]
        assert!(s.lyrics_loading.is_none());
        // Second open while loading: no duplicate fetch.
        let _ = s.update(Message::OpenLyrics("m1".into()));
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        // Arrival caches; failures report.
        let _ = s.update(Message::LyricsReady {
            track_id: "m1".into(),
            result: Ok(LyricsResult {
                plain: "la\nla".into(),
                synced: vec![LyricLine {
                    start_ms: 0,
                    text: "la".into(),
                }],
                instrumental: false,
            }),
        });
        assert!(s.lyrics_loading.is_none());
        assert_eq!(s.lyrics_cache["m1"].synced.len(), 1);
        let _ = s.update(Message::LyricsReady {
            track_id: "m2".into(),
            result: Err("offline".into()),
        });
        assert!(s.status.contains("Lyrics failed"));
    }

    #[test]
    fn url_expiry_parses_googlevideo_param() {
        assert_eq!(
            url_expiry("https://x.googlevideo.com/v?expire=1791411186&ei=abc"),
            Some(1791411186)
        );
        assert_eq!(url_expiry("https://example.invalid/audio.webm"), None);
        assert_eq!(url_expiry("https://x/?expire=notanumber"), None);
    }

    #[test]
    fn browser_back_forward_walk_history() {
        let mut s = test_state();
        assert!(s.nav_back.is_empty());
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        let _ = s.update(Message::Navigate(Route::Search));
        let _ = s.update(Message::Navigate(Route::Queue));
        assert_eq!(s.route, Route::Queue);
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Search);
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        // Exhausted: stays put.
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Home);
        let _ = s.update(Message::NavForward);
        assert_eq!(s.route, Route::Search);
        // New navigation clears the forward stack.
        let _ = s.update(Message::Navigate(Route::Library));
        let _ = s.update(Message::NavForward);
        assert_eq!(s.route, Route::Library);
    }

    #[test]
    fn home_search_stays_home_until_navigation() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("beatles".into()));
        assert_eq!(s.search_query, "beatles");
        assert_eq!(s.route, Route::Home);
        // Completion never kicks the user to another page.
        let _ = s.update(Message::SearchCompleted {
            query: "beatles".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![mk_track("v1", "Yesterday", "The Beatles")],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert_eq!(s.route, Route::Home);
        assert_eq!(s.results.tracks.len(), 1);
    }

    #[test]
    fn stale_search_completion_never_clobbers_newer_query() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("bonobo".into()));
        let gen = s.search_gen;
        // User kept typing: newer generation wins, stale debounce drops.
        let _ = s.update(Message::SearchDebounced {
            gen: gen.wrapping_sub(1),
            query: "bon".into(),
        });
        assert!(!s.status.contains("Searching"));
        // Out-of-order payload registers tracks but keeps newer results.
        let _ = s.update(Message::SearchCompleted {
            query: "bon".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![mk_track("old", "Old Song", "Someone")],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert!(s.find_track("old").is_some());
        assert!(s.results.tracks.is_empty());
        // Current payload lands normally.
        let _ = s.update(Message::SearchCompleted {
            query: "bonobo".into(),
            result: Ok(Box::new(bundle_of(SearchResults {
                tracks: vec![mk_track("new", "Rosewood", "Bonobo")],
                albums: vec![],
                artists: vec![],
                playlists: vec![],
            }))),
        });
        assert_eq!(s.results.tracks.len(), 1);
        assert_eq!(s.results.tracks[0].id, "new");
    }

    #[test]
    fn sidebar_and_right_tabs_switch() {
        let mut s = test_state();
        assert!(s.sidebar_pinned);
        let _ = s.update(Message::ToggleSidebarPin);
        assert!(!s.sidebar_pinned);
        assert!(s.right_pinned);
        let _ = s.update(Message::ToggleRightPin);
        assert!(!s.right_pinned);
        let _ = s.update(Message::SidebarFilterChanged("gym".into()));
        assert_eq!(s.sidebar_filter, "gym");
        assert_eq!(s.right_tab, RightTab::Queue);
        let _ = s.update(Message::SetRightTab(RightTab::Recent));
        assert_eq!(s.right_tab, RightTab::Recent);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn google_login_flow_reports_honestly() {
        let mut s = test_state();
        assert!(!s.logged_in);
        // No flow yet: polling says so.
        let _ = s.update(Message::GoogleLoginPoll);
        assert!(s.status.contains("Sign in"));
        // Start marks busy (network runs on the runtime).
        let _ = s.update(Message::GoogleLoginStart);
        assert!(s.login_busy);
        // Begin failure (no client id in test env) clears busy + reports.
        let _ = s.update(Message::GoogleLoginBegan {
            result: Err("no client".into()),
        });
        assert!(!s.login_busy);
        assert!(s.status.contains("Sign-in failed"));
        // Still-pending poll keeps the flow alive.
        let _ = s.update(Message::GoogleLoginPoll);
        assert!(s.status.contains("Sign in"));
        // Import without sign-in refuses; logout resets.
        let _ = s.update(Message::LibraryImport);
        assert!(s.status.contains("Sign in"));
        let _ = s.update(Message::GoogleLogout);
        assert!(!s.logged_in);
        assert!(s.status.contains("Signed out"));
        // Staged base merges catalog + likes at once (playlists stream next).
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Library,
            result: Ok(staged(
                vec![mk_track("lib1", "Lied", "Someone")],
                vec!["lib1".into()],
                vec![],
            )),
        });
        assert!(!s.login_busy);
        assert!(s.find_track("lib1").is_some());
        assert!(s.liked.contains("lib1"));
        assert!(s.status.contains("Import: 1"));
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Library,
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed: 403"));
    }

    #[test]
    fn play_now_replaces_queue_and_autoplay_appends() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.queue.len(), 2);
        // Click-to-play: queue replaced by the single + autoplay follows.
        let _ = s.update(Message::PlayNow("m3".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m3"));
        assert_eq!(s.autoplay_for.as_deref(), Some("m3"));
        assert_eq!(s.playing_from, "Single + autoplay");
        // Unknown id: fetch kicked off, queue untouched.
        let _ = s.update(Message::PlayNow("ghost".into()));
        assert_eq!(s.queue.len(), 1);
        // Stale autoplay (user moved on): catalog only.
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("old", "Old", "Someone")]),
        });
        assert_eq!(s.queue.len(), 1);
        assert!(s.find_track("old").is_some());
        // Current seed: seed + dupes filtered, tail appended (manual
        // queue untouched: Spotify split).
        let _ = s.update(Message::AutoplayReady {
            seed: "m3".into(),
            result: Ok(vec![
                mk_track("m3", "Dup", "X"),
                mk_track("m4", "Fresh", "Y"),
            ]),
        });
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.autoplay.len(), 1);
        assert_eq!(s.autoplay[0].id, "m4");
        // Peek sees the tail past the manual end (prefetch crosses over).
        assert_eq!(s.peek_next_id().as_deref(), Some("m4"));
        // Failures log without touching either list.
        let _ = s.update(Message::AutoplayReady {
            seed: "m3".into(),
            result: Err("offline".into()),
        });
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.autoplay.len(), 1);
        assert!(s.error_log.iter().any(|e| e.contains("Autoplay")));
    }

    #[test]
    fn shuffle_reorders_visibly_and_restores_on_off() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::EnqueueTrack("m3".into()));
        let _ = s.update(Message::EnqueueTrack("m4".into()));
        let _ = s.update(Message::QueueJump(1)); // current m2
        let _ = s.update(Message::ToggleShuffle);
        assert!(s.queue.shuffle);
        // Current leads at index 0; the rest is a permutation of the others.
        assert_eq!(s.queue.position(), Some(0));
        assert_eq!(s.queue.current().unwrap().id, "m2");
        let mut rest: Vec<&str> = s.queue.items()[1..].iter().map(|t| t.id.as_str()).collect();
        rest.sort_unstable();
        assert_eq!(rest, vec!["m1", "m3", "m4"]);
        assert!(s.pre_shuffle.is_some());
        // The walk visits every song exactly once, then ends.
        let mut seen = vec![s.queue.current().unwrap().id.clone()];
        while s.advance_queue() {
            seen.push(s.queue.current().unwrap().id.clone());
        }
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(sorted, vec!["m1", "m2", "m3", "m4"]);
        assert!(!s.advance_queue());
        // Toggle off: the exact original order returns, same song playing.
        let playing = s.queue.current().unwrap().id.clone();
        let _ = s.update(Message::ToggleShuffle);
        assert!(!s.queue.shuffle);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3", "m4"]);
        assert_eq!(s.queue.current().unwrap().id, playing);
        assert!(s.pre_shuffle.is_none());
        // Edits while shuffled survive the restore (appended at the end).
        let _ = s.update(Message::ToggleShuffle);
        let _ = s.update(Message::QueueTrack("m5".into()));
        let _ = s.update(Message::ToggleShuffle);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids[..4], ["m1", "m2", "m3", "m4"]);
        assert_eq!(ids[4], "m5");
    }

    #[test]
    fn shuffle_puts_current_first_from_middle_and_last() {
        for jump in [1, 3] {
            let mut s = test_state();
            for m in ["m1", "m2", "m3", "m4"] {
                let _ = s.update(Message::EnqueueTrack(m.into()));
            }
            let _ = s.update(Message::QueueJump(jump));
            let cur = s.queue.current().unwrap().id.clone();
            let _ = s.update(Message::ToggleShuffle);
            assert_eq!(s.queue.position(), Some(0));
            assert_eq!(s.queue.current().unwrap().id, cur);
            assert_eq!(s.queue.len(), 4);
            let _ = s.update(Message::ToggleShuffle);
            let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
            assert_eq!(ids, vec!["m1", "m2", "m3", "m4"]);
        }
    }

    #[test]
    fn shuffle_empty_and_single_are_noops() {
        let mut s = test_state();
        let _ = s.update(Message::ToggleShuffle);
        assert!(s.queue.shuffle);
        assert!(s.queue.items().is_empty());
        let _ = s.update(Message::ToggleShuffle);
        assert!(!s.queue.shuffle);
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::ToggleShuffle);
        assert_eq!(s.queue.position(), Some(0));
        assert_eq!(s.queue.len(), 1);
        let _ = s.update(Message::ToggleShuffle);
        assert_eq!(s.queue.items()[0].id, "m1");
    }

    #[test]
    fn shuffle_duplicate_ids_restore() {
        let mut s = test_state();
        for m in ["m1", "m2", "m1"] {
            let _ = s.update(Message::EnqueueTrack(m.into()));
        }
        // Snapshot on the second `m1`: restore must re-seat it, not the first.
        let _ = s.update(Message::QueueJump(2));
        assert_eq!(s.queue.position(), Some(2));
        let _ = s.update(Message::ToggleShuffle);
        assert_eq!(s.queue.len(), 3);
        assert_eq!(s.queue.position(), Some(0));
        assert_eq!(s.queue.current().unwrap().id, "m1");
        let _ = s.update(Message::ToggleShuffle);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m1"]);
        assert_eq!(s.queue.position(), Some(2));
    }

    #[test]
    fn shuffle_end_continues_into_autoplay() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        let _ = s.update(Message::ToggleShuffle);
        // Single-song order ends at once → autoplay head shifts in.
        assert!(s.advance_queue());
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m4"));
        assert!(s.autoplay.is_empty());
    }

    #[test]
    fn next_bypasses_repeat_one_but_end_replays() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::CycleRepeat);
        let _ = s.update(Message::CycleRepeat);
        assert_eq!(s.queue.repeat, yutu_player::RepeatMode::One);
        // Manual Next always advances, even under Repeat One.
        let _ = s.update(Message::Next);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        // Natural end-of-track path still replays under Repeat One.
        assert!(s.advance_queue());
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        assert!(s.autoplay.is_empty());
        // Manual Next at the tail under Repeat One: silent no-op (no
        // replay, no autoplay, position unchanged).
        let _ = s.update(Message::Next);
        assert_eq!(s.queue.position(), Some(1));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    }

    #[test]
    fn queue_track_appends_without_playing() {
        let mut s = test_state();
        // Empty queue: appends, nothing loads or plays.
        let _ = s.update(Message::QueueTrack("m1".into()));
        assert_eq!(s.queue.len(), 1);
        assert!(s.loading_track.is_none());
        assert!(s.status.contains("Added to queue"));
        // Playing something: appends to the end, current untouched.
        let _ = s.update(Message::PlayNow("m2".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        let _ = s.update(Message::QueueTrack("m3".into()));
        assert_eq!(s.queue.len(), 2);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        assert_eq!(s.queue.items()[1].id, "m3");
        assert_eq!(s.loading_track.as_deref(), Some("m2"));
        assert!(s.status.contains("Added to queue"));
        // Unknown id: fetch kicked off, queue untouched.
        let _ = s.update(Message::QueueTrack("ghost".into()));
        assert!(s.status.contains("Fetching info"));
        assert_eq!(s.queue.len(), 2);
    }

    #[test]
    fn new_load_resets_position_and_marks_loading() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        assert_eq!(s.position_secs, 0.0);
        assert!(s.status.contains("Loading"));
    }

    #[test]
    fn play_next_inserts_after_current() {
        let mut s = test_state();
        // Empty queue: behaves like enqueue-and-play.
        let _ = s.update(Message::PlayNext("m1".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::PlayNext("m3".into()));
        assert_eq!(s.queue.items()[1].id, "m3");
        assert_eq!(s.queue.position(), Some(0));
        assert!(s.status.contains("Queued next"));
        let _ = s.update(Message::PlayNext("ghost".into()));
        assert!(s.status.contains("Fetching info"));
    }

    #[test]
    fn context_menu_opens_navigates_and_closes() {
        let mut s = test_state();
        assert!(s.context_menu.is_none());
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
        let _ = s.update(Message::CloseContext);
        assert!(s.context_menu.is_none());
        // Album/artist jump from the menu (m1 carries both).
        let _ = s.update(Message::OpenContext("m1".into()));
        let _ = s.update(Message::OpenTrackArtist("m1".into()));
        assert!(matches!(s.route, Route::Artist(_)));
        assert!(s.context_menu.is_none());
        // Unknown track: fetch kicked off.
        let _ = s.update(Message::OpenTrackAlbum("ghost".into()));
        assert!(s.status.contains("Fetching info"));
        // Download validates the id before touching the network.
        let _ = s.update(Message::DownloadTrack("ghost".into()));
        assert!(s.status.contains("Fetching info"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.status.contains("Downloaded"));
        assert!(s.downloaded.contains("m1"));
        assert!(s.playlist_has("__downloaded", "m1"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed"));
    }

    #[test]
    fn suggestions_store_and_drop_stale() {
        let mut s = test_state();
        let _ = s.update(Message::HomeSearchChanged("bon".into()));
        let gen = s.suggest_gen;
        assert!(s.show_suggest);
        let _ = s.update(Message::SuggestCompleted {
            gen: gen.wrapping_sub(1),
            result: Ok(vec!["stale".into()]),
        });
        assert!(s.suggest.is_empty());
        let _ = s.update(Message::SuggestCompleted {
            gen,
            result: Ok(vec!["bonobo".into(), "bon iver".into()]),
        });
        assert_eq!(s.suggest.len(), 2);
        // Enter hides the popup and lands on Search.
        let _ = s.update(Message::SearchStay);
        assert!(!s.show_suggest);
        assert_eq!(s.route, Route::Search);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn cookie_import_validates_and_merges() {
        let mut s = test_state();
        let _ = s.update(Message::CookiePathChanged("/tmp/x.txt".into()));
        assert_eq!(s.cookie_path, "/tmp/x.txt");
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(
                vec![mk_track("c1", "Cookie Song", "Someone")],
                vec!["c1".into()],
                vec![],
            )),
        });
        assert!(s.find_track("c1").is_some());
        assert!(s.liked.contains("c1"));
        assert!(s.status.contains("Cookie import"));
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Err("bad cookies".into()),
        });
        assert!(s.status.contains("bad cookies"));
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn staged_import_streams_playlists_with_progress() {
        use yutu_core::Playlist;
        let mut s = test_state();
        // Base stage: songs land immediately, playlists fan out next.
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(
                vec![mk_track("c1", "Cookie Song", "Someone")],
                vec!["c1".into()],
                vec![("PLserver1", "Road Trip"), ("PLserver2", "Chill")],
            )),
        });
        assert!(s.find_track("c1").is_some());
        assert!(s.login_busy);
        assert!(s.status.contains("fetching 2 playlists"));
        assert_eq!((s.import_done, s.import_total), (0, 2));
        let gen = s.import_gen;
        // First playlist arrives: merged under its real id, still busy.
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Road Trip".into(),
            index: 0,
            total: 2,
            count: None,
            result: Ok(Playlist {
                id: "PLserver1".into(),
                title: "Road Trip".into(),
                tracks: vec![mk_track("p1", "Pl Song", "Someone")],
            }),
        });
        assert!(s.user_playlists.iter().any(|p| p.id == "PLserver1"));
        assert!(s.find_track("p1").is_some());
        assert!(s.login_busy);
        assert!(s.status.contains("1/2"));
        // A failed list reports but does not stall the import.
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Chill".into(),
            index: 1,
            total: 2,
            count: None,
            result: Err("400".into()),
        });
        assert!(!s.login_busy);
        // Release builds only surface failures: the terminal line must
        // carry "fail" (see `status_text`).
        assert!(s.status.contains("complete with failures"));
        assert!(s.status.to_lowercase().contains("fail"));
        assert!(s.status.contains("2/2"));
    }

    #[cfg(feature = "innertube")]
    #[tokio::test]
    async fn tick_forward_stream_preserves_order_and_ends() {
        use futures::StreamExt as _;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send((0usize, Some(248))).unwrap();
        tx.send((100, Some(248))).unwrap();
        tx.send((248, Some(248))).unwrap();
        drop(tx);
        let out: Vec<Message> = AppState::tick_forward_stream(rx, 7, 0).collect().await;
        let arrived: Vec<usize> = out
            .iter()
            .map(|m| match m {
                Message::PlaylistTick {
                    gen,
                    index,
                    arrived,
                    count,
                } => {
                    assert_eq!((*gen, *index), (7, 0));
                    assert_eq!(*count, Some(248));
                    *arrived
                }
                other => panic!("expected PlaylistTick, got {other:?}"),
            })
            .collect();
        assert_eq!(arrived, vec![0, 100, 248]);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn playlist_ticks_move_song_counters() {
        let mut s = test_state();
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(
                vec![mk_track("c1", "Cookie Song", "Someone")],
                vec![],
                vec![
                    ("PLbig", "Big"),
                    ("PLmystery", "Mystery"),
                    ("PLdoomed", "Doomed"),
                ],
            )),
        });
        let gen = s.import_gen;
        // Base songs seed the expected total; nothing arrived yet.
        assert_eq!(s.import_expected, 1);
        assert!(s.import_banner_view().is_some());
        // Page ticks accumulate per list plus the details total.
        let tick = |index: usize, arrived: usize, count: Option<usize>| Message::PlaylistTick {
            gen,
            index,
            arrived,
            count,
        };
        let _ = s.update(tick(0, 100, Some(248)));
        assert_eq!(s.import_expected, 1 + 248);
        let _ = s.update(tick(0, 200, Some(248)));
        let _ = s.update(tick(1, 50, None));
        // Unknown-total lists stay out of the bar math (no denominator).
        assert_eq!(s.import_expected, 1 + 248);
        // Display eases toward the truth instead of teleporting: pages
        // land ~100 songs apart microseconds from each other.
        assert_eq!(s.import_shown, 1);
        assert_eq!(s.import_song_target(), 1 + 200);
        s.ease_import_shown();
        assert_eq!(s.import_shown, 1 + 100);
        s.ease_import_shown();
        assert_eq!(s.import_shown, 1 + 150);
        for _ in 0..20 {
            s.ease_import_shown();
        }
        assert_eq!(s.import_shown, s.import_song_target());
        // Overshoot snaps back down (failed lists shrink the target).
        s.import_shown = 9999;
        s.ease_import_shown();
        assert_eq!(s.import_shown, s.import_song_target());
        // Late ticks for finished lists are ignored (channel vs Done race).
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Big".into(),
            index: 0,
            total: 3,
            count: Some(248),
            result: Ok(yutu_core::Playlist {
                id: "PLbig".into(),
                title: "Big".into(),
                tracks: vec![mk_track("b1", "Big Song", "Someone")],
            }),
        });
        assert!(s.login_busy);
        assert_eq!(s.import_songs, 2);
        assert!(!s.import_live.contains_key(&0));
        let _ = s.update(tick(0, 248, Some(248)));
        assert!(!s.import_live.contains_key(&0));
        assert_eq!(s.import_songs, 2);
        // Failed lists drop their expected share so the bar can still finish.
        // (Mystery had no known total: nothing was ever added.)
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Mystery".into(),
            index: 1,
            total: 3,
            count: None,
            result: Err("400".into()),
        });
        assert!(s.login_busy);
        assert_eq!(s.import_expected, 1 + 248);
        // Known-total failure subtracts its share.
        let _ = s.update(tick(2, 10, Some(60)));
        assert_eq!(s.import_expected, 1 + 248 + 60);
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Doomed".into(),
            index: 2,
            total: 3,
            count: Some(60),
            result: Err("500".into()),
        });
        assert!(!s.login_busy);
        assert!(!s.import_active);
        assert!(s.import_banner_view().is_none());
        assert_eq!(s.import_expected, 1 + 248);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn stale_playlist_arrivals_are_ignored() {
        let mut s = test_state();
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(vec![], vec![], vec![("PLnew", "New")])),
        });
        // Straggler from a previous run (old generation): no merge,
        // no counter movement, no status change.
        s.status = "steady".into();
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen: s.import_gen.wrapping_sub(1),
            title: "Ghost".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PLghost".into(),
                title: "Ghost".into(),
                tracks: vec![mk_track("g1", "Ghost Song", "No One")],
            }),
        });
        assert!(!s.user_playlists.iter().any(|p| p.id == "PLghost"));
        assert!(s.find_track("g1").is_none());
        assert_eq!(s.import_done, 0);
        assert_eq!(s.status, "steady");
        assert!(s.login_busy);
        // Out-of-order arrivals (index 1 before 0) both merge and finish.
        let gen = s.import_gen;
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "New".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PLnew".into(),
                title: "New".into(),
                tracks: vec![],
            }),
        });
        assert!(!s.login_busy);
        assert!(s.user_playlists.iter().any(|p| p.id == "PLnew"));
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn persist_imported_writes_store_without_state() {
        // The background-task half of an import: needs no AppState, so it
        // never runs on the GUI thread.
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "yutu-persist-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_str = path.to_string_lossy().into_owned();
        let tracks = vec![mk_track("w1", "W Song", "Someone")];
        let liked = vec!["w1".into(), "bare-id".into()];
        let playlists = vec![yutu_core::Playlist {
            id: "PLw".into(),
            title: "W List".into(),
            tracks: tracks.clone(),
        }];
        AppState::persist_imported(&path_str, &tracks, &liked, &playlists);
        let store = yutu_core::LibraryStore::open(&path_str).unwrap();
        assert!(store.cached_track("w1").unwrap().is_some());
        assert!(store.liked_ids().unwrap().contains(&"w1".to_string()));
        assert!(store.liked_ids().unwrap().contains(&"bare-id".to_string()));
        let lists = store.user_playlists().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].tracks.len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn import_banner_tracks_active_imports() {
        let mut s = test_state();
        // Idle: no banner, layout untouched.
        assert!(s.import_banner_view().is_none());
        // Trigger shows the banner in base stage (totals unknown yet).
        s.cookie_path = "/tmp/x.txt".into();
        let _ = s.update(Message::CookieImport);
        assert!(s.import_active);
        assert!(s.import_banner_view().is_some());
        // Base stage with playlists keeps it up with counts.
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(vec![], vec![], vec![("PL1", "One")])),
        });
        assert!(s.import_active);
        assert!(s.import_banner_view().is_some());
        assert_eq!((s.import_done, s.import_total), (0, 1));
        // Final arrival hides it again.
        let gen = s.import_gen;
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "One".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PL1".into(),
                title: "One".into(),
                tracks: vec![],
            }),
        });
        assert!(!s.import_active);
        assert!(s.import_banner_view().is_none());
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn import_merges_server_playlists_with_real_ids() {
        let mut s = test_state();
        // Base stage first (sets the run generation + totals).
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Ok(staged(vec![], vec![], vec![("PLserver1", "Road Trip")])),
        });
        let gen = s.import_gen;
        // One playlist arriving through the staged flow.
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Road Trip".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PLserver1".into(),
                title: "Road Trip".into(),
                tracks: vec![mk_track("p1", "Pl Song", "Someone")],
            }),
        });
        // Server list lands in memory under its real id (syncable).
        let pl = s.user_playlists.iter().find(|p| p.id == "PLserver1");
        assert!(pl.is_some());
        assert!(AppState::is_server_playlist("PLserver1"));
        assert!(!AppState::is_server_playlist("u7"));
        assert!(!AppState::is_server_playlist("LM"));
        assert!(!AppState::is_server_playlist("u"));
        assert!(!AppState::is_server_playlist(AppState::FAVORITES_ID));
        assert!(!AppState::is_server_playlist(AppState::DOWNLOADED_ID));
        // Tracks register into the catalog + SQLite track cache.
        assert!(s.find_track("p1").is_some());
        assert!(s.status.contains("1 playlists"));
        // Re-import refreshes in place (no duplicate list).
        let _ = s.update(Message::PlaylistImported {
            kind: ImportKind::Cookie,
            gen,
            title: "Road Trip Renamed".into(),
            index: 0,
            total: 1,
            count: None,
            result: Ok(yutu_core::Playlist {
                id: "PLserver1".into(),
                title: "Road Trip Renamed".into(),
                tracks: vec![],
            }),
        });
        assert_eq!(
            s.user_playlists
                .iter()
                .filter(|p| p.id == "PLserver1")
                .count(),
            1
        );
        assert_eq!(
            s.user_playlists
                .iter()
                .find(|p| p.id == "PLserver1")
                .unwrap()
                .title,
            "Road Trip Renamed"
        );
    }

    #[cfg(feature = "innertube")]
    #[test]
    fn server_sync_results_report_honestly() {
        let mut s = test_state();
        // Like failure reports but keeps the local state.
        s.like_id("m1");
        let _ = s.update(Message::LikeSynced {
            track_id: "m1".into(),
            liked: true,
            result: Err("403".into()),
        });
        assert!(s.status.contains("failed"));
        assert!(s.liked.contains("m1"));
        // Success stays silent.
        s.status = "quiet".into();
        let _ = s.update(Message::LikeSynced {
            track_id: "m1".into(),
            liked: true,
            result: Ok(()),
        });
        assert_eq!(s.status, "quiet");
        // Server create migrates the local shell to the server id.
        let _ = s.update(Message::CreatePlaylist("Trip".into()));
        let local = s.user_playlists.last().unwrap().id.clone();
        assert!(local.starts_with('u'));
        let _ = s.update(Message::ServerPlaylistCreated {
            local_id: local.clone(),
            title: "Trip".into(),
            result: Ok("PLnew1".into()),
        });
        assert!(s.user_playlists.iter().any(|p| p.id == "PLnew1"));
        assert!(!s.user_playlists.iter().any(|p| p.id == local));
        assert!(s.status.contains("synced to YouTube"));
        // Create failure keeps the local shell and says so.
        let _ = s.update(Message::CreatePlaylist("Local".into()));
        let local2 = s.user_playlists.last().unwrap().id.clone();
        let _ = s.update(Message::ServerPlaylistCreated {
            local_id: local2.clone(),
            title: "Local".into(),
            result: Err("500".into()),
        });
        assert!(s.user_playlists.iter().any(|p| p.id == local2));
        assert!(s.status.contains("locally"));
        // Unknown shell (deleted mid-flight): orphan reports, no panic.
        let _ = s.update(Message::ServerPlaylistCreated {
            local_id: "u999".into(),
            title: "Ghost".into(),
            result: Ok("PLghost1".into()),
        });
        assert!(s.status.contains("PLghost1"));
        assert!(!s.user_playlists.iter().any(|p| p.id == "PLghost1"));
        // Failed import never flips the session flag by itself.
        let _ = s.update(Message::ImportStageReady {
            kind: ImportKind::Cookie,
            result: Err("boom".into()),
        });
        assert!(s.status.contains("Cookie import failed: boom"));
    }

    #[test]
    fn eq_selection_reports_backend_honestly() {
        let mut s = test_state();
        let _ = s.update(Message::SelectEqPreset("Rock".into()));
        assert_eq!(s.eq_preset, "Rock");
        assert!(s.status.contains("EQ: Rock"));
    }

    #[test]
    fn now_playing_route_opens() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::Navigate(Route::NowPlaying));
        assert_eq!(s.route, Route::NowPlaying);
    }

    #[test]
    fn menu_pos_anchors_at_cursor_and_clamps() {
        let win = Some(iced::Size::new(1280.0, 800.0));
        // Free space: origin == cursor.
        assert_eq!(
            menu_pos(Some(iced::Point::new(100.0, 120.0)), win),
            (100.0, 120.0)
        );
        // Near the bottom-right: clamped so 300x510 stays visible.
        assert_eq!(
            menu_pos(Some(iced::Point::new(1200.0, 700.0)), win),
            (980.0, 290.0)
        );
        // Unknown cursor/window: sane on-screen default.
        assert_eq!(menu_pos(None, None), (200.0, 150.0));
    }

    #[test]
    fn autoplay_tail_shifts_in_when_queue_ends() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        assert_eq!(s.queue.len(), 1);
        // Manual end → head of the tail becomes the current song.
        let _ = s.update(Message::Next);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m4"));
        assert!(s.autoplay.is_empty());
        // Playing a tail item directly moves it into the manual queue.
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m5", "Other", "Z")]),
        });
        let _ = s.update(Message::PlayAutoplay(0));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m5"));
        assert!(s.autoplay.is_empty());
        let _ = s.update(Message::PlayAutoplay(9));
        assert!(s.status.contains("Nothing at autoplay"));
    }

    #[test]
    fn queue_remove_advances_when_current_goes() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        // Upcoming instance goes; current untouched.
        let _ = s.update(Message::RemoveFromQueue("m2".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
        assert!(s.status.contains("Removed m2"));
        // Removing the current song moves playback on.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::RemoveFromQueue("m1".into()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        // Absent id: honest error.
        let _ = s.update(Message::RemoveFromQueue("ghost".into()));
        assert!(s.status.contains("Not in queue"));
        // Tail removal by index.
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        let _ = s.update(Message::RemoveAutoplay(0));
        assert!(s.autoplay.is_empty());
        assert!(s.status.contains("autoplay"));
        let _ = s.update(Message::RemoveAutoplay(9));
        assert!(s.status.contains("Nothing at autoplay"));
    }

    #[test]
    fn bulk_queue_absorbs_matching_autoplay() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Fresh", "Y")]),
        });
        assert_eq!(s.autoplay.len(), 1);
        let _ = s.queue_many(vec![mk_track("m4", "Fresh", "Y")], "Radio");
        assert!(s.autoplay.is_empty());
        assert!(s.queue.items().iter().any(|t| t.id == "m4"));
    }

    #[test]
    fn drag_slot_commit_maps_and_previews() {
        // Identity: no slot, own slot, adjacent slot, out-of-range.
        assert_eq!(drag_slot_preview(5, 0, None), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 2, Some(2)), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 2, Some(3)), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 9, Some(1)), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 0, Some(9)), vec![0, 1, 2, 3, 4]);
        // Edge slots included: top and bottom moves commit.
        assert_eq!(drag_slot_preview(5, 4, Some(0)), vec![4, 0, 1, 2, 3]);
        assert_eq!(drag_slot_preview(5, 0, Some(5)), vec![1, 2, 3, 4, 0]);
        assert_eq!(drag_slot_preview(5, 4, Some(5)), vec![0, 1, 2, 3, 4]);
        // Interior moves still work.
        assert_eq!(drag_slot_preview(5, 0, Some(2)), vec![1, 0, 2, 3, 4]);
        assert_eq!(drag_slot_preview(4, 3, Some(1)), vec![0, 3, 1, 2]);
        assert_eq!(drag_slot_preview(5, 0, Some(1)), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 3, Some(4)), vec![0, 1, 2, 3, 4]);
        assert_eq!(drag_slot_preview(5, 4, Some(2)), vec![0, 1, 4, 2, 3]);
        assert_eq!(drag_slot_preview(5, 1, Some(4)), vec![0, 2, 3, 1, 4]);
        assert_eq!(drag_slot_preview(5, 0, Some(4)), vec![1, 2, 3, 0, 4]);
        // Middle of 3 always has a move (edges included).
        assert_eq!(drag_slot_preview(3, 1, Some(0)), vec![1, 0, 2]);
        assert_eq!(drag_slot_preview(3, 1, Some(3)), vec![0, 2, 1]);
        // Two-row lists reorder via the trailing edge slot.
        assert_eq!(drag_slot_preview(2, 0, Some(1)), vec![0, 1]);
        assert_eq!(drag_slot_preview(2, 0, Some(2)), vec![1, 0]);
    }

    #[test]
    fn sidebars_pin_and_reveal() {
        let mut s = test_state();
        assert!(s.sidebar_open());
        assert!(s.right_open());
        // Unpinned + cursor far away: both collapse.
        let _ = s.update(Message::ToggleSidebarPin);
        let _ = s.update(Message::ToggleRightPin);
        let _ = s.update(Message::CursorMoved(iced::Point::new(640.0, 400.0)));
        let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
        assert!(!s.sidebar_open());
        assert!(!s.right_open());
        // Hover the left edge: sidebar reveals.
        let _ = s.update(Message::CursorMoved(iced::Point::new(10.0, 400.0)));
        assert!(s.sidebar_open());
        // Hysteresis: it stays open while the cursor roams inside the
        // panel, and collapses only outside panel + grace.
        let _ = s.update(Message::CursorMoved(iced::Point::new(150.0, 400.0)));
        assert!(s.sidebar_open());
        let _ = s.update(Message::CursorMoved(iced::Point::new(400.0, 400.0)));
        assert!(!s.sidebar_open());
        // Same contract on the right edge.
        let _ = s.update(Message::CursorMoved(iced::Point::new(1270.0, 400.0)));
        assert!(s.right_open());
        let _ = s.update(Message::CursorMoved(iced::Point::new(1130.0, 400.0)));
        assert!(s.right_open());
        let _ = s.update(Message::CursorMoved(iced::Point::new(640.0, 400.0)));
        assert!(!s.right_open());
        // Expand zone starts outside the collapsed rail (ww - 64), not at
        // the screen edge.
        let _ = s.update(Message::CursorMoved(iced::Point::new(1210.0, 400.0)));
        assert!(!s.right_open());
        let _ = s.update(Message::CursorMoved(iced::Point::new(1225.0, 400.0)));
        assert!(s.right_open());
    }

    #[test]
    fn empty_collapsed_rail_reserves_nothing() {
        let mut s = test_state();
        assert!(s.right_rail_empty());
        let _ = s.update(Message::ToggleRightPin);
        let _ = s.update(Message::CursorMoved(iced::Point::new(640.0, 400.0)));
        let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
        assert_eq!(s.right_target(), 0.0);
        // One queued song brings the rail back while still collapsed.
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert!(!s.right_rail_empty());
        assert_eq!(s.right_target(), 56.0);
        assert!(!s.right_open());
    }

    #[test]
    fn replaying_the_live_song_is_a_noop() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        // Same song again: no restart, loading untouched.
        let _ = s.update(Message::PlayNow("m1".into()));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
        assert_eq!(s.queue.len(), 1);
        // Jumping to the live index is a no-op too.
        let _ = s.update(Message::QueueJump(0));
        assert_eq!(s.queue.position(), Some(0));
        assert_eq!(s.loading_track.as_deref(), Some("m1"));
    }

    #[test]
    fn drawer_toggles_and_eases() {
        let mut s = test_state();
        assert!(!s.drawer_open);
        assert_eq!(s.drawer_anim, 0.0);
        let _ = s.update(Message::ToggleDrawer);
        assert!(s.drawer_open);
        assert!(s.animating());
        for _ in 0..20 {
            let _ = s.update(Message::Animate);
        }
        assert!(!s.animating());
        assert_eq!(s.drawer_anim, 1.0);
        // Disabled animations snap mid-flight in a single tick.
        s.drawer_anim = 0.5;
        s.drawer_open = true;
        assert!(s.animating());
        let _ = s.update(Message::AnimationsToggled(false));
        let _ = s.update(Message::Animate);
        assert!(!s.animating());
        assert_eq!(s.drawer_anim, 1.0);
        // Esc closes the drawer when no menu is open.
        let _ = s.update(Message::CloseContext);
        assert!(!s.drawer_open);
    }

    #[test]
    fn drawer_open_fetches_lyrics() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        // Track start already fetches; the drawer keeps it in flight.
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        #[cfg(not(feature = "lyrics"))]
        assert!(s.lyrics_loading.is_none());
        let _ = s.update(Message::ToggleDrawer);
        assert!(s.drawer_open);
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        #[cfg(not(feature = "lyrics"))]
        assert!(s.lyrics_loading.is_none());
        // Re-open while loading does not double-fetch.
        let _ = s.update(Message::ToggleDrawer);
        let _ = s.update(Message::ToggleDrawer);
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
    }

    #[test]
    fn heart_icon_fills_when_liked() {
        const HEART: &str = include_str!("../assets/icons/heart.svg");
        assert!(HEART.contains("fill=\"none\""));
        let filled = HEART.replace("fill=\"none\"", "fill=\"currentColor\"");
        assert!(filled.contains("fill=\"currentColor\""));
        assert!(!filled.contains("fill=\"none\""));
        assert!(filled.contains("M19 14c1.49"));
    }

    #[test]
    fn lyrics_follow_every_track_start() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        #[cfg(feature = "lyrics")]
        assert_eq!(s.lyrics_loading.as_deref(), Some("m1"));
        #[cfg(not(feature = "lyrics"))]
        assert!(s.lyrics_loading.is_none());
        // Stale arrivals never clear a newer in-flight fetch.
        s.lyrics_loading = Some("m2".into());
        let _ = s.update(Message::LyricsReady {
            track_id: "m1".into(),
            result: Err("gone".into()),
        });
        assert_eq!(s.lyrics_loading.as_deref(), Some("m2"));
        let _ = s.update(Message::LyricsReady {
            track_id: "m2".into(),
            result: Err("gone".into()),
        });
        assert!(s.lyrics_loading.is_none());
    }

    #[test]
    fn lyric_click_highlights_exact_verse() {
        use yutu_core::LyricLine;
        // f32 routing would truncate 2345ms below the boundary; the ms
        // path plus the +1ms guard must land exactly on the verse.
        let ms = 2345_u64;
        let pos = ms as f64 / 1000.0;
        assert!((pos * 1000.0 - 2345.0).abs() < 0.001);
        let lines = vec![
            LyricLine {
                start_ms: 1000,
                text: "one".into(),
            },
            LyricLine {
                start_ms: 2345,
                text: "two".into(),
            },
            LyricLine {
                start_ms: 5000,
                text: "three".into(),
            },
        ];
        let pos_ms = (pos * 1000.0 + 1.0) as u64;
        assert_eq!(yutu_core::line_at(&lines, pos_ms), Some(1));
        // Stanza markers never steal the highlight: the visible index
        // after a marker still resolves to the next verse.
        let parsed = yutu_core::parse_lrc("[00:01.00]one\n\n[00:05.00]two\n");
        let visible: Vec<LyricLine> = parsed
            .iter()
            .filter(|l| !l.text.trim().is_empty())
            .cloned()
            .collect();
        assert_eq!(visible.len(), 2);
        assert_eq!(yutu_core::line_at(&visible, 5001), Some(1));
    }

    #[test]
    fn favorites_leads_and_maps_to_likes() {
        let mut s = test_state();
        assert_eq!(s.all_playlists()[0].id, "__favorites");
        assert!(s.all_playlists()[0].tracks.is_empty());
        let _ = s.update(Message::Like("m1".into()));
        assert_eq!(s.all_playlists()[0].tracks.len(), 1);
        assert!(s.playlist_has("__favorites", "m1"));
        // Flyout add maps to like, remove maps to unlike.
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "__favorites".into(),
            track_id: "m2".into(),
        });
        assert!(s.liked.contains("m2"));
        assert!(s.playlist_has("__favorites", "m2"));
        // Add order, not alphabetical: m1 liked before m2.
        let all = s.all_playlists();
        let fav_ids: Vec<&str> = all[0].tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(fav_ids, vec!["m1", "m2"]);
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: "__favorites".into(),
            track_id: "m2".into(),
        });
        assert!(s.status.contains("Already in Favorites"));
        let _ = s.update(Message::RemoveFromPlaylist {
            playlist_id: "__favorites".into(),
            track_id: "m2".into(),
        });
        assert!(!s.liked.contains("m2"));
        // Deleting Favorites is a no-op.
        let _ = s.update(Message::DeletePlaylist("__favorites".into()));
        assert!(s.user_playlists.is_empty());
        assert_eq!(s.all_playlists()[0].id, "__favorites");
        // Play Favorites queues exactly the resolved likes.
        let _ = s.update(Message::PlayPlaylist("__favorites".into()));
        assert_eq!(s.queue.len(), 1);
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    }

    #[test]
    fn single_queue_absorbs_matching_autoplay() {
        let mut s = test_state();
        let _ = s.update(Message::PlayNow("m1".into()));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m4", "Four", "Y")]),
        });
        assert_eq!(s.autoplay.len(), 1);
        let _ = s.update(Message::PlayNext("m4".into()));
        assert!(s.autoplay.iter().all(|t| t.id != "m4"));
        assert!(s.queue.items().iter().any(|t| t.id == "m4"));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m5", "Five", "Y")]),
        });
        let _ = s.update(Message::QueueTrack("m5".into()));
        assert!(s.autoplay.iter().all(|t| t.id != "m5"));
        let _ = s.update(Message::AutoplayReady {
            seed: "m1".into(),
            result: Ok(vec![mk_track("m6", "Six", "Y")]),
        });
        let _ = s.update(Message::EnqueueTrack("m6".into()));
        assert!(s.autoplay.iter().all(|t| t.id != "m6"));
    }

    #[test]
    fn shuffle_playlist_plays_shuffled_from_top() {
        let mut s = test_state();
        let _ = s.update(Message::CreatePlaylist("Gym".into()));
        let pid = s.user_playlists[0].id.clone();
        for id in ["m1", "m2", "m3", "m4"] {
            s.user_playlists[0].tracks.push(Track {
                id: id.into(),
                title: id.into(),
                artist: "A".into(),
                album: None,
                duration_secs: 1,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            });
        }
        let _ = s.update(Message::ShufflePlaylist(pid.clone()));
        assert_eq!(s.queue.len(), 4);
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.position(), Some(0));
        // Same path as the page pill: the fetch targets the shuffled
        // head and the snapshot is the playlist order.
        let head = s.queue.current().unwrap().id.clone();
        assert_eq!(s.loading_track.as_deref(), Some(head.as_str()));
        let ordered: Vec<String> = ["m1", "m2", "m3", "m4"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        assert_eq!(s.pre_shuffle, Some(ordered.clone()));
        // Shuffle-play while already on reshuffles (no early-return).
        let _ = s.update(Message::ShufflePlaylist(pid));
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 4);
        assert_eq!(s.pre_shuffle, Some(ordered));
    }

    #[test]
    fn album_ax_id_resolves_tracks() {
        let mut s = test_state();
        s.catalog.insert(
            "t1".into(),
            Track {
                id: "t1".into(),
                title: "One".into(),
                artist: "A".into(),
                album: Some("Test Album XYZ".into()),
                duration_secs: 1,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
        );
        s.catalog.insert(
            "t2".into(),
            Track {
                id: "t2".into(),
                title: "Two".into(),
                artist: "A".into(),
                album: Some("Test Album XYZ".into()),
                duration_secs: 1,
                thumbnail_url: None,
                thumbnail_large_url: None,
                plays: None,
            },
        );
        assert_eq!(s.resolve_album_title("ax-Test Album XYZ"), "Test Album XYZ");
        assert_eq!(s.album_tracks("ax-Test Album XYZ").len(), 2);
    }

    #[test]
    fn hero_uses_catalog_art_when_row_is_stale() {
        let mut s = test_state();
        s.catalog.insert(
            "m1".into(),
            Track {
                id: "m1".into(),
                title: "M1".into(),
                artist: "A".into(),
                album: None,
                duration_secs: 1,
                thumbnail_url: Some("https://example.invalid/s".into()),
                thumbnail_large_url: None,
                plays: None,
            },
        );
        let stale = Track {
            id: "m1".into(),
            title: "M1".into(),
            artist: "A".into(),
            album: None,
            duration_secs: 1,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
        };
        assert!(s.hero_art_for(&stale).is_some());
    }

    #[test]
    fn clear_hovers_resets_all() {
        let mut s = test_state();
        let _ = s.update(Message::TrackHover(Some("m1".into())));
        let _ = s.update(Message::PlaylistHover(Some("u1".into())));
        let _ = s.update(Message::QueueHover(Some(2)));
        // Queue and autoplay hovers are mutually exclusive by design
        // (entering one list drops the other, so presses never arm off
        // stale state).
        assert!(s.hovered_queue.is_some());
        assert!(s.hovered_auto.is_none());
        let _ = s.update(Message::AutoHover(Some(1)));
        assert!(s.hovered_track.is_some());
        assert!(s.hovered_playlist.is_some());
        assert!(s.hovered_queue.is_none());
        assert!(s.hovered_auto.is_some());
        let _ = s.update(Message::ClearHovers);
        assert!(s.hovered_track.is_none());
        assert!(s.hovered_playlist.is_none());
        assert!(s.hovered_queue.is_none());
        assert!(s.hovered_auto.is_none());
        assert!(s.hover_remove.is_none());
    }

    #[test]
    fn cursor_in_player_zone_never_expands() {
        let mut s = test_state();
        let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
        let _ = s.update(Message::ToggleSidebarPin);
        let _ = s.update(Message::ToggleRightPin);
        let _ = s.update(Message::CursorMoved(iced::Point::new(10.0, 780.0)));
        assert!(!s.sidebar_latched);
        let _ = s.update(Message::CursorMoved(iced::Point::new(10.0, 100.0)));
        assert!(s.sidebar_latched);
        let _ = s.update(Message::CursorMoved(iced::Point::new(1270.0, 780.0)));
        assert!(!s.right_latched);
        let _ = s.update(Message::CursorMoved(iced::Point::new(1270.0, 100.0)));
        assert!(s.right_latched);
    }

    #[test]
    fn search_geometry_tracks_sidebar() {
        let mut s = test_state();
        let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
        s.sidebar_w = 56.0;
        let left = s.search_pill_left();
        assert!((left - 164.0).abs() < 1.0);
        let w = s.search_width();
        assert!((w - 537.6).abs() < 1.0);
        let pop = left.clamp(0.0, (1280.0 - w).max(0.0));
        assert!(pop + w <= 1280.0);
        // Wide window: popup sits exactly at the pill left edge.
        assert_eq!(pop, left);
    }

    #[test]
    fn clean_release_clicks_the_row() {
        // Press + release without moving over the same unmutated row
        // plays it (the whole row is one press path now).
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::EnqueueTrack("m3".into()));
        assert_eq!(s.queue.position(), Some(2));
        let at = iced::Point::new(10.0, 10.0);
        let _ = s.update(Message::CursorMoved(at));
        assert_eq!(s.cursor, Some(at));
        let _ = s.update(Message::PanelHover(0));
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.position(), Some(0));
        // Order untouched by a click.
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        // Tiny tremor inside slop still clicks.
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::CursorMoved(iced::Point::new(20.0, 20.0)));
        let _ = s.update(Message::PanelHover(2));
        let _ = s.update(Message::DragStart(2));
        let _ = s.update(Message::CursorMoved(iced::Point::new(22.0, 21.0)));
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.position(), Some(2));
        // Menu open at release: no click behind the menu.
        let _ = s.update(Message::CursorMoved(iced::Point::new(30.0, 30.0)));
        let _ = s.update(Message::PanelHover(1));
        let _ = s.update(Message::DragStart(1));
        let _ = s.update(Message::OpenContext("m2".into()));
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.position(), Some(2));
    }

    #[test]
    fn click_suppressed_after_mutation_or_move() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::EnqueueTrack("m3".into()));
        // Removal mutates the queue: no jump.
        let _ = s.update(Message::PanelHover(0));
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::RemoveFromQueue("m1".into()));
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.position(), Some(1));
        // Cursor moved away: no jump.
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::PanelHover(0));
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::CursorMoved(iced::Point::new(600.0, 100.0)));
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.position(), Some(1));
    }

    #[test]
    fn press_on_remove_never_arms_drag() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::PanelHover(0));
        let _ = s.update(Message::RemoveHover(Some(0)));
        let _ = s.update(Message::DragStart(0));
        assert!(s.drag.is_none());
        let _ = s.update(Message::GlobalPress);
        assert!(s.drag.is_none());
        let _ = s.update(Message::RemoveHover(None));
        let _ = s.update(Message::GlobalPress);
        assert!(s.drag.is_some());
    }

    #[test]
    fn list_titles_cap_at_two_lines() {
        assert_eq!(truncate_title("Short", 55), "Short");
        assert_eq!(truncate_title("", 55), "");
        let exact: String = "x".repeat(55);
        assert_eq!(truncate_title(&exact, 55), exact);
        let over: String = "y".repeat(57);
        let cut = truncate_title(&over, 55);
        assert!(cut.ends_with("…"));
        assert_eq!(cut.chars().count(), 56);
        let uni = "é".repeat(60);
        let ucut = truncate_title(&uni, 55);
        assert_eq!(ucut.chars().count(), 56);
        // Drawer and player bypass track_text (full titles kept): they
        // call text(t.title) directly, never truncate_title.
    }

    #[test]
    fn player_layout_breakpoints() {
        assert_eq!(player_layout_mode(499.0), PlayerLayout::Minimal);
        assert_eq!(player_layout_mode(500.0), PlayerLayout::Compact);
        assert_eq!(player_layout_mode(699.0), PlayerLayout::Compact);
        assert_eq!(player_layout_mode(700.0), PlayerLayout::Wide);
        assert_eq!(player_layout_mode(1280.0), PlayerLayout::Wide);
        let mut s = test_state();
        let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
        assert!((s.player_zone_h() - PLAYER_ZONE_H).abs() < 0.01);
        let _ = s.update(Message::WindowResized(iced::Size::new(400.0, 800.0)));
        assert!(s.player_zone_h() > PLAYER_ZONE_H);
        assert_eq!(s.player_zone_h(), PLAYER_ZONE_MINIMAL_H);
        let _ = s.update(Message::WindowResized(iced::Size::new(650.0, 800.0)));
        assert_eq!(s.player_zone_h(), PLAYER_ZONE_H);
    }

    #[test]
    fn player_view_builds_at_all_widths() {
        let mut s = test_state();
        for w in [1280.0, 650.0, 400.0] {
            let _ = s.update(Message::WindowResized(iced::Size::new(w, 800.0)));
            let _ = s.view();
        }
        // Minimal keeps shuffle state visible (no silent restyle).
        let mut s = test_state();
        let _ = s.update(Message::WindowResized(iced::Size::new(400.0, 800.0)));
        let _ = s.update(Message::ToggleShuffle);
        assert!(s.queue.shuffle);
        let _ = s.view();
    }

    #[test]
    fn mouse_backdismisses_menu_before_navigating() {
        let mut s = test_state();
        s.navigate(Route::Search);
        s.navigate(Route::Queue);
        let _ = s.update(Message::OpenContext("m1".into()));
        assert!(s.context_menu.is_some());
        let _ = s.update(Message::NavBack);
        assert!(s.context_menu.is_none());
        assert_eq!(s.route, Route::Queue);
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Search);
    }

    #[test]
    fn mouse_back_ignores_drag() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        s.navigate(Route::Search);
        let _ = s.update(Message::DragStart(0));
        assert!(s.drag.is_some());
        let _ = s.update(Message::NavBack);
        assert_eq!(s.route, Route::Search);
        assert!(s.drag.is_some());
    }

    #[test]
    fn art_url_threshold_picks_hero_above_64() {
        let mut s = test_state();
        let t = Track {
            id: "m1".into(),
            title: "M1".into(),
            artist: "A".into(),
            album: None,
            duration_secs: 1,
            thumbnail_url: Some("https://example.invalid/s".into()),
            thumbnail_large_url: None,
            plays: None,
        };
        s.catalog.insert("m1".into(), t.clone());
        let small = s.art_url_for(&t, 40.0).expect("small");
        assert_eq!(small, "https://example.invalid/s");
        // example.invalid passes hires through unchanged, so assert via
        // a YTM-style URL for the hero rewrite.
        let ytm = Track {
            thumbnail_url: Some("https://x=w60-h60-l90-rj".into()),
            ..t.clone()
        };
        s.catalog.insert("m1".into(), ytm.clone());
        let hero = s.art_url_for(&ytm, 96.0).expect("hero");
        assert!(hero.contains("=w544-h544"), "{hero}");
    }

    #[test]
    fn hero_url_derives_hires() {
        let t = Track {
            id: "v".into(),
            title: "T".into(),
            artist: "A".into(),
            album: None,
            duration_secs: 1,
            thumbnail_url: Some("https://yt3.googleusercontent.com/x=w60-h60-l90-rj".into()),
            thumbnail_large_url: Some(
                "https://yt3.googleusercontent.com/x=w120-h120-l90-rj".into(),
            ),
            plays: None,
        };
        let url = AppState::hero_art_url(&t).expect("hero");
        assert!(url.contains("=w544-h544"), "{url}");
    }

    #[test]
    fn animation_prefs_clamp_and_default() {
        let s = test_state();
        assert!(s.animations_enabled);
        assert_eq!(s.anim_ms, ANIM_DEFAULT_MS);
        let mut s = s;
        let _ = s.update(Message::AnimMsChanged(9999.0));
        assert_eq!(s.anim_ms, ANIM_MAX_MS);
        let _ = s.update(Message::AnimMsChanged(0.0));
        assert_eq!(s.anim_ms, ANIM_MIN_MS);
    }

    #[test]
    fn anim_frames_math() {
        // 100ms at a 16ms tick: 6 frames, ~30px sidebar steps.
        let (sidebar, right, unit) = anim_steps(100);
        assert!((sidebar - 29.8).abs() < 0.2);
        assert!((right - 47.5).abs() < 0.2);
        assert!((unit - 1.0 / 6.0).abs() < 0.001);
        // 1px floor binds only for tiny distances, never for glides.
        let (small, _, _) = anim_steps(400);
        assert!(small > 1.0);
    }

    #[test]
    fn search_cache_hit_miss_and_normalization() {
        let mut s = test_state();
        assert!(s.search_cache_get("beatles").is_none());
        s.search_cache_put(
            "  Beatles ",
            bundle_of(SearchResults {
                tracks: vec![mk_track("v1", "Hey Jude", "The Beatles")],
                ..Default::default()
            }),
        );
        let hit = s.search_cache_get("beatles").expect("normalized hit");
        assert_eq!(hit.filtered.tracks.len(), 1);
        assert_eq!(hit.filtered.tracks[0].id, "v1");
        // TTL expiry reads as a miss.
        if let Some((at, _)) = s.search_cache.get_mut("beatles") {
            *at = std::time::Instant::now() - std::time::Duration::from_secs(3600);
        }
        assert!(s.search_cache_get("beatles").is_none());
    }

    #[test]
    fn status_text_is_full_log_in_debug() {
        let mut s = test_state();
        s.status = "Loading x".into();
        assert_eq!(s.status_text(), "Loading x");
    }

    #[test]
    fn panel_drag_reorders_then_releases() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::EnqueueTrack("m3".into()));
        assert!(s.drag.is_none());
        // Release with no drag: no-op.
        let _ = s.update(Message::DragEnd);
        assert_eq!(s.queue.items()[0].id, "m1");
        // Hover without drag: no slot latched.
        let _ = s.update(Message::DragSlot(2));
        assert!(s.drag.is_none());
        // Row press → slot hover → release elsewhere reorders.
        let _ = s.update(Message::DragStart(0));
        assert!(s.drag.is_some());
        let _ = s.update(Message::DragSlot(2));
        let _ = s.update(Message::DragEnd);
        assert!(s.drag.is_none());
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m2", "m1", "m3"]);
        assert!(s.status.contains("reordered"));
        // Own-spot edge slot: no move, no gap.
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::DragSlot(0));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m2", "m1", "m3"]);
        assert_eq!(s.drag_gap, 0.0);
        // Middle of 3 moves via edges: to front, then to back.
        let _ = s.update(Message::DragStart(1));
        let _ = s.update(Message::DragSlot(0));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        let _ = s.update(Message::DragStart(1));
        let _ = s.update(Message::DragSlot(3));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m3", "m2"]);
        // Out-of-range slot ignored.
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::DragSlot(9));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m3", "m2"]);
        // Between-positions: last item lands between the first two.
        let _ = s.update(Message::DragStart(2));
        let _ = s.update(Message::DragSlot(1));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        // Same/adjacent slot: no move, no noise.
        let _ = s.update(Message::DragStart(1));
        let _ = s.update(Message::DragSlot(2));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        // Esc cancels the drag without touching the order.
        let _ = s.update(Message::DragStart(0));
        let _ = s.update(Message::DragSlot(1));
        let _ = s.update(Message::CloseContext);
        assert!(s.drag.is_none());
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        // Press + release on the same row: no move, no noise.
        let _ = s.update(Message::DragStart(1));
        let _ = s.update(Message::DragEnd);
        let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["m1", "m2", "m3"]);
        // Out-of-range press ignored.
        let _ = s.update(Message::DragStart(99));
        assert!(s.drag.is_none());
    }

    #[test]
    fn global_press_arms_drag_from_hovered_row() {
        // Text/art presses never reach the row area (inner buttons own
        // them), so the global press covers the whole row via hover.
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::EnqueueTrack("m3".into()));
        let _ = s.update(Message::GlobalPress);
        assert!(s.drag.is_none());
        let _ = s.update(Message::PanelHover(1));
        let _ = s.update(Message::GlobalPress);
        assert_eq!(s.drag.map(|d| d.from), Some(1));
        // A normal click still works after: release with no slot is a
        // no-op, then the row button jumps.
        let _ = s.update(Message::DragEnd);
        assert!(s.drag.is_none());
        let _ = s.update(Message::QueueJump(1));
        assert_eq!(s.queue.position(), Some(1));
    }

    #[test]
    fn queue_hover_tracks_cursor() {
        let mut s = test_state();
        assert!(s.hovered_queue.is_none());
        let _ = s.update(Message::QueueHover(Some(2)));
        assert_eq!(s.hovered_queue, Some(2));
        let _ = s.update(Message::QueueHover(None));
        assert!(s.hovered_queue.is_none());
        // Id-keyed row hover for non-index lists.
        assert!(s.hovered_track.is_none());
        let _ = s.update(Message::TrackHover(Some("m1".into())));
        assert_eq!(s.hovered_track.as_deref(), Some("m1"));
        let _ = s.update(Message::TrackHover(None));
        assert!(s.hovered_track.is_none());
    }

    #[test]
    fn desktop_shuffle_and_loop_apply() {
        use std::sync::{Arc, Mutex};
        let mut s = test_state();
        let handle: yutu_desktop::SharedPlayer =
            Arc::new(Mutex::new(yutu_desktop::MprisPlayer::default()));
        s.mpris = Some(handle.clone());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        // Widget shuffle flips the app order + publishes it back.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::SetShuffle(true));
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!(s.queue.shuffle);
        assert!(s.pre_shuffle.is_some());
        assert!(handle.lock().unwrap().shuffle());
        // Widget loop lands on the repeat mode + publishes back.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::SetLoop(
                yutu_desktop::LoopStatus::Track,
            ));
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert_eq!(s.queue.repeat, yutu_player::RepeatMode::One);
        assert_eq!(
            handle.lock().unwrap().loop_status(),
            yutu_desktop::LoopStatus::Track
        );
    }

    #[test]
    fn desktop_commands_drive_playback() {
        use std::sync::{Arc, Mutex};
        let mut s = test_state();
        let handle: yutu_desktop::SharedPlayer =
            Arc::new(Mutex::new(yutu_desktop::MprisPlayer::default()));
        s.mpris = Some(handle.clone());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        // Widget pause flips the app within one tick.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::Pause);
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!(s.paused);
        assert!(s.status.contains("desktop"));
        // Widget next advances the real queue.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::Next);
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
        // Widget volume lands too.
        handle
            .lock()
            .unwrap()
            .push_command(yutu_desktop::PlayerCommand::SetVolume(0.5));
        let _ = s.update(Message::Tick(std::time::Instant::now()));
        assert!((s.volume - 0.5).abs() < 0.001);
    }

    #[test]
    fn seek_hold_advances_manually_then_resyncs() {
        use std::time::{Duration, Instant};
        let mut s = test_state();
        s.position_secs = 10.0;
        let t0 = Instant::now();
        s.last_tick = Some(t0);
        s.seek_hold_until = Some(t0 + Duration::from_secs(5));
        let _ = s.update(Message::Tick(t0 + Duration::from_millis(500)));
        assert!((s.position_secs - 10.5).abs() < 0.05);
        // Expired hold: backend clock wins again.
        s.seek_hold_until = Some(t0);
        s.position_secs = 99.0;
        let _ = s.update(Message::Tick(t0 + Duration::from_secs(10)));
        assert!(s.seek_hold_until.is_none());
        assert!(s.position_secs < 50.0);
    }

    #[test]
    fn playback_failures_land_in_the_error_log() {
        let mut s = test_state();
        assert!(s.error_log.is_empty());
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::StreamResolved {
            video_id: "m1".into(),
            result: Err("403 Forbidden".into()),
        });
        assert_eq!(s.error_log.len(), 1);
        assert!(s.error_log[0].contains("m1"));
    }

    #[test]
    fn context_menu_freezes_position_while_open() {
        let mut s = test_state();
        let _ = s.update(Message::CursorMoved(iced::Point::new(100.0, 120.0)));
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.menu_at, Some(iced::Point::new(100.0, 120.0)));
        // Mouse keeps moving (aiming at the menu): the anchor holds.
        let _ = s.update(Message::CursorMoved(iced::Point::new(900.0, 700.0)));
        assert_eq!(s.menu_at, Some(iced::Point::new(100.0, 120.0)));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
        // Closing drops the anchor.
        let _ = s.update(Message::CloseContext);
        assert!(s.menu_at.is_none());
        assert!(s.context_menu.is_none());
    }

    #[test]
    fn cursor_and_window_tracking_store() {
        let mut s = test_state();
        assert!(s.cursor.is_none());
        let _ = s.update(Message::CursorMoved(iced::Point::new(11.0, 22.0)));
        assert_eq!(s.cursor, Some(iced::Point::new(11.0, 22.0)));
        let _ = s.update(Message::WindowResized(iced::Size::new(1600.0, 900.0)));
        assert_eq!(s.window, Some(iced::Size::new(1600.0, 900.0)));
        // Right-click opens the menu for the row's track.
        let _ = s.update(Message::OpenContext("m1".into()));
        assert_eq!(s.context_menu.as_deref(), Some("m1"));
    }

    #[test]
    fn lyrics_page_follows_track_changes() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::OpenLyrics("m1".into()));
        assert_eq!(s.route, Route::Lyrics("m1".into()));
        // Advancing while on the lyrics page swaps the id in place.
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        assert_eq!(s.route, Route::Lyrics("m2".into()));
        // No back-stack spam from the follow.
        assert!(!s.nav_back.iter().any(|r| *r == Route::Lyrics("m1".into())));
    }

    #[test]
    fn peek_next_id_walks_and_wraps_on_repeat_all() {
        let mut s = test_state();
        assert_eq!(s.peek_next_id(), None);
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        // Current is m2 (last enqueued); nothing after it, no repeat.
        assert_eq!(s.peek_next_id(), None);
        let _ = s.update(Message::QueueJump(0));
        assert_eq!(s.peek_next_id().as_deref(), Some("m2"));
        // Repeat-all wraps the tail back to the head.
        let _ = s.update(Message::CycleRepeat);
        let _ = s.update(Message::QueueJump(1));
        assert_eq!(s.peek_next_id().as_deref(), Some("m1"));
    }

    #[test]
    fn prefetch_is_kept_only_for_the_upcoming_track() {
        let mut s = test_state();
        let _ = s.update(Message::EnqueueTrack("m1".into()));
        let _ = s.update(Message::EnqueueTrack("m2".into()));
        let _ = s.update(Message::QueueJump(0));
        let _ = s.update(Message::UrlPrefetched {
            track_id: "m2".into(),
            result: Ok("https://example.invalid/m2".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
        // A late arrival for any other track is dropped.
        let _ = s.update(Message::UrlPrefetched {
            track_id: "zzz".into(),
            result: Ok("https://example.invalid/z".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
        // Failures never poison the slot.
        let _ = s.update(Message::UrlPrefetched {
            track_id: "m2".into(),
            result: Err("boom".into()),
        });
        assert_eq!(
            s.prefetched,
            Some(("m2".into(), "https://example.invalid/m2".into()))
        );
    }

    /// Live composition check: the UI's `live_search` over the real API.
    #[cfg(feature = "innertube")]
    #[tokio::test]
    #[ignore = "live network: UI search composition over core Innertube queries"]
    async fn live_ui_search_returns_tracks() {
        let r = super::live_search("Beatles").await.expect("live UI search");
        assert!(!r.all.tracks.is_empty(), "expected real tracks for Beatles");
        assert!(
            r.all
                .tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry playable ids + titles"
        );
    }
}
