//! mpris_tests: moved from mpris.rs (no behavior change).

#[allow(unused_imports)]
use super::mpris::*;
#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use zbus::zvariant::ObjectPath;

fn sample_track() -> NowPlaying {
    NowPlaying {
        title: "Midnight City".to_owned(),
        artist: "M83".to_owned(),
        album: "Hurry Up, We're Dreaming".to_owned(),
        track_id: "m1".to_owned(),
        length_secs: 185,
        art_url: "https://example.invalid/hero".to_owned(),
    }
}

#[test]
fn metadata_keys_and_scalar_values() {
    let player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 0.8);
    let map = player.metadata_map();

    assert_eq!(map.len(), 6);
    assert_eq!(
        String::try_from(map[KEY_ART_URL].clone()).expect("art decodes"),
        "https://example.invalid/hero"
    );
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
fn command_inbox_queues_and_drains_in_order() {
    let mut player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 1.0);
    assert!(player.drain_commands().is_empty());
    player.push_command(PlayerCommand::PlayPause);
    player.push_command(PlayerCommand::Seek(5.0));
    let drained = player.drain_commands();
    assert_eq!(
        drained,
        vec![PlayerCommand::PlayPause, PlayerCommand::Seek(5.0)]
    );
    assert!(player.drain_commands().is_empty());
    // Overflow caps the inbox instead of growing it.
    for _ in 0..20 {
        player.push_command(PlayerCommand::Next);
    }
    assert!(player.drain_commands().len() <= 16);
}

#[test]
fn loop_status_parses_wire_values() {
    assert_eq!(LoopStatus::parse("None"), Some(LoopStatus::None));
    assert_eq!(LoopStatus::parse("Track"), Some(LoopStatus::Track));
    assert_eq!(LoopStatus::parse("Playlist"), Some(LoopStatus::Playlist));
    assert_eq!(LoopStatus::parse("bogus"), None);
    assert_eq!(LoopStatus::Track.as_str(), "Track");
}

#[test]
fn snapshot_tracks_id_status_and_volume() {
    let mut player = MprisPlayer::new(sample_track(), PlaybackStatus::Playing, 1.0);
    let a = player.snapshot();
    player.set_status(PlaybackStatus::Paused);
    assert_ne!(a, player.snapshot());
    player.set_status(PlaybackStatus::Playing);
    assert_eq!(a, player.snapshot());
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

    let artists = Vec::<String>::try_from(map[KEY_ARTIST].clone()).expect("artist is string array");
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
