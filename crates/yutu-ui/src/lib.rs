//! yutu-ui: native Iced app: real YouTube Music data only, no mock fixtures.

mod dropdown;
mod eq_graph;
mod i18n;
mod pointing_slider;
mod row_button;
mod seek_bar;

pub use i18n::Language;

mod app;
mod app_state;
mod catalog;
mod fonts;
mod helpers;
mod icons;
mod layout;
mod playback_sink;
mod radio;
mod search;
mod state;
mod storage;
mod theme;
mod widgets;
pub use app::*;
pub use app_state::*;
pub(crate) use fonts::*;
pub(crate) use helpers::*;
pub(crate) use icons::*;
pub(crate) use layout::*;
pub(crate) use playback_sink::*;
pub use radio::*;
pub use search::*;
pub use state::*;
pub use storage::*;
pub(crate) use theme::*;
pub(crate) use widgets::*;
#[cfg(test)]
mod app_tests;
mod dialogs_menus;
mod library_ops;
mod lifecycle;
mod persist;
mod playback;
mod player_cards;
mod queue_artwork;
mod seek_status;
mod tasks;
mod update;
mod views_main;
mod views_search;
