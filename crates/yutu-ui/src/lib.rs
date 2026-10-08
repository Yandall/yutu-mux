//! yutu-ui: native Iced app: real YouTube Music data only, no mock fixtures.

use std::collections::{HashMap, HashSet};
use yutu_core::{
    forgotten_favorites, should_scrobble, weekly_mix, Album, Artist, HistoryEntry, LibraryStore,
    LyricsResult, Playlist, SearchResults, Track,
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
        result: Result<SearchResults, String>,
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
    CookieImported {
        result: Result<(Vec<Track>, Vec<String>), String>,
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
    #[cfg(feature = "innertube")]
    LibraryImported {
        result: Result<(Vec<Track>, Vec<String>), String>,
    },
    StreamResolved {
        video_id: String,
        result: Result<Vec<u8>, String>,
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

/// Context-menu origin from the last cursor position, clamped so the
/// 300px menu never runs off screen. Falls back to a sane default when
/// the cursor/window are still unknown (e.g. keyboard-opened).
fn menu_pos(cursor: Option<iced::Point>, window: Option<iced::Size>) -> (f32, f32) {
    const MENU_W: f32 = 300.0;
    const MENU_H: f32 = 470.0;
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

pub struct AppState {
    pub route: Route,
    pub search_query: String,
    pub results: SearchResults,
    pub queue: Queue,
    pub liked: HashSet<String>,
    /// Liked ids in add order (membership stays in `liked`).
    pub liked_order: Vec<String>,
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
    /// Context menu open for this track id (None = closed).
    context_menu: Option<String>,
    /// Playlist menu open for this playlist id (None = closed).
    pl_menu: Option<String>,
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
    /// Track id a prefetch was already attempted for (no Tick spam).
    prefetch_attempted: Option<String>,
    /// Google device-flow login in progress (cloned into poll tasks).
    #[cfg(feature = "innertube")]
    google_login: Option<yutu_core::GoogleLogin>,
    /// A saved OAuth token exists on disk (signed in).
    #[cfg(feature = "innertube")]
    logged_in: bool,
    /// A login/library network task is in flight.
    #[cfg(feature = "innertube")]
    login_busy: bool,
    /// End-of-track auto-advance already fired for the current song.
    end_advanced: bool,
    library_path: String,
    /// Real-track catalog: every track the app has seen (search, radio,
    /// playlists). ID lookups resolve here: never fixtures.
    catalog: HashMap<String, Track>,
    /// Last complete real search (local filtering base for `SearchChanged`).
    search_base: SearchResults,
    /// Query cache: normalized query to (arrival time, results). Repeat
    /// searches (backspace/retype, Enter-after-popup) skip the network.
    search_cache: HashMap<String, (std::time::Instant, SearchResults)>,
    /// A real search is in flight (loading indicator).
    searching: bool,
    /// Keystroke generation: each input bumps it; only the latest
    /// debounced task may fire (no search spam while typing).
    search_gen: u64,
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
    fn search_cache_get(&self, query: &str) -> Option<SearchResults> {
        let (at, results) = self.search_cache.get(&search_cache_key(query))?;
        if at.elapsed().as_secs() > SEARCH_CACHE_TTL_SECS {
            return None;
        }
        Some(results.clone())
    }

    /// Store results under the normalized query (evict the oldest when
    /// full, never the whole cache).
    fn search_cache_put(&mut self, query: &str, results: SearchResults) {
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
        self.search_cache.insert(
            search_cache_key(query),
            (std::time::Instant::now(), results),
        );
    }

    /// Cache-first search: paint cached results with no network task,
    /// else None so the caller fires live (or reports offline).
    fn search_cache_hit_task(&mut self, q: &str) -> Option<iced::Task<Message>> {
        let hit = self.search_cache_get(q)?;
        self.searching = false;
        Some(self.apply_search_hit(q, hit))
    }

    /// Register tracks and queue artwork (no painting). Shared by live
    /// completions (fresh or stale) and cache hits.
    fn stage_search(&mut self, search: SearchResults) -> (SearchResults, Vec<iced::Task<Message>>) {
        let tracks = self.register_tracks(search.tracks);
        let search = SearchResults { tracks, ..search };
        let art_urls: Vec<String> = search
            .albums
            .iter()
            .filter_map(|a| a.thumbnail_url.clone())
            .chain(
                search
                    .artists
                    .iter()
                    .filter_map(|a| a.thumbnail_url.clone()),
            )
            // Hi-res keys: display sites show the 544px rewrite and
            // art_thumb is an exact-key lookup, so warm what is shown.
            .map(|u| yutu_core::hires_thumb_url(&u, 544))
            .collect();
        let mut tasks = self.artwork_tasks(&search.tracks);
        tasks.extend(self.artwork_tasks_for_urls(&art_urls));
        (search, tasks)
    }

    /// Paint results into results/base/status/artwork (shared by live
    /// completions and cache hits). Returns the artwork batch.
    fn apply_search_hit(&mut self, query: &str, search: SearchResults) -> iced::Task<Message> {
        let n = search.tracks.len();
        let (search, tasks) = self.stage_search(search);
        self.search_base = search.clone();
        self.results = filter_results(&search, &self.search_query);
        if self.results.tracks.is_empty() && !search.tracks.is_empty() {
            self.results = search;
        }
        self.status = format!("{n} songs for “{query}”.");
        self.publish_mpris();
        iced::Task::batch(tasks)
    }

    /// Merge a stored row with its catalog entry: catalog wins, but
    /// art the catalog lacks is filled from the stored row (and healed
    /// back into the catalog). Queues then own full Tracks even for
    /// pre-migration rows.
    fn resolve_for_play(&mut self, t: &Track) -> Track {
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
    fn register_tracks(&mut self, tracks: Vec<Track>) -> Vec<Track> {
        for t in &tracks {
            self.catalog.insert(t.id.clone(), t.clone());
        }
        tracks
    }

    /// Merge an imported library: catalog + likes (memory + SQLite) +
    /// artwork tasks for the new tracks.
    fn merge_imported(
        &mut self,
        tracks: Vec<Track>,
        liked_ids: Vec<String>,
    ) -> Vec<iced::Task<Message>> {
        let tracks = self.register_tracks(tracks);
        for id in &liked_ids {
            self.like_id(id);
        }
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            for id in &liked_ids {
                let _ = store.like(id);
            }
        }
        self.artwork_tasks(&tracks)
    }

    /// Everything the app has really seen (discovery candidate pool).
    fn catalog_tracks(&self) -> Vec<Track> {
        let mut v: Vec<Track> = self.catalog.values().cloned().collect();
        v.sort_by(|a, b| a.title.cmp(&b.title));
        v
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

/// Real search over the live Innertube API: tracks, albums and artists.
/// Each category fails open (empty vec) so one bad query cannot sink the
/// other two; only a failed *connection* or a triple failure errors.
#[cfg(feature = "innertube")]
async fn live_search(query: &str) -> Result<SearchResults, String> {
    use yutu_core::InnertubeBackend;
    let backend = InnertubeBackend::connect_anonymous().await?;
    let tracks = backend.search_tracks(query).await;
    let albums = backend.search_albums(query).await;
    let artists = backend.search_artists(query).await;
    match (tracks, albums, artists) {
        (Err(e), Err(_), Err(_)) => Err(e),
        (tracks, albums, artists) => Ok(SearchResults {
            tracks: tracks.unwrap_or_default(),
            albums: albums.unwrap_or_default(),
            artists: artists.unwrap_or_default(),
        }),
    }
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

/// Resolve a video ID to bytes. yt-dlp goes first (maintained decipher, no
/// feature gate, license-clean); then the raw WEB_REMIX probes with sidecar
/// mint + retry on PO-gating. A bare-URL 403 retries once with the session
/// token as `&pot=`. The yt-dlp leg needs no feature; the raw probes do.
async fn resolve_and_fetch(video_id: &str) -> Result<Vec<u8>, String> {
    if let Some(bin) = yutu_core::find_yt_dlp() {
        match yutu_core::resolve_audio_url(&bin, video_id) {
            Ok(url) => {
                return yutu_player::fetch_bytes(&url)
                    .await
                    .map_err(|e| e.to_string());
            }
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

/// Saved Google OAuth token bundle (0600, written by the login flow).
#[cfg(feature = "innertube")]
pub fn oauth_path() -> String {
    format!("{}/youtube_oauth.json", data_dir())
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

fn load_persisted(
    path: &str,
) -> (
    HashSet<String>,
    Vec<String>,
    Vec<String>,
    Vec<Playlist>,
    u64,
) {
    let empty = (HashSet::new(), Vec::new(), Vec::new(), Vec::new(), 0);
    let store = match LibraryStore::open(path) {
        Ok(s) => s,
        Err(_) => return empty,
    };
    // Likes: every id in SQLite in add order, no catalog probe.
    let order: Vec<String> = store.liked_ids().unwrap_or_default();
    let liked: HashSet<String> = order.iter().cloned().collect();
    let history = store
        .history(50)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.track_id)
        .collect();
    let user_playlists = store.user_playlists().unwrap_or_default();
    let seq = user_playlists
        .iter()
        .filter_map(|p| p.id.strip_prefix('u')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    (liked, order, history, user_playlists, seq)
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
        let (liked, liked_order, history, user_playlists, playlist_seq) =
            load_persisted(&library_path);
        let mut state = Self {
            route: Route::Home,
            search_query: String::new(),
            results: SearchResults::default(),
            queue: Queue::default(),
            liked,
            liked_order,
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
            context_menu: None,
            submenu_playlists_for: None,
            pl_menu: None,
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
            sidebar_filter: String::new(),
            autoplay_for: None,
            autoplay: Vec::new(),
            hovered_queue: None,
            hover_remove: None,
            hovered_track: None,
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
            prefetch_attempted: None,
            #[cfg(feature = "innertube")]
            google_login: None,
            #[cfg(feature = "innertube")]
            logged_in: oauth_path_exists(),
            #[cfg(feature = "innertube")]
            login_busy: false,
            end_advanced: false,
            library_path,
            catalog: HashMap::new(),
            search_base: SearchResults::default(),
            search_cache: HashMap::new(),
            searching: false,
            search_gen: 0,
            radio_tracks: Vec::new(),
            mpris: None,
        };
        // Seed the catalog from persisted playlists so playlist songs
        // play on first launch (no search-first needed), and warm art.
        let seed: Vec<Track> = state
            .user_playlists
            .iter()
            .flat_map(|p| p.tracks.clone())
            .collect();
        let seeded = state.register_tracks(seed);
        let warm = state.artwork_tasks(&seeded);
        (state, iced::Task::batch(warm))
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

    fn persist_like(&self, id: &str, liked: bool) {
        let Ok(store) = LibraryStore::open(&self.library_path) else {
            return;
        };
        let _ = if liked {
            store.like(id)
        } else {
            store.unlike(id)
        };
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
        if self.artwork.contains_key(&url) || self.artwork_loading.contains(&url) {
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
                    true
                }
                Err(e) => {
                    let msg = format!("Playback failed for {}: {e}", t.id);
                    self.log_error(msg.clone());
                    self.status = msg;
                    false
                }
            },
            None => {
                self.status = "Stream ready, but no audio device.".into();
                false
            }
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
                self.lyrics_loading = Some(t.id.clone());
                tasks.push(Self::lyrics_task(t.clone()));
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
                let mut tasks = self.track_start_tasks(&t);
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

    /// Spotify-style numbered row: rank + art + play + title + time + actions.
    fn numbered_track_row(&self, t: &Track, n: usize) -> iced::Element<'_, Message> {
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
                    text(format!("{n}")).size(13).color(MUTED).width(28),
                    self.thumb(t),
                    button(icon("play", 13.0, iced::Color::WHITE))
                        .style(green_play)
                        .on_press(Message::PlayNow(t.id.clone()))
                        .padding(6),
                    Self::track_text(&t.title, &t.artist, 14, iced::Color::WHITE),
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
            .width(Fill),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
        .on_enter(Message::TrackHover(Some(hover_id)))
        .on_exit(Message::TrackHover(None))
        .into()
    }

    /// Album card with cover art (search grids, artist pages).
    fn album_card(&self, a: &Album) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, text};
        use iced::Fill;
        let cover = self.album_cover(a);
        let sub = match a.year {
            Some(y) => format!("{} · {}", a.artist, y),
            None => a.artist.clone(),
        };
        button(
            column![
                self.art_thumb(cover.as_deref(), 140.0),
                text(a.title.clone()).size(14).width(Fill),
                text(sub).size(12).color(MUTED).width(Fill),
            ]
            .spacing(4)
            .padding(8)
            .width(Fill),
        )
        .style(card_button)
        .on_press(Message::OpenAlbum(a.id.clone()))
        .padding(0)
        .width(156)
        .into()
    }

    /// Artist card with photo (search grids).
    fn artist_card(&self, a: &Artist) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, text};
        use iced::Fill;
        // Same hires key stage_search warms (raw would placeholder).
        let photo = a
            .thumbnail_url
            .as_deref()
            .map(|u| yutu_core::hires_thumb_url(u, 544));
        button(
            column![
                self.art_thumb(photo.as_deref(), 140.0),
                text(a.name.clone()).size(14).width(Fill),
                text("Artist").size(12).color(MUTED).width(Fill),
            ]
            .spacing(4)
            .padding(8)
            .width(Fill),
        )
        .style(card_button)
        .on_press(Message::OpenArtist(a.id.clone()))
        .padding(0)
        .width(156)
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
        for t in tracks {
            let rt = self.resolve_for_play(&t);
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
    /// radio/discovery/search). With `shuffle`, shuffle turns on first and
    /// the fresh queue plays in shuffled order.
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
        // then the list plays from the top.
        self.queue.clear();
        self.autoplay.clear();
        self.autoplay_for = None;
        self.drag = None;
        let task = self.queue_many(tracks, &label);
        if shuffle {
            // Fresh queue: full shuffle with the just-started track first.
            self.set_shuffle(true);
        }
        self.publish_mpris();
        task
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
            None => {
                self.status = format!("Track not found: {id}.");
                iced::Task::none()
            }
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
    fn artwork_tasks_for_urls(&mut self, urls: &[String]) -> Vec<iced::Task<Message>> {
        let mut tasks = Vec::new();
        for url in urls {
            if !self.artwork.contains_key(url)
                && !self.artwork_loading.contains(url)
                && tasks.len() < 12
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

    /// Cover/photo art by URL at any size (albums, artists).
    fn art_thumb(&self, url: Option<&str>, size: f32) -> iced::Element<'_, Message> {
        use iced::widget::{container, image};
        match url.and_then(|u| self.artwork.get(u)) {
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

    /// Two-line song label: title (normal) + artist (smaller, muted).
    /// `title_size` 14 for pages, 12-13 for panel/autoplay rows.
    fn track_text<'a>(
        title: &str,
        artist: &str,
        title_size: u16,
        color: iced::Color,
    ) -> iced::Element<'a, Message> {
        use iced::widget::{column, text};
        use iced::Fill;
        column![
            // Two-line cap (55 + ellipsis): long names never stretch rows.
            // Rows are art-height dominated at 40px, so 1-2 line titles
            // share one height across the list.
            text(truncate_title(title, 55))
                .size(f32::from(title_size))
                .color(color),
            text(artist.to_owned())
                .size(f32::from(title_size.saturating_sub(2).max(11)))
                .color(MUTED),
        ]
        .spacing(2)
        .width(Fill)
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
    #[cfg(feature = "innertube")]
    fn cookie_section(&self) -> iced::Element<'_, Message> {
        use iced::widget::{button, column, text, text_input};
        use iced::Fill;
        column![
            text("Log into music.youtube.com in your browser, export cookies (e.g. the “Get cookies.txt” extension), then paste the file path (no Google Cloud client required).")
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
    }

    /// Open the playlist menu, freezing the cursor like the track menu.
    fn open_pl_menu(&mut self, id: String) {
        self.menu_at = self.cursor;
        self.pl_menu = Some(id);
        self.context_menu = None;
        self.submenu_playlists_for = None;
        self.show_suggest = false;
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
            if let Some(handle) = self.artwork.get(&url) {
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
        match url.as_deref().and_then(|u| self.artwork.get(u)) {
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
        match url.as_deref().and_then(|u| self.artwork.get(u)) {
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
    pub fn weekly_mix_tracks(&self) -> Vec<Track> {
        weekly_mix(
            &self.discovery_history(),
            &self.liked,
            &self.catalog_tracks(),
            10,
            now_unix(),
        )
    }

    /// Forgotten Favorites from in-memory likes + persisted history + catalog.
    pub fn forgotten_tracks(&self) -> Vec<Track> {
        forgotten_favorites(
            &self.liked,
            &self.discovery_history(),
            &self.catalog_tracks(),
            now_unix(),
        )
    }

    /// Virtual Favorites id (never persisted, never a `uN` user id).
    const FAVORITES_ID: &'static str = "__favorites";

    fn all_playlists(&self) -> Vec<Playlist> {
        let mut out = vec![Playlist {
            id: Self::FAVORITES_ID.into(),
            title: "Favorites".into(),
            tracks: self.favorite_tracks(),
        }];
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

    fn find_playlist(&self, id: &str) -> Option<Playlist> {
        // Favorites skips the clone plus sort of the whole library.
        if id == Self::FAVORITES_ID {
            return Some(Playlist {
                id: Self::FAVORITES_ID.into(),
                title: "Favorites".into(),
                tracks: self.favorite_tracks(),
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
            Message::Navigate(r) => self.navigate(r),
            Message::NavBack => {
                // Side buttons never disturb a drag (Esc owns cancel);
                // menus and the popup dismiss first, second press goes.
                // Header buttons share this message, so they inherit the
                // same dismiss-first order by design (one code path).
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some() || self.pl_menu.is_some() {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_back();
            }
            Message::NavForward => {
                if self.drag.is_some() {
                    return iced::Task::none();
                }
                if self.context_menu.is_some() || self.pl_menu.is_some() {
                    self.close_menu();
                    return iced::Task::none();
                }
                if self.show_suggest {
                    self.show_suggest = false;
                    return iced::Task::none();
                }
                self.go_forward();
            }
            Message::SetRightTab(t) => {
                self.right_tab = t;
            }
            Message::HomeSearchChanged(q) => {
                self.search_query = q.clone();
                let trimmed = q.trim().to_string();
                self.search_gen = self.search_gen.wrapping_add(1);
                self.suggest_gen = self.suggest_gen.wrapping_add(1);
                if trimmed.is_empty() {
                    self.results = SearchResults::default();
                    self.suggest.clear();
                    self.show_suggest = false;
                    self.searching = false;
                } else if trimmed.len() >= 2 {
                    self.show_suggest = true;
                    // Premature results: known tracks paint instantly.
                    self.results = SearchResults {
                        tracks: self.catalog_hits(&trimmed),
                        albums: Vec::new(),
                        artists: Vec::new(),
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
                        self.search_cache_put(&query, search.clone());
                        // Out-of-order arrival (user typed on): keep the
                        // tracks in the catalog, but never overwrite the
                        // newer query's results.
                        if query.trim() == self.search_query.trim() {
                            return self.apply_search_hit(&query, search);
                        }
                        let (_, tasks) = self.stage_search(search);
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
                if self.liked.contains(&id) {
                    self.unlike_id(&id);
                    self.persist_like(&id, false);
                    self.status = format!("Removed from favorites: {id}.");
                } else {
                    self.like_id(&id);
                    self.persist_like(&id, true);
                    self.status = format!("Added to favorites: {id}.");
                }
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
            Message::OpenArtist(id) => self.go(Route::Artist(id)),
            Message::OpenAlbum(id) => self.go(Route::Album(id)),
            Message::OpenPlaylist(id) => self.go(Route::PlaylistDetail(id)),
            Message::OpenLyrics(id) => {
                self.close_menu();
                self.go(Route::Lyrics(id.clone()));
                #[cfg(feature = "lyrics")]
                {
                    let needs = !self.lyrics_cache.contains_key(&id)
                        && self.lyrics_loading.as_deref() != Some(id.as_str());
                    if needs {
                        match self.find_track(&id) {
                            None => {
                                self.status =
                                    "Lyrics need a known track: search for it first.".to_string();
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
            Message::LyricsReady { track_id, result } => {
                // Stale arrivals must not clear a newer in-flight fetch.
                if self.lyrics_loading.as_deref() == Some(track_id.as_str()) {
                    self.lyrics_loading = None;
                }
                match result {
                    Ok(lyrics) => {
                        self.lyrics_cache.insert(track_id, lyrics);
                    }
                    Err(e) => {
                        self.status = format!("Lyrics failed: {e}");
                    }
                }
            }
            Message::ArtworkReady { url, result } => {
                self.artwork_loading.remove(&url);
                if let Ok(bytes) = result {
                    if bytes.len() > 512 {
                        self.artwork
                            .insert(url, iced::widget::image::Handle::from_bytes(bytes));
                    }
                }
            }
            Message::StartRadio(seed) => {
                self.close_menu();
                match self.find_track(&seed) {
                    None => {
                        self.status =
                            format!("Radio needs a known track: search for it first ({seed}).");
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
                    let n = tracks.len();
                    let tracks = self.register_tracks(tracks);
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
                        self.status = format!("Track not found: {id}.");
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
                    self.new_playlist_name.clear();
                    self.status = format!("Created playlist {title} ({id}).");
                }
            }
            Message::AddToPlaylist {
                playlist_id,
                track_id,
            } => match self.find_track(&track_id) {
                None => self.status = format!("Track not found: {track_id}."),
                Some(track) => {
                    // Virtual Favorites maps to likes, never SQLite.
                    if playlist_id == Self::FAVORITES_ID {
                        if self.liked.contains(&track.id) {
                            self.status = format!("Already in Favorites: {}.", track.title);
                        } else {
                            self.like_id(&track.id);
                            self.persist_like(&track.id, true);
                            self.status = format!("Added {} to Favorites.", track.title);
                        }
                        self.publish_mpris();
                        return iced::Task::none();
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
                                self.status = format!("Added {} to {}.", track.title, title);
                            }
                        }
                    }
                }
            },
            Message::RemoveFromPlaylist {
                playlist_id,
                track_id,
            } => {
                // Virtual Favorites maps to unlikes, never SQLite.
                if playlist_id == Self::FAVORITES_ID {
                    if self.liked.contains(&track_id) {
                        self.unlike_id(&track_id);
                        self.persist_like(&track_id, false);
                        self.status = format!("Removed {track_id} from Favorites.");
                    } else {
                        self.status = format!("{track_id} is not in Favorites.");
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
                        self.status = format!("Removed {track_id} from {title}.");
                    }
                }
            }
            Message::DeletePlaylist(id) => {
                self.close_menu();
                if id == Self::FAVORITES_ID {
                    self.status = "Favorites is built-in and can't be deleted.".into();
                    self.publish_mpris();
                    return iced::Task::none();
                }
                let before = self.user_playlists.len();
                self.user_playlists.retain(|p| p.id != id);
                if self.user_playlists.len() < before {
                    self.persist_playlist_deleted(&id);
                    if self.route == Route::PlaylistDetail(id.clone()) {
                        self.go(Route::Library);
                    }
                    self.status = format!("Deleted playlist {id}.");
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
                        self.status = format!("Track not found: {id}.");
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
                            .filter(|t| t.id != seed && !known.contains(&t.id))
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
                        self.status = format!("Track not found: {id}.");
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
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest {
                    self.hovered_queue = None;
                } else {
                    self.hovered_queue = h;
                }
                self.hovered_auto = None;
            }
            Message::TrackHover(h) => {
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest {
                    self.hovered_track = None;
                } else {
                    self.hovered_track = h;
                }
                // Entering a non-queue row drops stale queue hovers so a
                // press there never arms a drag off old state.
                self.hovered_queue = None;
                self.hovered_auto = None;
            }
            Message::ClearHovers => {
                self.hovered_track = None;
                self.hovered_playlist = None;
                self.hovered_queue = None;
                self.hovered_auto = None;
                self.hover_remove = None;
            }
            Message::RemoveHover(h) => {
                self.hover_remove = h;
            }
            Message::PlaylistHover(h) => {
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest {
                    self.hovered_playlist = None;
                } else {
                    self.hovered_playlist = h;
                }
                self.hovered_queue = None;
                self.hovered_auto = None;
            }
            Message::AutoHover(h) => {
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest {
                    self.hovered_auto = None;
                } else {
                    self.hovered_auto = h;
                }
                self.hovered_queue = None;
            }
            Message::PanelHover(i) => {
                if self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest {
                    self.hovered_queue = None;
                } else {
                    self.hovered_queue = Some(i);
                }
                self.hovered_auto = None;
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
                    let modal =
                        self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest;
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
                for t in &p.tracks {
                    let rt = self.resolve_for_play(t);
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
                self.queue.clear();
                self.autoplay.clear();
                self.autoplay_for = None;
                self.cancel_drag();
                let title = p.title.clone();
                let task = self.queue_many(p.tracks.clone(), &format!("Playlist {title}"));
                self.set_shuffle(true);
                self.publish_mpris();
                return task;
            }
            Message::CursorMoved(p) => {
                self.cursor = Some(p);
                // Modal freeze: hover latch never glides behind menus.
                let modal =
                    self.context_menu.is_some() || self.pl_menu.is_some() || self.show_suggest;
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
                if self.context_menu.is_some() || self.pl_menu.is_some() {
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
                        self.status = format!("Track not found: {id}.");
                    }
                }
            }
            Message::OpenTrackArtist(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    Some(t) => self.go(Route::Artist(t.artist.clone())),
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                }
            }
            Message::DownloadTrack(id) => {
                self.close_menu();
                match self.find_track(&id) {
                    None => {
                        self.status = format!("Track not found: {id}.");
                    }
                    Some(t) => {
                        self.status = format!("Downloading {} · {}…", t.title, t.artist);
                        self.publish_mpris();
                        return Self::download_task(id);
                    }
                }
            }
            Message::Downloaded { track_id, result } => match result {
                Ok(()) => {
                    self.status = format!("Downloaded {track_id} for offline listening.");
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
                        self.status = "Reading cookies + contacting YouTube Music…".into();
                        self.publish_mpris();
                        return Self::cookie_import_task(path);
                    }
                }
                #[cfg(not(feature = "innertube"))]
                {
                    let _ = path;
                    self.status =
                        "Cookie import needs `--features innertube` (offline build).".into();
                }
            }
            Message::CookieImported { result } => {
                #[cfg(feature = "innertube")]
                {
                    self.login_busy = false;
                }
                match result {
                    Ok((tracks, liked_ids)) => {
                        let n = tracks.len();
                        let m = liked_ids.len();
                        let tasks = self.merge_imported(tracks, liked_ids);
                        self.status = format!("Cookie import: {n} library tracks ({m} liked).");
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        self.status = format!("Cookie import failed: {e}");
                    }
                }
            }
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
                                self.status = "Signed in: importing your library…".into();
                                self.publish_mpris();
                                return Self::library_import_task();
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
                    self.status = "Importing your library…".into();
                    self.publish_mpris();
                    return Self::library_import_task();
                }
            }
            #[cfg(feature = "innertube")]
            Message::LibraryImported { result } => {
                self.login_busy = false;
                match result {
                    Ok((tracks, liked_ids)) => {
                        let n = tracks.len();
                        let m = liked_ids.len();
                        let tasks = self.merge_imported(tracks, liked_ids);
                        self.status = format!("Imported {n} library tracks ({m} liked).");
                        self.publish_mpris();
                        return iced::Task::batch(tasks);
                    }
                    Err(e) => {
                        self.status = format!("Library import failed: {e}");
                    }
                }
            }
            Message::Tick(now) => {
                // Desktop transport commands first (widget / media keys).
                let mut tasks = vec![self.drain_player_commands()];
                // Position clock: manual advance while a user seek settles
                // (the backend reports the stale pre-seek position for a
                // tick or two: that stale tick is the lyrics flicker).
                let dt = self
                    .last_tick
                    .map(|last| now.duration_since(last).as_secs_f64())
                    .unwrap_or(0.0)
                    .clamp(0.0, 2.0);
                self.last_tick = Some(now);
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
                                }
                                Err(e) => {
                                    let msg = format!("Playback failed for {video_id}: {e}");
                                    self.log_error(msg.clone());
                                    self.status = msg;
                                }
                            },
                        }
                    }
                }
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

    /// Signed-in library import: library songs + liked ids (catalog merge).
    #[cfg(feature = "innertube")]
    fn library_import_task() -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let persist =
                    yutu_core::OAuthPersist::load_from_file(std::path::Path::new(&oauth_path()))?;
                let backend = yutu_core::InnertubeBackend::connect_with_oauth(&persist)?;
                let lib = backend.library_songs().await.unwrap_or_default();
                let liked = backend.liked_songs().await.unwrap_or_default();
                if lib.is_empty() && liked.is_empty() {
                    return Err("empty library: is this the right Google account?".to_string());
                }
                let liked_ids: Vec<String> = liked.iter().map(|t| t.id.clone()).collect();
                let mut all = lib;
                for t in liked {
                    if !all.iter().any(|x| x.id == t.id) {
                        all.push(t);
                    }
                }
                Ok((all, liked_ids))
            },
            |result| Message::LibraryImported { result },
        )
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

    /// Offline download: resolve + fetch bytes, persist to the disk cache.
    fn download_task(track_id: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let result: Result<(), String> = async {
                    let bytes = resolve_and_fetch(&track_id).await?;
                    let mut cache = yutu_player::OfflineCache::new(500 * 1024 * 1024);
                    cache.set_dir(format!("{}/offline_cache", data_dir()));
                    cache
                        .store_bytes(&track_id, &bytes)
                        .map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;
                (track_id, result)
            },
            |(track_id, result)| Message::Downloaded { track_id, result },
        )
    }

    /// Cookie import: normal browser login, no OAuth client needed. The user
    /// logs into music.youtube.com in their own browser, exports cookies to
    /// a file, and points the app at it: same library/liked merge as OAuth.
    #[cfg(feature = "innertube")]
    fn cookie_import_task(path: String) -> iced::Task<Message> {
        iced::Task::perform(
            async move {
                let backend = yutu_core::InnertubeBackend::connect_with_cookie_file(
                    std::path::Path::new(&path),
                )
                .await?;
                let lib = backend.library_songs().await.unwrap_or_default();
                let liked = backend.liked_songs().await.unwrap_or_default();
                if lib.is_empty() && liked.is_empty() {
                    return Err("empty library: bad cookies file or wrong account?".to_string());
                }
                let liked_ids: Vec<String> = liked.iter().map(|t| t.id.clone()).collect();
                let mut all = lib;
                for t in liked {
                    if !all.iter().any(|x| x.id == t.id) {
                        all.push(t);
                    }
                }
                Ok((all, liked_ids))
            },
            |result| Message::CookieImported { result },
        )
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
                    Self::track_text(&t.title, &t.artist, 14, iced::Color::WHITE),
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
            .width(Fill),
        )
        .on_right_press(Message::OpenContext(t.id.clone()))
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
        let lifted: iced::Element<'_, Message> = container(row_content)
            .style(row_bg(hovered))
            .width(Fill)
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
                            button(Self::track_text(
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
                    .width(Fill),
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
                                            button(Self::track_text(
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
                                    .width(Fill),
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
                    // Two-across quick picks, Spotify-style: artwork + title.
                    let rows: Vec<iced::Element<'_, Message>> = picks
                        .chunks(2)
                        .map(|pair| {
                            let cards: Vec<iced::Element<'_, Message>> = pair
                                .iter()
                                .map(|t| {
                                    let menu_id = t.id.clone();
                                    mouse_area(
                                        button(
                                            row![
                                                self.thumb_hero(t, 52.0),
                                                column![
                                                    text(t.title.clone()).size(14),
                                                    text(t.artist.clone()).size(12).color(MUTED),
                                                ]
                                                .spacing(2)
                                                .width(Fill),
                                            ]
                                            .spacing(10)
                                            .padding(6)
                                            .align_y(iced::alignment::Vertical::Center),
                                        )
                                        .style(card_button)
                                        .on_press(Message::PlayNow(t.id.clone()))
                                        .padding(0)
                                        .width(Fill),
                                    )
                                    .on_right_press(Message::OpenContext(menu_id))
                                    .into()
                                })
                                .collect();
                            row(cards).spacing(8).into()
                        })
                        .collect();
                    column(rows).spacing(8).into()
                };
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
                let songs: Vec<iced::Element<'_, Message>> = self
                    .results
                    .tracks
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.numbered_track_row(t, i + 1))
                    .collect();
                let album_cards: Vec<iced::Element<'_, Message>> = self
                    .results
                    .albums
                    .iter()
                    .take(5)
                    .map(|a| self.album_card(a))
                    .collect();
                let artist_cards: Vec<iced::Element<'_, Message>> = self
                    .results
                    .artists
                    .iter()
                    .take(5)
                    .map(|a| self.artist_card(a))
                    .collect();
                // Top-result hero: the first track, Spotify-style.
                let hero: iced::Element<'_, Message> = match self.results.tracks.first() {
                    None => text("").size(1).into(),
                    Some(t) => {
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
                                            "{} · {}",
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
                                            button(heart_icon(
                                                self.liked.contains(&t.id),
                                                15.0,
                                                heart_color
                                            ))
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
                };
                let search_note: iced::Element<'_, Message> = if self.searching {
                    text("Searching YouTube Music…").color(MUTED).into()
                } else if songs.is_empty() && album_cards.is_empty() && artist_cards.is_empty() {
                    text("Results land here: type above, they arrive as you type.")
                        .color(MUTED)
                        .into()
                } else {
                    text("").size(1).into()
                };
                column![
                    text(format!("Results for “{}”", self.search_query)).size(22),
                    search_note,
                    hero,
                    Self::section("Songs", songs),
                    Self::section(
                        "Albums",
                        if album_cards.is_empty() {
                            vec![]
                        } else {
                            vec![row(album_cards).spacing(10).into()]
                        }
                    ),
                    Self::section(
                        "Artists",
                        if artist_cards.is_empty() {
                            vec![]
                        } else {
                            vec![row(artist_cards).spacing(10).into()]
                        }
                    ),
                ]
                .spacing(10)
                .into()
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
                                    button(Self::track_text(
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
                            .width(Fill),
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
                                    button(Self::track_text(
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
                            .width(Fill),
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
                                Self::play_pill("SHUFFLE".into(), Message::PlayAllShuffled),
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
                            vec![row(cards).spacing(10).into()]
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
                                Self::play_pill("SHUFFLE".into(), Message::PlayAllShuffled),
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
                    let rows: Vec<iced::Element<'_, Message>> =
                        p.tracks.iter().map(|t| self.track_row(t)).collect();
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
                            button(icon("ellipsis", 15.0, MUTED))
                                .style(hover_row)
                                .on_press(Message::OpenPlaylistMenu(p.id.clone()))
                                .padding(8),
                        ]
                        .spacing(4)
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
            .height(Fill)
            .into()
        } else {
            scrollable(container(body).padding(20).width(Fill))
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
                .map(|t| self.track_row(t))
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
        let base: iced::Element<'_, Message> =
            container(column![main, self.player_view()].spacing(8).height(Fill))
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
                    button(
                        row![
                            icon("download", 14.0, iced::Color::WHITE),
                            text("Download for offline").size(14)
                        ]
                        .spacing(8)
                        .align_y(iced::alignment::Vertical::Center)
                        .width(Fill),
                    )
                    .style(hover_row)
                    .on_press(Message::DownloadTrack(t.id.clone()))
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
                            icon("mic", 14.0, iced::Color::WHITE),
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
                // buttons below borrow nothing from self). Favorites
                // leads via all_playlists.
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
                if pid3 != Self::FAVORITES_ID {
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
            result: Ok(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Yellow Submarine", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
            }),
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
    fn radio_ready_queues_fetched_sequence() {
        let mut s = test_state();
        // Unknown seed: no catalog entry, honest error, nothing queued.
        let _ = s.update(Message::StartRadio("unknown-id".into()));
        assert!(s.status.contains("search for it first"));
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
            result: Ok(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Hey Jude", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert!(!s.queue.shuffle);
        let _ = s.update(Message::PlayAllShuffled);
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.len(), 2);
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
            result: Ok(SearchResults {
                tracks: vec![
                    mk_track("v1", "Yesterday", "The Beatles"),
                    mk_track("v2", "Hey Jude", "The Beatles"),
                ],
                albums: vec![],
                artists: vec![],
            }),
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

    #[test]
    fn lyrics_ready_caches_and_open_lyrics_fetches_once() {
        use yutu_core::{LyricLine, LyricsResult};
        let mut s = test_state();
        // Unknown id: honest error, no fetch.
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
            result: Ok(SearchResults {
                tracks: vec![mk_track("v1", "Yesterday", "The Beatles")],
                albums: vec![],
                artists: vec![],
            }),
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
            result: Ok(SearchResults {
                tracks: vec![mk_track("old", "Old Song", "Someone")],
                albums: vec![],
                artists: vec![],
            }),
        });
        assert!(s.find_track("old").is_some());
        assert!(s.results.tracks.is_empty());
        // Current payload lands normally.
        let _ = s.update(Message::SearchCompleted {
            query: "bonobo".into(),
            result: Ok(SearchResults {
                tracks: vec![mk_track("new", "Rosewood", "Bonobo")],
                albums: vec![],
                artists: vec![],
            }),
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
        // Imported payload merges catalog + likes.
        let _ = s.update(Message::LibraryImported {
            result: Ok((
                vec![mk_track("lib1", "Lied", "Someone")],
                vec!["lib1".into()],
            )),
        });
        assert!(!s.login_busy);
        assert!(s.find_track("lib1").is_some());
        assert!(s.liked.contains("lib1"));
        assert!(s.status.contains("Imported 1"));
        let _ = s.update(Message::LibraryImported {
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
        // Unknown id: honest error, queue untouched.
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
        // Unknown id: honest error, queue untouched.
        let _ = s.update(Message::QueueTrack("ghost".into()));
        assert!(s.status.contains("not found"));
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
        assert!(s.status.contains("not found"));
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
        // Unknown track: honest error.
        let _ = s.update(Message::OpenTrackAlbum("ghost".into()));
        assert!(s.status.contains("not found"));
        // Download validates the id before touching the network.
        let _ = s.update(Message::DownloadTrack("ghost".into()));
        assert!(s.status.contains("not found"));
        let _ = s.update(Message::Downloaded {
            track_id: "m1".into(),
            result: Ok(()),
        });
        assert!(s.status.contains("Downloaded m1"));
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

    #[test]
    fn cookie_import_validates_and_merges() {
        let mut s = test_state();
        let _ = s.update(Message::CookiePathChanged("/tmp/x.txt".into()));
        assert_eq!(s.cookie_path, "/tmp/x.txt");
        let _ = s.update(Message::CookieImported {
            result: Ok((
                vec![mk_track("c1", "Cookie Song", "Someone")],
                vec!["c1".into()],
            )),
        });
        assert!(s.find_track("c1").is_some());
        assert!(s.liked.contains("c1"));
        assert!(s.status.contains("Cookie import"));
        let _ = s.update(Message::CookieImported {
            result: Err("bad cookies".into()),
        });
        assert!(s.status.contains("bad cookies"));
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
        // Near the bottom-right: clamped so 300x470 stays visible.
        assert_eq!(
            menu_pos(Some(iced::Point::new(1200.0, 700.0)), win),
            (980.0, 330.0)
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
            });
        }
        let _ = s.update(Message::ShufflePlaylist(pid));
        assert_eq!(s.queue.len(), 4);
        assert!(s.queue.shuffle);
        assert_eq!(s.queue.position(), Some(0));
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
            SearchResults {
                tracks: vec![mk_track("v1", "Hey Jude", "The Beatles")],
                ..Default::default()
            },
        );
        let hit = s.search_cache_get("beatles").expect("normalized hit");
        assert_eq!(hit.tracks.len(), 1);
        assert_eq!(hit.tracks[0].id, "v1");
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
        assert!(!r.tracks.is_empty(), "expected real tracks for Beatles");
        assert!(
            r.tracks
                .iter()
                .all(|t| !t.id.is_empty() && !t.title.is_empty()),
            "tracks must carry playable ids + titles"
        );
    }
}
