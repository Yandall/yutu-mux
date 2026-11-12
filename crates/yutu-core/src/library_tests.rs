//! library_tests: moved from library.rs (no behavior change).

#[allow(unused_imports)]
use super::library::*;
#[allow(unused_imports)]
use super::*;

#[test]
fn like_round_trip() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(!s.is_liked("m1").unwrap());
    s.like("m1").unwrap();
    assert!(s.is_liked("m1").unwrap());
    s.unlike("m1").unwrap();
    assert!(!s.is_liked("m1").unwrap());
}

#[test]
fn dislike_round_trip_preserves_order() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(!s.is_disliked("m1").unwrap());
    assert!(s.disliked_ids().unwrap().is_empty());
    s.dislike("vid-b").unwrap();
    s.dislike("vid-a").unwrap();
    s.dislike("vid-a").unwrap();
    assert!(s.is_disliked("vid-a").unwrap());
    assert_eq!(s.disliked_ids().unwrap(), vec!["vid-b", "vid-a"]);
    s.undislike("vid-a").unwrap();
    assert!(!s.is_disliked("vid-a").unwrap());
    assert_eq!(s.disliked_ids().unwrap(), vec!["vid-b"]);
}

#[test]
fn prefs_round_trip() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(s.get_pref("auto_sync").unwrap().is_none());
    s.set_pref("auto_sync", "0").unwrap();
    assert_eq!(s.get_pref("auto_sync").unwrap().as_deref(), Some("0"));
    s.set_pref("auto_sync", "1").unwrap();
    assert_eq!(s.get_pref("auto_sync").unwrap().as_deref(), Some("1"));
}

#[test]
fn pending_collapses_to_latest_intent() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(s.pending_ops().unwrap().is_empty());
    s.record_pending("like", None, Some("m1"), None).unwrap();
    s.record_pending("unlike", None, Some("m1"), None).unwrap();
    // Only the latest rate row for the track survives.
    assert_eq!(
        s.pending_ops().unwrap(),
        vec![PendingOp {
            op: "unlike".into(),
            playlist_id: None,
            track_id: Some("m1".into()),
            title: None,
        }]
    );
    s.record_pending("add", Some("PL1"), Some("m1"), None)
        .unwrap();
    s.record_pending("remove", Some("PL1"), Some("m1"), None)
        .unwrap();
    let ops = s.pending_ops().unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[1].op, "remove");
    // Recording a rename keeps the latest title only.
    s.record_pending("rename", Some("PL1"), None, Some("Old"))
        .unwrap();
    s.record_pending("rename", Some("PL1"), None, Some("New"))
        .unwrap();
    let renames: Vec<_> = s
        .pending_ops()
        .unwrap()
        .into_iter()
        .filter(|o| o.op == "rename")
        .collect();
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0].title.as_deref(), Some("New"));
    // Recording a delete drops add/remove/rename rows for that playlist.
    s.record_pending("delete", Some("PL1"), None, None).unwrap();
    let ops = s.pending_ops().unwrap();
    assert!(ops.iter().any(|o| o.op == "unlike"));
    assert!(ops.iter().any(|o| o.op == "delete"));
    assert!(!ops.iter().any(|o| o.op == "rename"));
    // Exact-op clear leaves newer rows alone.
    s.clear_pending("like", None, Some("m1")).unwrap();
    s.clear_pending("unlike", None, Some("m1")).unwrap();
}

#[test]
fn rename_pending_clears_only_matching_title() {
    let s = LibraryStore::in_memory().unwrap();
    s.record_pending("rename", Some("PL1"), None, Some("B"))
        .unwrap();
    // Stale in-flight title does not clear the newer intent.
    s.clear_pending_rename("PL1", "A").unwrap();
    assert_eq!(s.pending_ops().unwrap().len(), 1);
    s.clear_pending_rename("PL1", "B").unwrap();
    assert!(s.pending_ops().unwrap().is_empty());
}

#[test]
fn rename_playlist_updates_title_only() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    s.create_playlist("u1", "Gym").unwrap();
    let t = Track {
        id: "m1".into(),
        title: "Song".into(),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.add_to_playlist("u1", &t).unwrap();
    s.rename_playlist("u1", "Evening").unwrap();
    let lists = s.user_playlists().unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].title, "Evening");
    assert_eq!(lists[0].tracks.len(), 1);
    // Unknown ids are a no-op, never an error.
    s.rename_playlist("missing", "Whatever").unwrap();
    assert_eq!(s.user_playlists().unwrap().len(), 1);
}

#[test]
fn pending_repoint_moves_shell_rows_to_server_id() {
    let s = LibraryStore::in_memory().unwrap();
    s.record_pending("add", Some("u7"), Some("m1"), None)
        .unwrap();
    s.record_pending("create", Some("u7"), None, Some("Gym"))
        .unwrap();
    s.repoint_pending("u7", "PLsrv").unwrap();
    for o in s.pending_ops().unwrap() {
        assert_eq!(o.playlist_id.as_deref(), Some("PLsrv"));
    }
    assert!(s.has_playlist_pending("PLsrv").unwrap());
    assert!(!s.has_playlist_pending("u7").unwrap());
}

#[test]
fn sync_pull_replaces_sets_and_prunes() {
    let s = LibraryStore::in_memory().unwrap();
    s.like("stale").unwrap();
    s.dislike("stale-dislike").unwrap();
    // A still-pending unlike must survive the server-wins replace.
    s.like("srv").unwrap();
    s.record_pending("unlike", None, Some("srv"), None).unwrap();
    s.apply_sync_pull(&["srv".to_string()], &[], true, &[])
        .unwrap();
    // Server-dropped rows are gone; the pending unlike removed `srv`.
    assert!(s.liked_ids().unwrap().is_empty());
    assert!(s.disliked_ids().unwrap().is_empty());
    // A pending like rides over the replace instead of being lost.
    s.clear_pending("unlike", None, Some("srv")).unwrap();
    s.record_pending("like", None, Some("fresh"), None).unwrap();
    s.apply_sync_pull(&["srv".to_string()], &[], true, &[])
        .unwrap();
    assert_eq!(s.liked_ids().unwrap(), vec!["srv", "fresh"]);
    // Prune deletes listed playlists (and only those).
    s.create_playlist("PLgone", "Gone").unwrap();
    s.create_playlist("PLkept", "Kept").unwrap();
    s.create_playlist("u9", "Local").unwrap();
    s.apply_sync_pull(&[], &[], false, &["PLgone".to_string()])
        .unwrap();
    let ids: Vec<String> = s
        .user_playlists()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert!(!ids.contains(&"PLgone".to_string()));
    assert!(ids.contains(&"PLkept".to_string()));
    assert!(ids.contains(&"u9".to_string()));
}

#[test]
fn liked_ids_lists_without_catalog_probe() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(s.liked_ids().unwrap().is_empty());
    s.like("vid-b").unwrap();
    s.like("vid-a").unwrap();
    s.like("vid-a").unwrap();
    // Add order, not alphabetical: b first, re-like is a no-op.
    assert_eq!(s.liked_ids().unwrap(), vec!["vid-b", "vid-a"]);
    s.unlike("vid-a").unwrap();
    assert_eq!(s.liked_ids().unwrap(), vec!["vid-b"]);
}

#[test]
fn liked_ids_preserves_insertion_order() {
    let s = LibraryStore::in_memory().unwrap();
    s.like("b").unwrap();
    s.like("a").unwrap();
    s.like("c").unwrap();
    assert_eq!(s.liked_ids().unwrap(), vec!["b", "a", "c"]);
    s.unlike("a").unwrap();
    s.like("a").unwrap();
    assert_eq!(s.liked_ids().unwrap(), vec!["b", "c", "a"]);
}

#[test]
fn queue_snapshot_round_trips() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(s.load_queue().unwrap().is_none());
    s.save_queue(&QueueSnapshot {
        ids: vec!["m1".into(), "m2".into(), "m3".into()],
        index: Some(1),
        position_secs: 42.5,
        shuffle: true,
        repeat: "all".into(),
        volume: 0.5,
        muted: true,
    })
    .unwrap();
    let snap = s.load_queue().unwrap().expect("saved");
    assert_eq!(snap.ids, vec!["m1", "m2", "m3"]);
    assert_eq!(snap.index, Some(1));
    assert!((snap.position_secs - 42.5).abs() < 1e-6);
    assert!(snap.shuffle);
    assert_eq!(snap.repeat, "all");
    assert!((snap.volume - 0.5).abs() < 1e-6);
    assert!(snap.muted);
    // Overwrite replaces wholesale (no stale ids or meta).
    s.save_queue(&QueueSnapshot {
        ids: vec!["m9".into()],
        index: None,
        position_secs: 0.0,
        shuffle: false,
        repeat: "off".into(),
        volume: 0.8,
        muted: false,
    })
    .unwrap();
    let snap = s.load_queue().unwrap().expect("saved");
    assert_eq!(snap.ids, vec!["m9"]);
    assert_eq!(snap.index, None);
    // Clearing reads back as nothing saved.
    s.clear_queue().unwrap();
    assert!(s.load_queue().unwrap().is_none());
}

#[test]
fn queue_snapshot_parsing_is_lenient() {
    let s = LibraryStore::in_memory().unwrap();
    s.save_queue(&QueueSnapshot {
        ids: vec!["m1".into()],
        index: Some(99),
        position_secs: f64::NAN,
        shuffle: false,
        repeat: "bogus".into(),
        volume: 99.0,
        muted: false,
    })
    .unwrap();
    // NaN parses to 0, unknown repeat falls back to off, volume clamps.
    let snap = s.load_queue().unwrap().expect("saved");
    assert_eq!(snap.position_secs, 0.0);
    assert_eq!(snap.repeat, "off");
    assert!((snap.volume - 1.0).abs() < 1e-6);
    // Index clamping is the caller's job (kept verbatim here).
    assert_eq!(snap.index, Some(99));
}

#[test]
fn history_order() {
    let s = LibraryStore::in_memory().unwrap();
    s.record_play("m1", 1).unwrap();
    s.record_play("m2", 2).unwrap();
    let h = s.history(10).unwrap();
    assert_eq!(h[0].track_id, "m2");
    assert_eq!(h.len(), 2);
}

#[test]
fn playlists_crud_persists() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    s.create_playlist("u1", "Gym").unwrap();
    let t = Track {
        id: "m1".into(),
        title: "Come Together".into(),
        artist: "The Beatles".into(),
        album: Some("Abbey Road".into()),
        duration_secs: 259,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.add_to_playlist("u1", &t).unwrap();
    s.add_to_playlist("u1", &t).unwrap(); // deduped
    let lists = s.user_playlists().unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].tracks.len(), 1);
    assert_eq!(lists[0].tracks[0].album.as_deref(), Some("Abbey Road"));
    s.remove_from_playlist("u1", "m1").unwrap();
    assert!(s.user_playlists().unwrap()[0].tracks.is_empty());
    s.delete_playlist("u1").unwrap();
    assert!(s.user_playlists().unwrap().is_empty());
}

#[test]
fn server_playlist_sync_refreshes_title_and_tracks() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    let mk = |id: &str| Track {
        id: id.into(),
        title: format!("Song {id}"),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.sync_server_playlist("PL1", "Road Trip", &[mk("a"), mk("b")])
        .unwrap();
    // Re-import with a rename plus a server-side removal mirrors both.
    s.sync_server_playlist("PL1", "Road Trip Renamed", &[mk("b"), mk("c")])
        .unwrap();
    let lists = s.user_playlists().unwrap();
    assert_eq!(lists.len(), 1);
    assert_eq!(lists[0].title, "Road Trip Renamed");
    let ids: Vec<&str> = lists[0].tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["b", "c"]);
}

#[test]
fn remove_from_middle_keeps_order() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    let mk = |id: &str| Track {
        id: id.into(),
        title: format!("Song {id}"),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.create_playlist("u1", "Gym").unwrap();
    for id in ["a", "b", "c", "d"] {
        s.add_to_playlist("u1", &mk(id)).unwrap();
    }
    s.remove_from_playlist("u1", "b").unwrap();
    let ids: Vec<String> = s.user_playlists().unwrap()[0]
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["a", "c", "d"]);
    // Removing a missing track is a no-op, never an error.
    s.remove_from_playlist("u1", "ghost").unwrap();
    let ids: Vec<String> = s.user_playlists().unwrap()[0]
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["a", "c", "d"]);
    // Head and tail removals keep the rest in order too.
    s.remove_from_playlist("u1", "a").unwrap();
    s.remove_from_playlist("u1", "d").unwrap();
    let ids: Vec<String> = s.user_playlists().unwrap()[0]
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["c"]);
}

#[test]
fn big_playlist_sync_and_remove_round_trips() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    let tracks: Vec<Track> = (0..500)
        .map(|i| Track {
            id: format!("t{i:04}"),
            title: format!("Song {i}"),
            artist: "Someone".into(),
            album: None,
            duration_secs: 200,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
            is_video: false,
        })
        .collect();
    s.sync_server_playlist("PLbig", "Big", &tracks).unwrap();
    assert_eq!(s.user_playlists().unwrap()[0].tracks.len(), 500);
    s.remove_from_playlist("PLbig", "t0000").unwrap();
    let lists = s.user_playlists().unwrap();
    assert_eq!(lists[0].tracks.len(), 499);
    assert_eq!(lists[0].tracks[0].id, "t0001");
    assert_eq!(lists[0].tracks[498].id, "t0499");
}

#[test]
fn playlist_art_round_trips() {
    use crate::models::Track;
    let s = LibraryStore::in_memory().unwrap();
    s.create_playlist("u1", "Gym").unwrap();
    let t = Track {
        id: "m1".into(),
        title: "Come Together".into(),
        artist: "The Beatles".into(),
        album: Some("Abbey Road".into()),
        duration_secs: 259,
        thumbnail_url: Some("https://example.invalid/s".into()),
        thumbnail_large_url: Some("https://example.invalid/l".into()),
        plays: None,
        is_video: false,
    };
    s.add_to_playlist("u1", &t).unwrap();
    let lists = s.user_playlists().unwrap();
    assert_eq!(
        lists[0].tracks[0].thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
    assert_eq!(
        lists[0].tracks[0].thumbnail_large_url.as_deref(),
        Some("https://example.invalid/l")
    );
}

#[test]
fn migration_adds_art_columns_to_old_db() {
    let s = LibraryStore::in_memory().unwrap();
    s.create_playlist("u1", "Gym").unwrap();
    s.conn
        .execute(
            "INSERT INTO playlist_tracks
             (playlist_id, position, track_id, title, artist, album, duration_secs)
             VALUES ('u1', 0, 'm1', 'Come Together', 'The Beatles', 'Abbey Road', 259)",
            [],
        )
        .unwrap();
    // Simulate a pre-art database: drop the new columns, remigrate.
    s.conn
        .execute_batch(
            "CREATE TEMP TABLE backup AS SELECT playlist_id, position, track_id, title, artist, album, duration_secs FROM playlist_tracks;
             DROP TABLE playlist_tracks;
             CREATE TABLE playlist_tracks(
                 playlist_id TEXT NOT NULL,
                 position INTEGER NOT NULL,
                 track_id TEXT NOT NULL,
                 title TEXT NOT NULL,
                 artist TEXT NOT NULL,
                 album TEXT,
                 duration_secs INTEGER NOT NULL,
                 PRIMARY KEY (playlist_id, position));
             INSERT INTO playlist_tracks SELECT * FROM backup;
             DROP TABLE backup;",
        )
        .unwrap();
    s.migrate().unwrap();
    // Old rows survive with empty art; new writes carry art.
    let lists = s.user_playlists().unwrap();
    assert_eq!(lists[0].tracks.len(), 1);
    assert_eq!(lists[0].tracks[0].id, "m1");
    assert!(lists[0].tracks[0].thumbnail_url.is_none());
}

fn cached_fixture(id: &str) -> Track {
    Track {
        id: id.into(),
        title: format!("title-{id}"),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: Some("https://example.invalid/s".into()),
        thumbnail_large_url: Some("https://example.invalid/l".into()),
        plays: None,
        is_video: false,
    }
}

#[test]
fn track_cache_round_trips_and_preserves_art() {
    let s = LibraryStore::in_memory().unwrap();
    assert!(s.cached_track("m1").unwrap().is_none());
    s.upsert_track(&cached_fixture("m1")).unwrap();
    let got = s.cached_track("m1").unwrap().unwrap();
    assert_eq!(got.title, "title-m1");
    assert_eq!(
        got.thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
    // Incoming row with no art must not clobber cached art.
    let bare = Track {
        thumbnail_url: None,
        thumbnail_large_url: None,
        ..cached_fixture("m1")
    };
    s.upsert_track(&bare).unwrap();
    let kept = s.cached_track("m1").unwrap().unwrap();
    assert_eq!(
        kept.thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
    // Fresh art heals a bare cached row.
    let s2 = LibraryStore::in_memory().unwrap();
    s2.upsert_track(&bare).unwrap();
    // Bare-but-titled rows are cached (title survives); art stays None.
    assert!(s2.cached_track("m1").unwrap().is_some());
    s2.upsert_track(&cached_fixture("m1")).unwrap();
    let healed = s2.cached_track("m1").unwrap().unwrap();
    assert_eq!(
        healed.thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
}

#[test]
fn bare_id_rows_never_poison_cache() {
    let s = LibraryStore::in_memory().unwrap();
    let bare = Track {
        id: "ghost".into(),
        title: String::new(),
        artist: String::new(),
        album: None,
        duration_secs: 0,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.upsert_track(&bare).unwrap();
    assert!(s.cached_track("ghost").unwrap().is_none());
}

#[test]
fn like_track_caches_metadata_for_restart() {
    let s = LibraryStore::in_memory().unwrap();
    s.like_track(&cached_fixture("m1")).unwrap();
    assert_eq!(s.liked_ids().unwrap(), vec!["m1"]);
    let cached = s.cached_track("m1").unwrap().unwrap();
    assert_eq!(cached.title, "title-m1");
    s.unlike("m1").unwrap();
    // Unlike drops the like but keeps the cache for history.
    assert!(s.liked_ids().unwrap().is_empty());
    assert!(s.cached_track("m1").unwrap().is_some());
}

#[test]
fn upsert_heals_pre_art_playlist_rows() {
    let s = LibraryStore::in_memory().unwrap();
    s.create_playlist("u1", "Gym").unwrap();
    let bare = Track {
        id: "m1".into(),
        title: "Come Together".into(),
        artist: "The Beatles".into(),
        album: Some("Abbey Road".into()),
        duration_secs: 259,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.add_to_playlist("u1", &bare).unwrap();
    assert!(s.user_playlists().unwrap()[0].tracks[0]
        .thumbnail_url
        .is_none());
    s.upsert_tracks(&[cached_fixture("m1")]).unwrap();
    let healed = s.user_playlists().unwrap()[0].tracks[0].clone();
    assert_eq!(
        healed.thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
}
