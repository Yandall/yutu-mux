//! yutu-ui playlist_sync: what a fresh sign-in does with the library
//! already on this device, and each playlist's link to YouTube Music
//! (synced, uploading, or device-only; upload and stop syncing).
//!
//! Rules:
//! - A fresh cookie import over a non-empty local library asks first
//!   (`ImportChoice`). Both picks drop favorites, dislikes and every
//!   pending push (stale intent from an older session, which once
//!   queued real playlist deletes), then import.
//! - Keep: lists still on the account stay synced, lists missing from
//!   it become device-only copies instead of being pruned.
//! - Stop syncing: the list becomes a device-only copy and its server
//!   id goes on the skip list, so syncs never bring it back as a twin.

use super::*;
use yutu_core::{LibraryStore, NewPlaylistSpec};

/// Where one playlist stands with YouTube Music (header chip, playlist
/// menu, `ctl playlists`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaylistSyncState {
    /// Lives on YouTube Music: edits mirror there while signed in.
    Synced,
    /// Device-only list whose upload is in flight.
    Uploading,
    /// Only on this device: nothing pushes.
    Local,
}

impl PlaylistSyncState {
    /// Stable word for the control socket.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Uploading => "uploading",
            Self::Local => "local",
        }
    }
}

impl AppState {
    /// Sync state of one of the user's playlists. `None` for Favorites
    /// (its songs sync as likes, not as a list) and unknown ids.
    pub(crate) fn playlist_sync_state(&self, id: &str) -> Option<PlaylistSyncState> {
        if id == Self::FAVORITES_ID || !self.user_playlists.iter().any(|p| p.id == id) {
            return None;
        }
        Some(if Self::is_server_playlist(id) {
            PlaylistSyncState::Synced
        } else if self.uploading_playlists.contains(id) {
            PlaylistSyncState::Uploading
        } else {
            PlaylistSyncState::Local
        })
    }

    /// Library-choice prompt for a fresh cookie import, or `None` when
    /// there is nothing local to decide about.
    pub(crate) fn import_choice_prompt(&self, path: &str) -> Option<ImportChoicePrompt> {
        let favorites = self.liked.len();
        let playlists = self.user_playlists.len();
        (favorites > 0 || playlists > 0).then(|| ImportChoicePrompt {
            path: path.to_string(),
            favorites,
            playlists,
        })
    }

    /// Start the fresh cookie import (base stage) from `path`.
    pub(crate) fn start_cookie_import(&mut self, path: String) -> iced::Task<Message> {
        self.login_busy = true;
        self.import_active = true;
        self.sync_active = false;
        self.status = self.language.reading_cookies();
        // Capture the source now: the modal decision later must name
        // the file actually imported, not whatever the field holds when
        // stages finish.
        self.pending_cookie_source = Some(path.clone());
        self.pending_cookie_cleanup = None;
        self.publish_mpris();
        Self::cookie_import_task(path, self.library_path.clone())
    }

    /// Apply a library choice before the import runs (disk + memory):
    /// see the module rules. Keep arms [`AppState::import_keep_local`]
    /// for the coming base reconcile.
    pub(crate) fn reset_library_for_import(&mut self, choice: ImportChoice) {
        let keep = choice == ImportChoice::KeepPlaylists;
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            if let Err(e) = store.reset_library(keep) {
                yutu_core::app_log::warn("yutu-import", format!("library reset: {e}"));
            }
        }
        self.liked.clear();
        self.liked_order.clear();
        self.disliked.clear();
        self.disliked_order.clear();
        self.favorites_order.clear();
        self.uploading_playlists.clear();
        if !keep {
            self.cancel_pl_drag();
            self.user_playlists.clear();
            self.playlist_added.clear();
            if matches!(&self.route, Route::PlaylistDetail(id) if id != Self::FAVORITES_ID) {
                self.go(Route::Home);
            }
        }
        self.import_keep_local = keep;
        self.refresh_home_shelves();
    }

    /// Turn a synced list into a device-only copy under a fresh local
    /// id (disk + memory): rows, sort pick, insertion times and open
    /// routes follow it. Returns the new id, `None` for unknown ids.
    pub(crate) fn detach_to_local(&mut self, server_id: &str) -> Option<String> {
        let pos = self.user_playlists.iter().position(|p| p.id == server_id)?;
        let local = loop {
            self.playlist_seq += 1;
            let id = format!("u{}", self.playlist_seq);
            if !self.user_playlists.iter().any(|p| p.id == id) {
                break id;
            }
        };
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.detach_playlist(server_id, &local);
        }
        self.user_playlists[pos].id = local.clone();
        if let Some(sort) = self.playlist_sorts.remove(server_id) {
            self.persist_playlist_sort(&local, sort);
            if let Ok(store) = LibraryStore::open(&self.library_path) {
                let _ = store.set_pref(&Self::playlist_sort_key(server_id), "");
            }
        }
        if let Some(added) = self.playlist_added.remove(server_id) {
            self.playlist_added.insert(local.clone(), added);
        }
        // The old id dies here: repoint the open page and the history
        // in place (no extra navigation step).
        let old = Route::PlaylistDetail(server_id.to_string());
        let new = Route::PlaylistDetail(local.clone());
        for r in std::iter::once(&mut self.route)
            .chain(self.nav_back.iter_mut())
            .chain(self.nav_forward.iter_mut())
        {
            if *r == old {
                *r = new.clone();
            }
        }
        if self.pl_drag_id.as_deref() == Some(server_id) {
            self.cancel_pl_drag();
        }
        if self.pl_menu.as_deref() == Some(server_id) {
            self.pl_menu = None;
        }
        Some(local)
    }

    /// Stop syncing a YouTube Music list: keep a device-only copy and
    /// skip the server list from now on (it stays on YouTube Music).
    pub(crate) fn stop_playlist_sync(&mut self, server_id: &str) {
        if self.playlist_sync_state(server_id) != Some(PlaylistSyncState::Synced) {
            return;
        }
        let title = self
            .find_playlist(server_id)
            .map(|p| p.title.clone())
            .unwrap_or_default();
        if self.detach_to_local(server_id).is_none() {
            return;
        }
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.mark_playlist_unsynced(server_id);
        }
        self.unsynced_playlists.insert(server_id.to_string());
        self.status = self.language.playlist_sync_stopped(&title);
        self.publish_mpris();
    }

    /// Upload a device-only list to YouTube Music with its songs. The
    /// create reply migrates it to the server id (see
    /// `Message::ServerPlaylistCreated`); until then it reads Uploading.
    pub(crate) fn upload_playlist(&mut self, id: &str) -> iced::Task<Message> {
        if self.playlist_sync_state(id) != Some(PlaylistSyncState::Local) {
            return iced::Task::none();
        }
        if !Self::has_session() {
            self.status = self.language.upload_needs_signin();
            self.publish_mpris();
            return iced::Task::none();
        }
        let Some(p) = self.find_playlist(id) else {
            return iced::Task::none();
        };
        let spec = NewPlaylistSpec {
            title: p.title.clone(),
            video_ids: p.tracks.iter().map(|t| t.id.clone()).collect(),
            ..NewPlaylistSpec::default()
        };
        // A queued create for this list would upload it a second time.
        if let Ok(store) = LibraryStore::open(&self.library_path) {
            let _ = store.clear_pending("create", Some(id), None);
        }
        self.uploading_playlists.insert(id.to_string());
        self.status = self.language.uploading_playlist(&p.title);
        self.publish_mpris();
        Self::playlist_create_sync_task(id.to_string(), spec)
    }
}

impl AppState {
    /// Header line under a playlist's count: where the list stands with
    /// YouTube Music. Synced is the active state (filled cloud, accent);
    /// a synced list while signed out says edits wait for sign-in.
    pub(crate) fn playlist_sync_line(&self, id: &str) -> Option<iced::Element<'_, Message>> {
        use iced::widget::{row, text};
        let state = self.playlist_sync_state(id)?;
        let (glyph, label, color): (iced::Element<'_, Message>, String, iced::Color) = match state {
            PlaylistSyncState::Synced if self.signed_in() => (
                cloud_icon(true, 13.0, ACCENT),
                self.language.playlist_synced(),
                iced::Color::WHITE,
            ),
            PlaylistSyncState::Synced => (
                cloud_icon(false, 13.0, MUTED),
                self.language.playlist_synced_offline(),
                MUTED,
            ),
            PlaylistSyncState::Uploading => (
                btn_icon("cloud-upload", 13.0, MUTED),
                self.language.playlist_uploading(),
                MUTED,
            ),
            PlaylistSyncState::Local => (
                btn_icon("cloud-off", 13.0, MUTED),
                self.language.playlist_local(),
                MUTED,
            ),
        };
        Some(
            row![glyph, text(label).size(12).color(color)]
                .spacing(6)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        )
    }
}
