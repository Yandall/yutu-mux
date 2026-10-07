//! MPRIS2 (Linux) media-player integration over D-Bus.
//!
//! [`MprisPlayer`] is plain, synchronous state: the current [`NowPlaying`]
//! track, a [`PlaybackStatus`] and a volume level. [`MprisPlayer::metadata_map`]
//! builds the exact `a{sv}` metadata dictionary MPRIS clients expect
//! (`mpris:trackid` as an object path, `xesam:title` / `xesam:artist` /
//! `xesam:album`, `mpris:length` in microseconds).
//!
//! [`MprisRoot`] / [`MprisService`] adapt a shared [`SharedPlayer`] for the
//! two standard MPRIS interfaces (`org.mpris.MediaPlayer2` and
//! `org.mpris.MediaPlayer2.Player`) served at [`OBJECT_PATH`] under the
//! well-known name [`SERVICE_NAME`]. Use [`serve`] to publish them.
//!
//! # Future UI wiring (Iced layer)
//!
//! The Iced UI owns playback state today and there is no bridge yet. The
//! intended wiring is:
//!
//! 1. At startup, create one [`SharedPlayer`] (`Arc<Mutex<MprisPlayer>>`)
//!    seeded from the initial queue entry and spawn `serve(handle.clone())`
//!    on the Tokio runtime. `serve` is best-effort: it returns `Err(String)`
//!    on a headless machine instead of panicking, so the app must keep
//!    running when it fails.
//! 2. On every track change, the playback controller calls
//!    `handle.lock().update_track(NowPlaying::from(track))` (and
//!    `set_status(...)` on play / pause / stop). The D-Bus properties read
//!    from the same lock, so external clients (player widgets, media keys
//!    via `playerctld`, desktop "now playing" indicators) observe the update
//!    on their next property read.
//! 3. Follow-up work (not done here): emit `PropertiesChanged` for
//!    `Metadata` / `PlaybackStatus` when the state mutates, which needs the
//!    live [`zbus::Connection`] (returned by [`serve`]) to reach the
//!    `SignalEmitter`; and route incoming `Play` / `Pause` / `Next` /
//!    `Previous` calls back into the Iced playback controller.
//!
//! For the macOS equivalent, see [`crate::macos_notes`].

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use zbus::zvariant::{ObjectPath, OwnedValue, Str, Value};

use crate::{NowPlaying, MPRIS_IDENTITY};

/// Well-known bus name claimed when serving MPRIS.
pub const SERVICE_NAME: &str = "org.mpris.MediaPlayer2.yutu_mux";

/// Object path both MPRIS interfaces are served at.
pub const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

/// Placeholder track id used when there is no current track (per MPRIS convention).
const NO_TRACK_OBJECT_PATH: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// Metadata dictionary keys, kept as constants so producers and tests agree.
pub const KEY_TRACK_ID: &str = "mpris:trackid";
pub const KEY_TITLE: &str = "xesam:title";
pub const KEY_ARTIST: &str = "xesam:artist";
pub const KEY_ALBUM: &str = "xesam:album";
pub const KEY_LENGTH: &str = "mpris:length";

/// MPRIS playback status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

impl PlaybackStatus {
    /// The exact status string MPRIS clients expect.
    pub fn as_str(self) -> &'static str {
        match self {
            PlaybackStatus::Playing => "Playing",
            PlaybackStatus::Paused => "Paused",
            PlaybackStatus::Stopped => "Stopped",
        }
    }
}

impl fmt::Display for PlaybackStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Plain playback state mirrored onto D-Bus.
///
/// Synchronous and unit-testable; shared with the D-Bus service through
/// [`SharedPlayer`].
#[derive(Debug, Clone, PartialEq)]
pub struct MprisPlayer {
    now_playing: NowPlaying,
    status: PlaybackStatus,
    volume: f64,
    position_secs: f64,
}

impl MprisPlayer {
    /// New player state. Volume is clamped to `0.0..=1.0`.
    pub fn new(now_playing: NowPlaying, status: PlaybackStatus, volume: f64) -> Self {
        let mut player = Self {
            now_playing,
            status,
            volume: 1.0,
            position_secs: 0.0,
        };
        player.set_volume(volume);
        player
    }

    /// Current track metadata.
    pub fn now_playing(&self) -> &NowPlaying {
        &self.now_playing
    }

    /// Current playback status.
    pub fn status(&self) -> PlaybackStatus {
        self.status
    }

    /// Current volume (`0.0..=1.0`).
    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Current position in seconds (published by the UI tick).
    pub fn position_secs(&self) -> f64 {
        self.position_secs
    }

    /// Set the position, clamped to `>= 0.0` (non-finite becomes `0.0`).
    pub fn set_position(&mut self, position_secs: f64) {
        self.position_secs = if position_secs.is_finite() {
            position_secs.max(0.0)
        } else {
            0.0
        };
    }

    /// Replace the current track. Called by the UI layer on track change.
    pub fn update_track(&mut self, track: NowPlaying) {
        self.now_playing = track;
    }

    /// Replace the playback status. Called by the UI layer on play/pause/stop.
    pub fn set_status(&mut self, status: PlaybackStatus) {
        self.status = status;
    }

    /// Set the volume, clamped to `0.0..=1.0` (non-finite input becomes `0.0`).
    pub fn set_volume(&mut self, volume: f64) {
        self.volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Build the exact MPRIS `a{sv}` metadata dictionary for the current track.
    ///
    /// Keys: `mpris:trackid` (object path), `xesam:title` (string),
    /// `xesam:artist` (string array), `xesam:album` (string),
    /// `mpris:length` (int64, microseconds).
    pub fn metadata_map(&self) -> HashMap<String, OwnedValue> {
        let track = &self.now_playing;
        let mut map = HashMap::with_capacity(5);
        map.insert(
            KEY_TRACK_ID.to_owned(),
            track_id_value(&track_object_path(&track.track_id)),
        );
        map.insert(KEY_TITLE.to_owned(), owned_string(track.title.clone()));
        if track.artist.is_empty() {
            map.insert(
                KEY_ARTIST.to_owned(),
                owned(Value::from(Vec::<Str<'_>>::new())),
            );
        } else {
            map.insert(
                KEY_ARTIST.to_owned(),
                owned(Value::from(vec![Str::from(track.artist.clone())])),
            );
        }
        map.insert(KEY_ALBUM.to_owned(), owned_string(track.album.clone()));
        map.insert(
            KEY_LENGTH.to_owned(),
            OwnedValue::from(track.length_secs as i64 * 1_000_000),
        );
        map
    }
}

impl Default for MprisPlayer {
    fn default() -> Self {
        Self {
            now_playing: NowPlaying::default(),
            status: PlaybackStatus::default(),
            volume: 1.0,
            position_secs: 0.0,
        }
    }
}

/// Handle shared between the UI thread and the D-Bus service.
///
/// Clone it: one copy moves into [`serve`], the other stays with the
/// playback controller, which mutates it via [`MprisPlayer::update_track`]
/// / [`MprisPlayer::set_status`].
pub type SharedPlayer = Arc<Mutex<MprisPlayer>>;

/// Replace every character that is illegal in a D-Bus object-path element
/// with `_`. Empty input becomes `"unknown"` so the element is never empty.
pub fn sanitize_object_path_element(element: &str) -> String {
    let sanitized: String = element
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "unknown".to_owned()
    } else {
        sanitized
    }
}

/// Object path identifying a track. Falls back to the MPRIS "no track"
/// placeholder when `track_id` is empty.
pub fn track_object_path(track_id: &str) -> String {
    if track_id.is_empty() {
        NO_TRACK_OBJECT_PATH.to_owned()
    } else {
        format!("/yutu_mux/{}", sanitize_object_path_element(track_id))
    }
}

/// Infallible `ObjectPath` for the "no track" placeholder (valid literal).
fn no_track_path() -> ObjectPath<'static> {
    ObjectPath::from_static_str_unchecked(NO_TRACK_OBJECT_PATH)
}

/// Wrap an object-path string as a variant value, falling back to the
/// "no track" placeholder if validation fails. Never panics.
fn track_id_value(path: &str) -> OwnedValue {
    ObjectPath::try_from(path.to_owned())
        .map(OwnedValue::from)
        .unwrap_or_else(|_| OwnedValue::from(no_track_path()))
}

/// Convert a `'static` D-Bus value into an owned variant value, falling back
/// to an empty string on error. Never panics.
fn owned(value: Value<'static>) -> OwnedValue {
    OwnedValue::try_from(value).unwrap_or_else(|_| OwnedValue::from(Str::from(String::new())))
}

/// Convenience wrapper for plain strings.
fn owned_string(s: String) -> OwnedValue {
    owned(Value::from(s))
}

/// D-Bus service adapting a [`SharedPlayer`] to the root MPRIS interface
/// (`org.mpris.MediaPlayer2`).
///
/// Construct with the shared handle and hand it to [`serve`]. See
/// [`MprisService`] for the player-interface half.
#[derive(Debug, Clone)]
pub struct MprisRoot {
    state: SharedPlayer,
}

impl MprisRoot {
    /// Wrap an existing shared player handle for serving.
    pub fn new(state: SharedPlayer) -> Self {
        Self { state }
    }

    /// The shared handle, for the UI layer to keep mutating after serving.
    pub fn handle(&self) -> SharedPlayer {
        Arc::clone(&self.state)
    }

    /// Lock the player state, tolerating a poisoned mutex without panicking.
    fn lock(&self) -> MutexGuard<'_, MprisPlayer> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    #[zbus(property)]
    fn identity(&self) -> String {
        MPRIS_IDENTITY.to_owned()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        // Skeleton: quitting via D-Bus is not wired to the app lifecycle yet.
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }

    /// Skeleton: mark playback stopped; wiring this to the real application
    /// lifecycle is follow-up work.
    fn quit(&self) {
        self.lock().set_status(PlaybackStatus::Stopped);
    }

    /// Skeleton: window raising is not implemented.
    fn raise(&self) {}
}

/// D-Bus service adapting a [`SharedPlayer`] to the player MPRIS interface
/// (`org.mpris.MediaPlayer2.Player`).
///
/// Construct with the shared handle and hand it to [`serve`]; every property
/// and transport method operates on the same lock the UI thread mutates, so
/// no extra synchronisation is needed in the caller. See [`MprisRoot`] for
/// the root-interface half.
#[derive(Debug, Clone)]
pub struct MprisService {
    state: SharedPlayer,
}

impl MprisService {
    /// Wrap an existing shared player handle for serving.
    pub fn new(state: SharedPlayer) -> Self {
        Self { state }
    }

    /// The shared handle, for the UI layer to keep mutating after serving.
    pub fn handle(&self) -> SharedPlayer {
        Arc::clone(&self.state)
    }

    /// Lock the player state, tolerating a poisoned mutex without panicking.
    fn lock(&self) -> MutexGuard<'_, MprisPlayer> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisService {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.lock().status().as_str().to_owned()
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        "None".to_owned()
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.lock().metadata_map()
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.lock().volume()
    }

    #[zbus(property)]
    fn set_volume(&self, volume: f64) {
        self.lock().set_volume(volume);
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        // Microseconds, from the UI-published clock (Tick, 2x/sec).
        (self.lock().position_secs() * 1_000_000.0) as i64
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    /// Skeleton transport control: flips local status only. Routing these
    /// calls into the real playback controller is follow-up work (see the
    /// module docs).
    fn play(&self) {
        self.lock().set_status(PlaybackStatus::Playing);
    }

    /// Skeleton transport control: flips local status only.
    fn pause(&self) {
        self.lock().set_status(PlaybackStatus::Paused);
    }

    /// Skeleton transport control: flips local status only.
    fn play_pause(&self) {
        let next = match self.lock().status() {
            PlaybackStatus::Playing => PlaybackStatus::Paused,
            PlaybackStatus::Paused | PlaybackStatus::Stopped => PlaybackStatus::Playing,
        };
        self.lock().set_status(next);
    }

    /// Skeleton transport control: flips local status only.
    fn stop(&self) {
        self.lock().set_status(PlaybackStatus::Stopped);
    }

    /// Skeleton: queue navigation lives in the UI layer and is not wired yet.
    fn next(&self) {}

    /// Skeleton: queue navigation lives in the UI layer and is not wired yet.
    fn previous(&self) {}
}

/// Publish the player on the session bus as [`SERVICE_NAME`].
///
/// Best-effort: connects to the session bus, registers both MPRIS
/// interfaces at [`OBJECT_PATH`] and claims the well-known name. Returns the
/// live connection (keep it alive for as long as the service should be
/// visible). On a headless machine with no session bus this returns
/// `Err(String)`; it never panics.
pub async fn serve(state: SharedPlayer) -> Result<zbus::Connection, String> {
    let root = MprisRoot::new(Arc::clone(&state));
    let player = MprisService::new(state);
    zbus::connection::Builder::session()
        .map_err(|err| format!("no D-Bus session bus available: {err}"))?
        .serve_at(OBJECT_PATH, root)
        .map_err(|err| format!("failed to serve {OBJECT_PATH}: {err}"))?
        .serve_at(OBJECT_PATH, player)
        .map_err(|err| format!("failed to serve {OBJECT_PATH}: {err}"))?
        .name(SERVICE_NAME)
        .map_err(|err| format!("failed to request {SERVICE_NAME}: {err}"))?
        .build()
        .await
        .map_err(|err| format!("failed to establish D-Bus connection: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_track() -> NowPlaying {
        NowPlaying {
            title: "Midnight City".to_owned(),
            artist: "M83".to_owned(),
            album: "Hurry Up, We're Dreaming".to_owned(),
            track_id: "m1".to_owned(),
            length_secs: 185,
        }
    }

    #[test]
    fn metadata_keys_and_scalar_values() {
        let player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 0.8);
        let map = player.metadata_map();

        assert_eq!(map.len(), 5);
        assert_eq!(
            String::try_from(map[KEY_TITLE].clone()).expect("title decodes"),
            "Midnight City"
        );
        assert_eq!(
            String::try_from(map[KEY_ALBUM].clone()).expect("album decodes"),
            "Hurry Up, We're Dreaming"
        );
        assert_eq!(
            i64::try_from(map[KEY_LENGTH].clone()).expect("length decodes"),
            185_000_000
        );
    }

    #[test]
    fn metadata_track_id_is_object_path() {
        let player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 1.0);
        let map = player.metadata_map();

        let path = ObjectPath::try_from(map[KEY_TRACK_ID].clone()).expect("trackid is object path");
        assert_eq!(path.as_str(), "/yutu_mux/m1");
    }

    #[test]
    fn metadata_artist_is_singleton_string_array() {
        let player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 1.0);
        let map = player.metadata_map();

        let artists =
            Vec::<String>::try_from(map[KEY_ARTIST].clone()).expect("artist is string array");
        assert_eq!(artists, vec!["M83".to_owned()]);
    }

    #[test]
    fn metadata_empty_artist_maps_to_empty_array() {
        let mut track = sample_track();
        track.artist.clear();
        let player = MprisPlayer::new(track, PlaybackStatus::Playing, 1.0);

        let artists = Vec::<String>::try_from(player.metadata_map()[KEY_ARTIST].clone())
            .expect("artist is string array");
        assert!(artists.is_empty());
    }

    #[test]
    fn metadata_length_uses_microseconds() {
        let mut track = sample_track();
        track.length_secs = 0;
        let player = MprisPlayer::new(track, PlaybackStatus::Playing, 1.0);
        assert_eq!(
            i64::try_from(player.metadata_map()[KEY_LENGTH].clone()).expect("length decodes"),
            0
        );
    }

    #[test]
    fn track_id_sanitizes_object_path_elements() {
        assert_eq!(track_object_path("m1"), "/yutu_mux/m1");
        assert_eq!(track_object_path("a b-c.d/e"), "/yutu_mux/a_b_c_d_e");
        assert_eq!(track_object_path(""), NO_TRACK_OBJECT_PATH);
    }

    #[test]
    fn update_track_and_status_mutate_state() {
        let mut player = MprisPlayer::default();
        assert_eq!(player.status(), PlaybackStatus::Stopped);

        player.update_track(sample_track());
        player.set_status(PlaybackStatus::Playing);

        assert_eq!(player.now_playing().title, "Midnight City");
        assert_eq!(player.status(), PlaybackStatus::Playing);
        assert_eq!(
            String::try_from(player.metadata_map()[KEY_TITLE].clone()).expect("title decodes"),
            "Midnight City"
        );
    }

    #[test]
    fn volume_is_clamped_and_finite() {
        let mut player = MprisPlayer::default();
        player.set_volume(2.0);
        assert_eq!(player.volume(), 1.0);
        player.set_volume(-0.5);
        assert_eq!(player.volume(), 0.0);
        player.set_volume(f64::NAN);
        assert_eq!(player.volume(), 0.0);
        player.set_volume(0.5);
        assert_eq!(player.volume(), 0.5);
    }

    #[test]
    fn status_strings_match_mpris_vocabulary() {
        assert_eq!(PlaybackStatus::Playing.as_str(), "Playing");
        assert_eq!(PlaybackStatus::Paused.as_str(), "Paused");
        assert_eq!(PlaybackStatus::Stopped.as_str(), "Stopped");
    }

    /// Needs a real session bus (run with `dbus-run-session`):
    /// `cargo test -p yutu-desktop -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn serve_registers_service_on_session_bus() {
        let state: SharedPlayer = Arc::new(Mutex::new(MprisPlayer::new(
            sample_track(),
            PlaybackStatus::Playing,
            1.0,
        )));
        match serve(state).await {
            Ok(connection) => {
                let proxy = zbus::fdo::DBusProxy::new(&connection)
                    .await
                    .expect("standard bus proxy is constructible");
                let name: zbus::names::BusName<'_> = SERVICE_NAME
                    .try_into()
                    .expect("service name is a valid bus name");
                assert!(
                    proxy
                        .name_has_owner(name)
                        .await
                        .expect("name ownership is queryable"),
                    "service name should be owned after serve()"
                );
            }
            Err(err) => {
                eprintln!("SKIP: no session bus in this environment: {err}");
            }
        }
    }
}
