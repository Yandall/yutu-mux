//! yutu-ui app_tests: update/view regression tests (no behavior change).

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use std::collections::{HashMap, HashSet};
#[allow(unused_imports)]
use std::sync::atomic::{AtomicU64, Ordering};
#[allow(unused_imports)]
use yutu_core::{
    Album, Artist, HistoryEntry, HistorySection, LibraryStore, LyricsProvider, LyricsResult,
    Playlist, PlaylistHit, QueueSnapshot, SearchAll, SearchResults, TopKind, Track,
};
#[cfg(feature = "mpv")]
#[allow(unused_imports)]
use yutu_player::MpvBackend;
#[allow(unused_imports)]
use yutu_player::{EqPreset, Queue, RodioBackend, BAND_COUNT, MAX_GAIN_DB, MIN_GAIN_DB};

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
        is_video: false,
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
    // Unit tests pin English (system-locale default would make
    // string assertions env-dependent); Spanish gets its own tests.
    s.language = Language::English;
    // Settle the silent startup update probe: unit tests drive
    // completions explicitly, and a perpetually-checking flag would
    // pin `animating()` on for every animation test.
    s.update_checking = false;
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
fn staged(tracks: Vec<Track>, liked_ids: Vec<String>, headers: Vec<(&str, &str)>) -> StagedImport {
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
    // New keystrokes clear the old rows and search once: no
    // premature catalog snapshot flashes before the live payload.
    let _ = s.update(Message::HomeSearchChanged("come".into()));
    assert!(s.results.tracks.is_empty());
    // Failures for the current query report without painting.
    let _ = s.update(Message::SearchCompleted {
        query: "come".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("Search failed"));
    assert!(s.results.tracks.is_empty());
    // Stale-query failures stay silent.
    let _ = s.update(Message::SearchCompleted {
        query: "ancient".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("Search failed"));
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
    let _ = s.suggest_track_row(&wide, false);
    let _ = s.track_row("search", &wide);
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
fn queue_row_fits_wrapped_title_and_artist() {
    // `track_text` allows a 2-line title plus one artist line at a
    // 1.25 line height with a 2px gap, inside 8px top/bottom row
    // padding. The fixed row height must cover that worst case or
    // long titles clip the artist (the container clips, never grows).
    let title = 2.0 * 12.0 * 1.25 + 2.0 + 11.0 * 1.25 + 16.0;
    assert!(
        QUEUE_ROW_H >= title,
        "queue row {QUEUE_ROW_H}px < {title}px for 2-line title + artist"
    );
}

#[test]
fn server_search_keeps_mixed_order() {
    #[allow(unused_imports)]
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
    #[allow(unused_imports)]
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
            plays: None,
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
            plays: None,
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
            plays: None,
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
    #[allow(unused_imports)]
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
fn accented_hits_match_unaccented_queries() {
    #[allow(unused_imports)]
    use yutu_core::{Artist, SearchAll};
    assert_eq!(
        fold_diacritics(&"Héroes del Silencio".to_lowercase()),
        "heroes del silencio"
    );
    assert_eq!(fold_diacritics(&"Beyoncé".to_lowercase()), "beyonce");
    assert_eq!(fold_diacritics(&"Motörhead".to_lowercase()), "motorhead");
    assert!(hit_matches("Héroes del Silencio", "heroes"));
    // The server-ranked accented artist survives the All narrow.
    let all = SearchAll {
        top: None,
        tracks: vec![mk_track("v1", "Heroes", "David Bowie")],
        artists: vec![Artist {
            id: "a1".into(),
            name: "Héroes del Silencio".into(),
            thumbnail_url: None,
        }],
        albums: vec![],
        playlists: vec![],
        mixed: vec![],
    };
    let narrowed = filter_all(&all, "heroes");
    assert_eq!(narrowed.artists.len(), 1);
    assert_eq!(narrowed.artists[0].id, "a1");
    // Same for the tab-depth narrow.
    let filtered = SearchResults {
        tracks: vec![],
        albums: vec![],
        artists: vec![Artist {
            id: "a1".into(),
            name: "Héroes del Silencio".into(),
            thumbnail_url: None,
        }],
        playlists: vec![],
    };
    let narrowed = filter_results(&filtered, "heroes");
    assert_eq!(narrowed.artists.len(), 1);
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
    #[allow(unused_imports)]
    use yutu_core::{MixedRef, SearchAll, TopHit, TopKind};
    // Card without a tap target: title match still heroes right.
    let all = SearchAll {
        top: Some(TopHit {
            kind: TopKind::Song,
            id: String::new(),
            title: "Hey Jude".into(),
            artist: "The Beatles".into(),
            thumbnail_url: None,
            plays: None,
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
    let _ = s.update(Message::Scrolled("queue", 0.0));
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
    let _ = s.update(Message::DrawerScrolled(10.0, 0.0));
    assert_eq!(s.scroll_opacity("drawer"), 1.0);
}

// Offline builds never start a mix (StartRadio only reports).
#[cfg(feature = "innertube")]
#[test]
fn mix_ready_queues_youtube_mix_without_navigating() {
    let mut s = test_state();
    let route = s.route.clone();
    // Unknown seed: the mix response itself carries the seed, so no
    // hydration round-trip and no view change.
    let _ = s.update(Message::StartRadio("unknown-id".into()));
    assert_eq!(s.route, route);
    assert!(s.queue.is_empty());
    let _ = s.update(Message::StartRadio("m1".into()));
    assert_eq!(s.route, route);
    let gen = s.mix_gen;
    let _ = s.update(Message::MixReady {
        gen,
        seed: "m1".into(),
        result: Ok(vec![
            mk_track("v9", "Come Together", "The Beatles"),
            mk_track("m1", "Seed Song", "Seed Artist"),
            mk_track("v9", "Come Together", "The Beatles"),
            mk_track("v8", "Something", "The Beatles"),
        ]),
    });
    // Seed leads, server order after it, duplicates dropped.
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["m1", "v9", "v8"]);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert_eq!(s.route, route);
    assert_eq!(
        s.playing_from.as_deref(),
        Some("Mix of Seed Song - Seed Artist")
    );
    // Failures report cleanly and leave the queue alone.
    let _ = s.update(Message::StartRadio("m1".into()));
    let gen = s.mix_gen;
    let _ = s.update(Message::MixReady {
        gen,
        seed: "m1".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("Mix failed"));
    assert_eq!(s.queue.len(), 3);
}

// Offline builds never start a mix (StartRadio only reports).
#[cfg(feature = "innertube")]
#[test]
fn stale_mix_never_hijacks_a_replaced_queue() {
    let mut s = test_state();
    let _ = s.update(Message::StartRadio("m1".into()));
    let stale = s.mix_gen;
    // The user plays something else before the mix lands.
    let _ = s.update(Message::PlayNow("m2".into()));
    let _ = s.update(Message::MixReady {
        gen: stale,
        seed: "m1".into(),
        result: Ok(vec![mk_track("v9", "Come Together", "The Beatles")]),
    });
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    assert!(s.playing_from.is_none());
}

// Offline builds never start a mix (StartRadio only reports).
#[cfg(feature = "innertube")]
#[test]
fn mix_is_dropped_when_playback_moved_on_while_loading() {
    let mut s = test_state();
    let _ = s.update(Message::PlayNow("m1".into()));
    let _ = s.update(Message::QueueTrack("m2".into()));
    let _ = s.update(Message::StartRadio("m3".into()));
    let gen = s.mix_gen;
    // The user skips ahead before the mix arrives.
    let _ = s.update(Message::QueueJump(1));
    let _ = s.update(Message::MixReady {
        gen,
        seed: "m3".into(),
        result: Ok(vec![mk_track("m3", "Seed Song", "Seed Artist")]),
    });
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    assert!(s.playing_from.is_none());
}

// Offline builds never start a mix (StartRadio only reports).
#[cfg(feature = "innertube")]
#[test]
fn mix_of_live_song_keeps_it_playing() {
    let mut s = test_state();
    let _ = s.update(Message::PlayNow("m1".into()));
    // Pretend the load settled into a paused session (alive): a reload
    // would set `loading_track` again.
    s.loading_track = None;
    s.paused = true;
    let _ = s.update(Message::StartRadio("m1".into()));
    let gen = s.mix_gen;
    let _ = s.update(Message::MixReady {
        gen,
        seed: "m1".into(),
        result: Ok(vec![
            mk_track("m1", "Seed Song", "Seed Artist"),
            mk_track("v9", "Come Together", "The Beatles"),
        ]),
    });
    assert!(s.loading_track.is_none(), "live song must not restart");
    assert_eq!(s.queue.position(), Some(0));
    assert_eq!(s.queue.len(), 2);
    assert!(s.playing_from.is_some());
    // Replacing the queue drops the "Playing from" context.
    let _ = s.update(Message::QueueClear);
    assert!(s.playing_from.is_none());
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
    let path = std::env::temp_dir().join(format!("yutu-test-mig-{}-{}.db", std::process::id(), n));
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
    assert!(s.status.contains("failed"));
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
    let en = Language::English;
    assert_eq!(en.sync_ago(None, 1_000), "never");
    assert_eq!(en.sync_ago(Some(999), 1_000), "just now");
    assert_eq!(en.sync_ago(Some(700), 1_000), "5m ago");
    assert_eq!(en.sync_ago(Some(1_000 - 7_200), 1_000), "2h ago");
    assert_eq!(en.sync_ago(Some(1_000 - 3 * 86_400), 1_000), "3d ago");
    // Spanish renders the same instants in words.
    let es = Language::Spanish;
    assert_eq!(es.sync_ago(None, 1_000), "nunca");
    assert_eq!(es.sync_ago(Some(999), 1_000), "ahora mismo");
    assert_eq!(es.sync_ago(Some(700), 1_000), "hace 5 min");
    assert_eq!(es.sync_ago(Some(1_000 - 7_200), 1_000), "hace 2 h");
    assert_eq!(es.sync_ago(Some(1_000 - 3 * 86_400), 1_000), "hace 3 d");
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
fn playlist_delete_confirms_first() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = s.user_playlists[0].id.clone();
    // The menu row only arms the dialog: nothing is deleted yet.
    let _ = s.update(Message::OpenPlaylistMenu(pid.clone()));
    let _ = s.update(Message::RequestDeletePlaylist(pid.clone()));
    assert!(s.pl_menu.is_none());
    assert_eq!(s.pending_playlist_delete.as_deref(), Some(pid.as_str()));
    assert_eq!(s.user_playlists.len(), 1);
    let _ = s.view();
    // Cancel keeps the playlist; Esc dismisses too.
    let _ = s.update(Message::CancelDeletePlaylist);
    assert!(s.pending_playlist_delete.is_none());
    assert_eq!(s.user_playlists.len(), 1);
    let _ = s.update(Message::RequestDeletePlaylist(pid.clone()));
    let _ = s.update(Message::CloseContext);
    assert!(s.pending_playlist_delete.is_none());
    assert_eq!(s.user_playlists.len(), 1);
    // Confirm deletes and lands home from the detail page.
    let _ = s.update(Message::OpenPlaylist(pid.clone()));
    let _ = s.update(Message::RequestDeletePlaylist(pid.clone()));
    let _ = s.update(Message::DeletePlaylist(pid.clone()));
    assert!(s.user_playlists.is_empty());
    assert!(s.pending_playlist_delete.is_none());
    assert_eq!(s.route, Route::Home);
    // Unknown ids never arm the dialog.
    let _ = s.update(Message::RequestDeletePlaylist("nope".into()));
    assert!(s.pending_playlist_delete.is_none());
    // Favorites never arms the dialog either.
    let _ = s.update(Message::RequestDeletePlaylist("__favorites".into()));
    assert!(s.pending_playlist_delete.is_none());
    assert!(s.status.contains("can't be deleted"));
}

#[cfg(feature = "innertube")]
#[test]
fn signout_confirms_first() {
    let mut s = test_state();
    s.cookie_logged_in = true;
    // The Settings row only arms the dialog: still signed in.
    let _ = s.update(Message::RequestSignOut);
    assert!(s.pending_signout);
    assert!(s.cookie_logged_in);
    assert!(s.signout_dialog().is_some());
    let _ = s.view();
    // Cancel and Esc stay signed in.
    let _ = s.update(Message::CancelSignOut);
    assert!(!s.pending_signout);
    assert!(s.cookie_logged_in);
    let _ = s.update(Message::RequestSignOut);
    let _ = s.update(Message::CloseContext);
    assert!(!s.pending_signout);
    assert!(s.cookie_logged_in);
    // Confirm signs out.
    let _ = s.update(Message::RequestSignOut);
    let _ = s.update(Message::CookieLogout);
    assert!(!s.pending_signout);
    assert!(!s.cookie_logged_in);
    assert!(s.status.contains("cleared"));
}

#[test]
fn playlist_rename_via_dialog() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = s.user_playlists[0].id.clone();
    // The menu row arms the dialog with the current title, nothing renamed yet.
    let _ = s.update(Message::OpenPlaylistMenu(pid.clone()));
    let _ = s.update(Message::RequestRenamePlaylist(pid.clone()));
    assert!(s.pl_menu.is_none());
    assert_eq!(s.pending_playlist_rename.as_deref(), Some(pid.as_str()));
    assert_eq!(s.rename_playlist_name, "Gym");
    assert_eq!(s.user_playlists[0].title, "Gym");
    let _ = s.view();
    // Cancel keeps the old name; Esc dismisses too.
    let _ = s.update(Message::RenamePlaylistNameChanged("Evening".into()));
    let _ = s.update(Message::CancelRenamePlaylist);
    assert!(s.pending_playlist_rename.is_none());
    assert_eq!(s.user_playlists[0].title, "Gym");
    let _ = s.update(Message::RequestRenamePlaylist(pid.clone()));
    let _ = s.update(Message::CloseContext);
    assert!(s.pending_playlist_rename.is_none());
    assert_eq!(s.user_playlists[0].title, "Gym");
    // Confirm renames in memory and in SQLite.
    let _ = s.update(Message::RequestRenamePlaylist(pid.clone()));
    let _ = s.update(Message::RenamePlaylist {
        playlist_id: pid.clone(),
        title: "Evening".into(),
    });
    assert!(s.pending_playlist_rename.is_none());
    assert_eq!(s.user_playlists[0].title, "Evening");
    let store = LibraryStore::open(&s.library_path).unwrap();
    let saved = store.user_playlists().unwrap();
    assert_eq!(saved[0].title, "Evening");
    // Empty names are rejected; unknown and Favorites never arm.
    let _ = s.update(Message::RequestRenamePlaylist(pid.clone()));
    let _ = s.update(Message::RenamePlaylist {
        playlist_id: pid.clone(),
        title: "   ".into(),
    });
    assert_eq!(s.user_playlists[0].title, "Evening");
    let _ = s.update(Message::RequestRenamePlaylist("nope".into()));
    assert!(s.pending_playlist_rename.is_none());
    let _ = s.update(Message::RequestRenamePlaylist("__favorites".into()));
    assert!(s.pending_playlist_rename.is_none());
    assert!(s.status.contains("can't be deleted"));
    // Same name is a no-op with no status churn.
    let _ = s.update(Message::RequestRenamePlaylist(pid.clone()));
    let _ = s.update(Message::RenamePlaylist {
        playlist_id: pid.clone(),
        title: "Evening".into(),
    });
    assert_eq!(s.user_playlists[0].title, "Evening");
}

#[cfg(feature = "update")]
#[test]
fn update_check_reports_newer_and_up_to_date() {
    fn info(tag: &str, newer: bool) -> yutu_core::UpdateInfo {
        yutu_core::UpdateInfo {
            latest_tag: tag.into(),
            html_url: "https://example.test/r".into(),
            notes: "Highlights.".into(),
            newer,
        }
    }
    let mut s = test_state();
    // Manual check arms the probe with a checking status.
    let _ = s.update(Message::CheckForUpdates);
    assert!(s.update_checking);
    assert!(s.status.contains("Checking"));
    // A newer release surfaces with its tag but never arms the modal
    // on a manual check (inline settings row + status only).
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.9", true)),
    });
    assert!(!s.update_checking);
    assert!(s.update_checked);
    assert!(s.pending_update.is_none());
    assert!(s
        .update_available
        .as_ref()
        .is_some_and(|i| i.latest_tag == "v9.9.9"));
    assert!(s.status.contains("v9.9.9"));
    let _ = s.view();
    // Same version clears the badge and confirms up-to-date on manual checks.
    let _ = s.update(Message::CheckForUpdates);
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info(APP_VERSION, false)),
    });
    assert!(s.update_available.is_none());
    assert!(s.status.contains("Up to date"));
    // Failures report honestly on manual checks.
    let _ = s.update(Message::CheckForUpdates);
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Err("boom".into()),
    });
    assert!(!s.update_checking);
    assert!(s.update_available.is_none());
    assert!(s.status.contains("failed"));
    // The silent startup probe reports only newer releases: up-to-date
    // and failed probes leave the status alone.
    let quiet = s.language.ready_status();
    s.status = quiet.clone();
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info(APP_VERSION, false)),
    });
    assert_eq!(s.status, quiet);
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Err("boom".into()),
    });
    assert_eq!(s.status, quiet);
    assert!(s.update_checked);
}

#[cfg(feature = "update")]
#[test]
fn update_modal_omits_defers_and_installs() {
    fn info(tag: &str) -> yutu_core::UpdateInfo {
        yutu_core::UpdateInfo {
            latest_tag: tag.into(),
            html_url: "https://example.test/r".into(),
            notes: "Highlights.".into(),
            newer: true,
        }
    }
    let mut s = test_state();
    // The silent startup probe arms the modal for a newer release.
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.9")),
    });
    assert!(s.pending_update.is_some());
    assert!(s.update_available.is_some());
    assert!(s.update_dialog().is_some());
    let _ = s.view();
    // "Later" dismisses for this session but keeps the settings offer.
    // Esc and scrim clicks never dismiss this modal (an explicit
    // action does).
    let _ = s.update(Message::CloseContext);
    assert!(s.pending_update.is_some());
    let _ = s.update(Message::NoOp);
    assert!(s.pending_update.is_some());
    let _ = s.update(Message::UpdateLater);
    assert!(s.pending_update.is_none());
    assert!(s.update_dialog().is_none());
    assert!(s.update_available.is_some());
    // "Omit" persists the skip and clears everything.
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.9")),
    });
    let _ = s.update(Message::UpdateOmitVersion);
    assert!(s.pending_update.is_none());
    assert!(s.update_available.is_none());
    assert_eq!(s.update_skipped_tag.as_deref(), Some("v9.9.9"));
    let store = LibraryStore::open(&s.library_path).unwrap();
    assert_eq!(
        store.get_pref("update_skipped_tag").unwrap().as_deref(),
        Some("v9.9.9")
    );
    // An omitted tag never re-arms the auto-modal (manual checks still
    // report it inline).
    let _ = s.update(Message::CheckForUpdates);
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.9")),
    });
    assert!(s.pending_update.is_none());
    assert!(s.update_available.is_some());
    // A different newer tag prompts again despite the omit.
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.10")),
    });
    assert!(s.pending_update.is_some());
    // "Update and restart" starts the download with modal progress.
    let _ = s.update(Message::UpdateAndRestart);
    assert_eq!(s.update_phase, UpdatePhase::Downloading);
    assert!(s.update_progress.is_some());
    let _ = s.view();
    // Progress lands; a stale generation from a dismissed modal drops.
    let _ = s.update(Message::UpdateProgress {
        gen: s.update_gen,
        downloaded: 10,
        total: Some(100),
    });
    assert_eq!(s.update_progress, Some((10, Some(100))));
    let _ = s.update(Message::UpdateLater);
    let _ = s.update(Message::UpdateProgress {
        gen: s.update_gen - 1,
        downloaded: 99,
        total: Some(100),
    });
    assert!(s.update_progress.is_none());
    // Download completion moves to installing; install failure reports
    // honestly without touching the app.
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.10")),
    });
    let _ = s.update(Message::UpdateAndRestart);
    let gen = s.update_gen;
    let _ = s.update(Message::UpdateDownloaded {
        gen,
        result: Err("net down".into()),
    });
    assert_eq!(s.update_phase, UpdatePhase::Idle);
    assert!(s.status.contains("failed"));
}

#[cfg(feature = "update")]
#[test]
fn arming_any_modal_clears_hovers() {
    let mut s = test_state();
    // Frozen highlights (cursor never exits the row the modal opens
    // over) plus the unfocus task clear everything behind dialogs.
    s.hovered_track = Some("m1".into());
    s.hovered_playlist = Some("u1".into());
    s.hovered_queue = Some(0);
    s.pl_hover = Some(0);
    let _ = s.update(Message::RequestDeletePlaylist("nope".into()));
    // Unknown id never arms: hovers stay (nothing opened).
    assert!(s.hovered_track.is_some());
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = s.user_playlists[0].id.clone();
    s.hovered_track = Some("m1".into());
    s.hovered_playlist = Some(pid.clone());
    let _ = s.update(Message::RequestDeletePlaylist(pid.clone()));
    assert!(s.pending_playlist_delete.is_some());
    assert!(s.hovered_track.is_none());
    assert!(s.hovered_playlist.is_none());
    assert!(s.hovered_queue.is_none());
    assert!(s.pl_hover.is_none());
    let _ = s.update(Message::CancelDeletePlaylist);
    s.hovered_track = Some("m1".into());
    let _ = s.update(Message::RequestRenamePlaylist(pid));
    assert!(s.pending_playlist_rename.is_some());
    assert!(s.hovered_track.is_none());
    let _ = s.view();
}

#[cfg(feature = "update")]
#[test]
fn hovers_stay_frozen_behind_dialogs() {
    fn info(tag: &str) -> yutu_core::UpdateInfo {
        yutu_core::UpdateInfo {
            latest_tag: tag.into(),
            html_url: "https://example.test/r".into(),
            notes: "".into(),
            newer: true,
        }
    }
    let mut s = test_state();
    // Arm the update modal, then hover like the cursor gliding over a
    // row behind the scrim: nothing highlights, nothing arms.
    let _ = s.update(Message::UpdateCheckCompleted {
        result: Ok(info("v9.9.9")),
    });
    assert!(s.pending_update.is_some());
    let _ = s.update(Message::TrackHover(Some("m1".into())));
    let _ = s.update(Message::PlaylistHover(Some("u1".into())));
    let _ = s.update(Message::PanelHover(0));
    let _ = s.update(Message::AlbumHover(Some("al1".into())));
    let _ = s.update(Message::RemoveHover(Some(0)));
    let _ = s.update(Message::PlActionHover(Some(0)));
    assert!(s.hovered_track.is_none());
    assert!(s.hovered_playlist.is_none());
    assert!(s.hovered_queue.is_none());
    assert!(s.hovered_album.is_none());
    assert!(s.hover_remove.is_none());
    assert!(s.pl_action_hover.is_none());
    assert!(s.pl_hover.is_none());
    // Sidebar latches freeze too (no glide behind the dialog).
    s.sidebar_pinned = false;
    s.sidebar_latched = false;
    let _ = s.update(Message::CursorMoved(iced::Point::new(10.0, 400.0)));
    assert!(!s.sidebar_latched);
    let _ = s.view();
    // Dismiss and the rows highlight again.
    let _ = s.update(Message::UpdateLater);
    let _ = s.update(Message::TrackHover(Some(track_hover_key("quick", "m1"))));
    assert_eq!(
        s.hovered_track.as_deref(),
        Some(track_hover_key("quick", "m1").as_str())
    );
}

#[test]
fn hover_keys_scope_rows_to_their_own_list() {
    // Same track id in two scopes keys differently (the format is the
    // contract: views build it, the detail derivation strips the
    // `playlist:` prefix).
    assert_eq!(track_hover_key("quick", "m1"), "quick:m1");
    assert_ne!(
        track_hover_key("quick", "m1"),
        track_hover_key("more", "m1")
    );
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = s.user_playlists[0].id.clone();
    for tid in ["m1", "m2"] {
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: tid.into(),
        });
    }
    s.route = Route::PlaylistDetail(pid.clone());
    // Playlist-scoped hover derives the ordered-view index.
    let _ = s.update(Message::TrackHover(Some(track_hover_key("playlist", "m2"))));
    assert_eq!(s.hovered_track.as_deref(), Some("playlist:m2"));
    assert_eq!(s.pl_hover, Some(1));
    // Another list's key for the same track is a different hover:
    // exiting with it never clears the armed one.
    let _ = s.update(Message::TrackHover(Some(track_hover_key("more", "m2"))));
    assert_eq!(s.hovered_track.as_deref(), Some("more:m2"));
    // ...and it never derives the detail index either.
    assert!(s.pl_hover.is_none());
    let _ = s.update(Message::TrackExit(track_hover_key("playlist", "m2")));
    assert_eq!(s.hovered_track.as_deref(), Some("more:m2"));
    let _ = s.update(Message::TrackExit(track_hover_key("more", "m2")));
    assert!(s.hovered_track.is_none());
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
    assert_eq!(s.route, Route::Home);
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
    // Closing the menu also drops hover state, so a stale hot
    // flag can never pin the next menu's panel open.
    assert!(!s.submenu_trigger_hot && !s.submenu_fly_hot);
    assert!(s.submenu_hot_at.is_none());
}

#[test]
fn playlist_submenu_hover_opens_and_grace_closes() {
    use std::time::{Duration, Instant};
    let mut s = test_state();
    let _ = s.update(Message::OpenContext("m1".into()));
    // Hover opens with no click.
    let _ = s.update(Message::SubmenuTriggerHot(Some("m1".into())));
    assert_eq!(s.submenu_playlists_for.as_deref(), Some("m1"));
    assert!(s.submenu_trigger_hot);
    // Leaving the toggle keeps it open inside the grace (the gap
    // transit exits one layer before entering the other).
    let _ = s.update(Message::SubmenuTriggerHot(None));
    let t0 = Instant::now();
    let _ = s.update(Message::Tick(t0 + Duration::from_millis(100)));
    assert_eq!(s.submenu_playlists_for.as_deref(), Some("m1"));
    // Reaching the panel re-arms the grace.
    let _ = s.update(Message::SubmenuFlyHot(true));
    let _ = s.update(Message::SubmenuFlyHot(false));
    let _ = s.update(Message::Tick(t0 + Duration::from_millis(200)));
    assert_eq!(s.submenu_playlists_for.as_deref(), Some("m1"));
    // Cold past the grace on both sides closes.
    let _ = s.update(Message::Tick(
        t0 + SUBMENU_HOVER_GRACE + Duration::from_millis(600),
    ));
    assert!(s.submenu_playlists_for.is_none());
    // A foreign id can never steal the open menu's flyout.
    let _ = s.update(Message::SubmenuTriggerHot(Some("m2".into())));
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
fn sanitize_filename_maps_like_the_lyrics_files() {
    assert_eq!(sanitize_filename("abcXYZ012"), "abcXYZ012");
    assert_eq!(sanitize_filename("a/b-c_d e"), "a_b_c_d_e");
    assert_eq!(sanitize_filename(""), "unknown");
    assert_eq!(lyrics_file_in("/tmp/x", "a/b"), "/tmp/x/a_b.json");
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
    // Starting a mix never leaves the current page.
    let _ = s.update(Message::StartRadio("m1".into()));
    assert_eq!(s.route, Route::Lyrics("m4".into()));
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
        track_id: "t2".into(),
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
    // Detail page renders the new rows with no panic.
    s.route = Route::PlaylistDetail("u1".into());
    let _ = s.view();
    // Past-the-end index reports honestly with no queue change.
    let _ = s.update(Message::PlayPlaylistAt {
        playlist_id: "u1".into(),
        index: 9,
        track_id: "missing".into(),
    });
    assert_eq!(s.queue.len(), 2);
    assert!(s.status.contains("nothing to play"));
    // Unknown playlist reports honestly too.
    let _ = s.update(Message::PlayPlaylistAt {
        playlist_id: "nope".into(),
        index: 0,
        track_id: "t0".into(),
    });
    assert!(s.status.contains("not found"));
}

#[test]
fn playlist_search_click_queues_the_full_playlist() {
    let mut s = test_state();
    let tracks: Vec<Track> = ["Alpha", "Bravo", "Charlie", "Delta"]
        .iter()
        .enumerate()
        .map(|(i, title)| mk_track(&format!("t{i}"), title, "Artist"))
        .collect();
    s.user_playlists.push(Playlist {
        id: "u1".into(),
        title: "Gym".into(),
        tracks,
    });
    s.navigate(Route::PlaylistDetail("u1".into()));
    let _ = s.update(Message::PlaylistFilterChanged("charl".into()));
    // Filtered page renders (match and no-match) with no panic.
    let _ = s.view();
    // A click in the filtered view queues as if there were no search:
    // the clicked song plus the rest of the full list after it.
    let _ = s.update(Message::PlayPlaylistAt {
        playlist_id: "u1".into(),
        index: 2,
        track_id: "t2".into(),
    });
    assert_eq!(
        s.queue
            .items()
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        vec!["t2".to_string(), "t3".to_string()]
    );
    let _ = s.update(Message::PlaylistFilterChanged("zzz".into()));
    let _ = s.view();
    // Leaving the page drops the search text.
    s.navigate(Route::History);
    assert!(s.playlist_filter.is_empty());
    s.playlist_filter = "x".into();
    s.go_back();
    assert!(s.playlist_filter.is_empty());
}

#[test]
fn playlist_sort_orders_tracks_all_picks() {
    let mut s = test_state();
    let mut t1 = mk_track("t1", "Banana", "Zebra");
    t1.album = Some("Zulu".into());
    let mut t2 = mk_track("t2", "Apple", "Alpha");
    t2.album = Some("Alpha".into());
    let mut t3 = mk_track("t3", "Cherry", "Mike");
    t3.album = Some("Mike".into());
    for t in [&t1, &t2, &t3] {
        s.catalog.insert(t.id.clone(), t.clone());
    }
    s.user_playlists.push(Playlist {
        id: "u1".into(),
        title: "Gym".into(),
        tracks: vec![t1.clone(), t2.clone(), t3.clone()],
    });
    s.playlist_added.insert(
        "u1".into(),
        [
            ("t1".to_string(), 10),
            ("t2".to_string(), 20),
            ("t3".to_string(), 30),
        ]
        .into_iter()
        .collect(),
    );
    let p = s.find_playlist("u1").expect("playlist");
    let ids = |v: Vec<Track>| v.into_iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::Manual)),
        vec!["t1", "t2", "t3"]
    );
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::OldestFirst)),
        vec!["t1", "t2", "t3"]
    );
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::NewestFirst)),
        vec!["t3", "t2", "t1"]
    );
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::Title)),
        vec!["t2", "t1", "t3"]
    );
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::Artist)),
        vec!["t2", "t3", "t1"]
    );
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::Album)),
        vec!["t2", "t3", "t1"]
    );
    s.like_id("t3");
    assert_eq!(
        ids(s.playlist_ordered_tracks_with(&p, PlaylistSort::LikedFirst)),
        vec!["t3", "t2", "t1"]
    );
    // Detail page renders every pick with no panic (picker + rows).
    s.route = Route::PlaylistDetail("u1".into());
    for sort in PlaylistSort::ALL {
        s.playlist_sorts.insert("u1".into(), sort);
        let _ = s.view();
    }
}

#[test]
fn playlist_sort_pick_persists_and_play_follows_it() {
    let mut s = test_state();
    let t1 = mk_track("t1", "Banana", "Zebra");
    let t2 = mk_track("t2", "Apple", "Alpha");
    for t in [&t1, &t2] {
        s.catalog.insert(t.id.clone(), t.clone());
    }
    s.user_playlists.push(Playlist {
        id: "u1".into(),
        title: "Gym".into(),
        tracks: vec![t1.clone(), t2.clone()],
    });
    s.playlist_added.insert(
        "u1".into(),
        [("t1".to_string(), 1), ("t2".to_string(), 2)]
            .into_iter()
            .collect(),
    );
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: "u1".into(),
        sort: PlaylistSort::Title,
    });
    assert_eq!(s.playlist_sort_cached("u1"), PlaylistSort::Title);
    let store = LibraryStore::open(&s.library_path).unwrap();
    assert_eq!(
        store.get_pref(&AppState::playlist_sort_key("u1")).unwrap(),
        Some("title".to_string())
    );
    s.route = Route::PlaylistDetail("u1".into());
    let _ = s.update(Message::PlayAll);
    assert_eq!(
        s.queue
            .items()
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec!["t2", "t1"]
    );
    let _ = s.update(Message::PlayPlaylistAt {
        playlist_id: "u1".into(),
        index: 1,
        track_id: "t1".into(),
    });
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("t1"));
}

#[test]
fn playlist_sort_menu_opens_picks_and_dismisses() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let sort = || SelectMenu::Sort("u1".into());
    // Opens like the other menus, closing them.
    let _ = s.update(Message::OpenPlaylistMenu("u1".into()));
    let _ = s.update(Message::OpenSelectMenu(sort()));
    assert_eq!(s.sort_menu, Some(sort()));
    assert!(s.pl_menu.is_none());
    // A pick persists and closes the menu.
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: "u1".into(),
        sort: PlaylistSort::Artist,
    });
    assert_eq!(s.playlist_sort_cached("u1"), PlaylistSort::Artist);
    assert!(s.sort_menu.is_none());
    // Esc closes an open sort menu.
    let _ = s.update(Message::OpenSelectMenu(sort()));
    assert!(s.sort_menu.is_some());
    let _ = s.update(Message::CloseContext);
    assert!(s.sort_menu.is_none());
    // An open sort menu freezes row hovers like the other menus.
    let _ = s.update(Message::OpenSelectMenu(sort()));
    let _ = s.update(Message::TrackHover(Some("t0".into())));
    assert!(s.hovered_track.is_none());
    assert!(s.pl_hover.is_none());
}

#[test]
fn settings_select_menus_open_pick_and_dismiss() {
    let mut s = test_state();
    for kind in [
        SelectMenu::Language,
        SelectMenu::LyricsProvider,
        SelectMenu::FontFamily,
    ] {
        let _ = s.update(Message::OpenPlaylistMenu("x".into()));
        let _ = s.update(Message::OpenSelectMenu(kind.clone()));
        assert_eq!(s.sort_menu, Some(kind.clone()));
        assert!(s.pl_menu.is_none());
        let _ = s.update(Message::CloseContext);
        assert!(s.sort_menu.is_none());
    }
    let _ = s.update(Message::OpenSelectMenu(SelectMenu::Language));
    let _ = s.update(Message::LanguageChanged(Language::parse("Español")));
    assert_eq!(s.language, Language::Spanish);
    assert!(s.sort_menu.is_none());
}

#[test]
fn playlist_manual_drag_reorders_and_persists() {
    let mut s = test_state();
    for t in ["t0", "t1", "t2"].iter().map(|id| mk_track(id, id, "A")) {
        s.catalog.insert(t.id.clone(), t.clone());
    }
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = "u1".to_string();
    for tid in ["t0", "t1", "t2"] {
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: tid.into(),
        });
    }
    assert_eq!(s.playlist_sort_cached(&pid), PlaylistSort::Manual);
    let _ = s.update(Message::PlDragStart {
        playlist_id: pid.clone(),
        index: 0,
    });
    let _ = s.update(Message::PlDragSlot(3));
    let _ = s.update(Message::DragEnd);
    assert!(s.status.contains("reordered"));
    assert_eq!(
        s.find_playlist(&pid)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec!["t1", "t2", "t0"]
    );
    let store = LibraryStore::open(&s.library_path).unwrap();
    let stored = store.user_playlists().unwrap();
    let stored_ids = stored
        .iter()
        .find(|p| p.id == pid)
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(stored_ids, vec!["t1", "t2", "t0"]);
    let added = store.playlist_added_map(&pid).unwrap();
    assert!(added["t0"] < added["t1"] && added["t1"] < added["t2"]);
    let p = s.find_playlist(&pid).unwrap();
    let oldest = s.playlist_ordered_tracks_with(&p, PlaylistSort::OldestFirst);
    assert_eq!(oldest.first().map(|t| t.id.as_str()), Some("t0"));
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: pid.clone(),
        sort: PlaylistSort::Title,
    });
    let _ = s.update(Message::PlDragStart {
        playlist_id: pid.clone(),
        index: 0,
    });
    assert!(s.pl_drag.is_none());
}

#[test]
fn playlist_manual_drag_needs_a_move_and_ignores_action_presses() {
    let mut s = test_state();
    for t in ["t0", "t1", "t2"].iter().map(|id| mk_track(id, id, "A")) {
        s.catalog.insert(t.id.clone(), t.clone());
    }
    let _ = s.update(Message::CreatePlaylist("Gym".into()));
    let pid = "u1".to_string();
    for tid in ["t0", "t1", "t2"] {
        let _ = s.update(Message::AddToPlaylist {
            playlist_id: pid.clone(),
            track_id: tid.into(),
        });
    }
    let order_of = |s: &AppState| {
        s.find_playlist(&pid)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>()
    };
    // Presses on the heart/ellipsis never arm (slipping off an action
    // must not become a reorder).
    let _ = s.update(Message::PlActionHover(Some(0)));
    let _ = s.update(Message::PlDragStart {
        playlist_id: pid.clone(),
        index: 0,
    });
    assert!(s.pl_drag.is_none());
    let _ = s.update(Message::PlActionExit(0));
    // A press released with no move never reorders (no slot latched,
    // drag never went live): the row click just plays instead.
    let before = order_of(&s);
    s.route = Route::PlaylistDetail(pid.clone());
    let _ = s.update(Message::TrackHover(Some(track_hover_key("playlist", "t0"))));
    let _ = s.update(Message::PlDragStart {
        playlist_id: pid.clone(),
        index: 0,
    });
    assert!(s.pl_drag.is_some());
    assert!(!s.pl_dragging());
    let _ = s.update(Message::DragEnd);
    assert_eq!(order_of(&s), before);
    assert!(!s.status.contains("reordered"));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("t0"));
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
    #[allow(unused_imports)]
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
fn search_playlist_opens_its_page_instead_of_playing() {
    let mut s = test_state();
    s.results.playlists.push(PlaylistHit {
        id: "PL7".into(),
        title: "Queen Mix".into(),
        author: "Fan".into(),
        detail: "3 songs".into(),
        thumbnail_url: None,
    });
    // Opening routes to the page and never touches playback.
    let _ = s.update(Message::OpenSearchPlaylist("PL7".into()));
    assert_eq!(s.route, Route::RemotePlaylist("PL7".into()));
    assert!(s.queue.is_empty());
    let _ = s.view();
    // Metadata survives a later search replacing the results.
    s.results.playlists.clear();
    assert_eq!(s.search_playlist_label("PL7"), "Queen Mix");
    // Tracks land in server order and render.
    let _ = s.update(Message::RemotePlaylistLoaded {
        id: "PL7".into(),
        result: Ok(vec![
            mk_track("q1", "Bohemian Rhapsody", "Queen"),
            mk_track("q2", "Somebody to Love", "Queen"),
            mk_track("q3", "Killer Queen", "Queen"),
        ]),
    });
    let ids: Vec<String> = s
        .remote_playlist_tracks("PL7")
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, ["q1", "q2", "q3"]);
    assert!(s.queue.is_empty());
    let _ = s.view();
    // Row click plays from that song on.
    let _ = s.update(RowPlay::RemotePlaylistAt("PL7".into()).message("q2", 2));
    let queued: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(queued, ["q2", "q3"]);
    // Play pill reuses the fetched list (whole playlist, no refetch).
    let _ = s.update(Message::PlaySearchPlaylist("PL7".into()));
    assert_eq!(s.queue.len(), 3);
}

#[test]
fn remote_playlist_failure_reports_on_its_page() {
    let mut s = test_state();
    let _ = s.update(Message::OpenSearchPlaylist("PLx".into()));
    s.remote_playlist_loading = Some("PLx".into());
    let _ = s.update(Message::RemotePlaylistLoaded {
        id: "PLx".into(),
        result: Err("offline".into()),
    });
    assert!(s.remote_playlist_loading.is_none());
    assert!(s.language.is_failure_line(&s.status));
    let _ = s.view();
}

#[test]
fn artist_album_sub_skips_empty_parts() {
    assert_eq!(
        AppState::artist_album_sub("Queen", Some("A Night At The Opera")),
        "Queen · A Night At The Opera"
    );
    assert_eq!(AppState::artist_album_sub("Queen", None), "Queen");
    assert_eq!(AppState::artist_album_sub("Queen", Some("  ")), "Queen");
    assert_eq!(AppState::artist_album_sub("", Some("Jazz")), "Jazz");
    assert_eq!(AppState::artist_album_sub("", None), "");
}

#[test]
fn artist_top_expand_and_reset() {
    let mut s = test_state();
    assert!(!s.artist_expanded);
    let _ = s.update(Message::ArtistTopExpand);
    assert!(s.artist_expanded);
    // Every page open collapses (cached list keeps the expand free).
    let _ = s.update(Message::OpenArtist("UCx".into()));
    assert!(!s.artist_expanded);
    assert!(matches!(s.route, Route::Artist(_)));
}

#[test]
fn row_play_maps_rows_to_messages() {
    // 1-based row numbers become 0-based suffix offsets.
    match RowPlay::Now.message("v1", 3) {
        Message::PlayNow(id) => assert_eq!(id, "v1"),
        _ => panic!("wrong message"),
    }
    match RowPlay::ArtistAt("UCq".into()).message("v1", 2) {
        Message::PlayArtistAt {
            artist_id,
            index,
            track_id,
        } => {
            assert_eq!(artist_id, "UCq");
            assert_eq!(index, 1);
            assert_eq!(track_id, "v1");
        }
        _ => panic!("wrong message"),
    }
    match RowPlay::AlbumAt("MPREa".into()).message("v1", 1) {
        Message::PlayAlbumAt {
            album_id,
            index,
            track_id,
        } => {
            assert_eq!(album_id, "MPREa");
            assert_eq!(index, 0);
            assert_eq!(track_id, "v1");
        }
        _ => panic!("wrong message"),
    }
}

#[test]
fn play_artist_at_queues_clicked_plus_suffix() {
    let mut s = test_state();
    for (i, title) in ["A Song", "B Song", "C Song"].iter().enumerate() {
        let mut t = mk_track(&format!("v{}", i + 1), title, "Queen");
        t.album = Some("Jazz".into());
        s.catalog.insert(t.id.clone(), t);
    }
    s.results.artists.push(Artist {
        id: "UCq".into(),
        name: "Queen".into(),
        thumbnail_url: None,
    });
    // Catalog fallback is alphabetical: skip(1) plays B, C.
    let _ = s.update(Message::PlayArtistAt {
        artist_id: "Queen".into(),
        index: 1,
        track_id: "v2".into(),
    });
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["v2", "v3"]);
    // Same-song no-op (app contract): clicking the live track's
    // own row never restarts audio (queue keeps its head).
    let _ = s.update(Message::PlayArtistAt {
        artist_id: "Queen".into(),
        index: 0,
        track_id: "v2".into(),
    });
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["v2", "v3"]);
    // A catalog insert between render and click (shifts the index)
    // still starts at the clicked id, not the shifted offset.
    let mut shifted = mk_track("v0", "0 Song", "Queen");
    shifted.album = Some("Jazz".into());
    s.catalog.insert(shifted.id.clone(), shifted);
    s.queue.clear();
    let _ = s.update(Message::PlayArtistAt {
        artist_id: "Queen".into(),
        index: 1,
        track_id: "v2".into(),
    });
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["v2", "v3"]);
    // Out of range with a missing id: status names it, queue kept.
    let _ = s.update(Message::PlayArtistAt {
        artist_id: "Queen".into(),
        index: 9,
        track_id: "gone".into(),
    });
    assert!(s.status.contains("nothing to play"), "{}", s.status);
    assert_eq!(s.queue.items().len(), 2);
}

#[test]
fn play_album_at_queues_clicked_plus_suffix() {
    let mut s = test_state();
    for (i, title) in ["A Song", "B Song", "C Song"].iter().enumerate() {
        let mut t = mk_track(&format!("v{}", i + 1), title, "Queen");
        t.album = Some("Jazz".into());
        s.catalog.insert(t.id.clone(), t);
    }
    let _ = s.update(Message::PlayAlbumAt {
        album_id: "Jazz".into(),
        index: 0,
        track_id: "v1".into(),
    });
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["v1", "v2", "v3"]);
}

#[test]
fn pick_helpers_match_exact_only() {
    let artists = vec![
        Artist {
            id: "UCq".into(),
            name: "Queen".into(),
            thumbnail_url: None,
        },
        Artist {
            id: "UCf".into(),
            name: "Queens of the Stone Age".into(),
            thumbnail_url: None,
        },
    ];
    assert_eq!(
        AppState::pick_artist_channel(&artists, "queen").as_deref(),
        Some("UCq")
    );
    assert_eq!(AppState::pick_artist_channel(&artists, "Que"), None);
    let albums = vec![
        Album {
            id: "MPREa".into(),
            title: "Greatest Hits".into(),
            artist: "Queen".into(),
            year: None,
            thumbnail_url: None,
        },
        Album {
            id: "MPREb".into(),
            title: "Greatest Hits".into(),
            artist: "ABBA".into(),
            year: None,
            thumbnail_url: None,
        },
    ];
    assert_eq!(
        AppState::pick_album_browse(&albums, "Greatest Hits", Some("ABBA")).as_deref(),
        Some("MPREb")
    );
    assert_eq!(
        AppState::pick_album_browse(&albums, "Greatest Hits", Some("Nobody")),
        None
    );
    assert_eq!(
        AppState::pick_album_browse(&albums, "Greatest Hits", None).as_deref(),
        Some("MPREa")
    );
    assert_eq!(AppState::pick_album_browse(&albums, "Unknown", None), None);
}

#[test]
fn artist_resolve_chains_into_top_fetch() {
    let mut s = test_state();
    s.artist_resolving = Some("Queen".into());
    let _ = s.update(Message::ArtistChannelResolved {
        route_id: "Queen".into(),
        name: "Queen".into(),
        result: Ok("UCq".into()),
    });
    assert_eq!(s.artist_resolving, None);
    assert_eq!(s.artist_alias.get("Queen").map(String::as_str), Some("UCq"));
    assert_eq!(s.artist_loading.as_deref(), Some("UCq"));
    // Failures name the failure only for the page still showing.
    s.route = Route::Artist("Queen".into());
    s.artist_resolving = Some("Queen".into());
    let _ = s.update(Message::ArtistChannelResolved {
        route_id: "Queen".into(),
        name: "Queen".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("failed"), "{}", s.status);
    s.status = "Ready.".into();
    s.route = Route::Home;
    s.artist_resolving = Some("Y".into());
    let _ = s.update(Message::ArtistChannelResolved {
        route_id: "Y".into(),
        name: "Y".into(),
        result: Err("offline".into()),
    });
    assert_eq!(s.status, "Ready.");
}

#[test]
fn album_resolve_chains_into_tracklist_fetch() {
    let mut s = test_state();
    s.album_resolving = Some("Test Album".into());
    let _ = s.update(Message::AlbumBrowseResolved {
        route_id: "Test Album".into(),
        title: "Test Album".into(),
        result: Ok("MPREt".into()),
    });
    assert_eq!(s.album_resolving, None);
    assert_eq!(
        s.album_alias.get("Test Album").map(String::as_str),
        Some("MPREt")
    );
    assert_eq!(s.album_loading.as_deref(), Some("MPREt"));
    // Failures name the failure only for the page still showing.
    s.route = Route::Album("Zed".into());
    s.album_resolving = Some("Zed".into());
    let _ = s.update(Message::AlbumBrowseResolved {
        route_id: "Zed".into(),
        title: "Zed".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("failed"), "{}", s.status);
    s.status = "Ready.".into();
    s.route = Route::Home;
    s.album_resolving = Some("Y".into());
    let _ = s.update(Message::AlbumBrowseResolved {
        route_id: "Y".into(),
        title: "Y".into(),
        result: Err("offline".into()),
    });
    assert_eq!(s.status, "Ready.");
}

#[test]
fn open_track_entries_funnel_to_detail_routes() {
    let mut s = test_state();
    let mut t = mk_track("v1", "Song", "Queen");
    t.album = Some("Test Album".into());
    s.catalog.insert("v1".into(), t);
    let _ = s.update(Message::OpenTrackArtist("v1".into()));
    assert!(matches!(s.route, Route::Artist(_)));
    let _ = s.update(Message::OpenTrackAlbum("v1".into()));
    assert!(matches!(s.route, Route::Album(_)));
}

#[test]
fn artist_full_arrival_replaces_head_and_marks_full() {
    let mut s = test_state();
    s.results.artists.push(Artist {
        id: "UCq".into(),
        name: "Queen".into(),
        thumbnail_url: None,
    });
    // Head fetch first (5-song typed shelf).
    let mut h1 = mk_track("v1", "One", "Queen");
    h1.plays = Some("1M plays".into());
    s.artist_loading = Some("UCq".into());
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCq".into(),
        result: Ok(vec![h1]),
    });
    assert!(!s.artist_full.contains("UCq"));
    assert_eq!(s.artist_display_tracks("UCq").len(), 1);
    // Loading state renders the button with a spinner beside it
    // (no text row, no panic) before the full shelf lands.
    s.route = Route::Artist("Queen".into());
    s.artist_expanded = true;
    s.artist_full_loading = Some("UCq".into());
    let _ = s.view();
    // The ticker runs for the fetch: the spinner turns one frame
    // per tick until the shelf lands.
    #[cfg(feature = "innertube")]
    {
        let f0 = s.spin_frame;
        let _ = s.update(Message::Animate);
        assert_eq!(s.spin_frame, f0 + 1);
    }
    // "Show more" fetch swaps in the full ranked shelf.
    let full: Vec<Track> = (0..8)
        .map(|i| {
            let mut t = mk_track(&format!("v{i}"), &format!("Song {i}"), "Queen");
            t.plays = Some("1M plays".into());
            t
        })
        .collect();
    let _ = s.update(Message::ArtistFullLoaded {
        channel_id: "UCq".into(),
        result: Ok(full),
    });
    assert_eq!(s.artist_full_loading, None);
    assert!(s.artist_full.contains("UCq"));
    let shown = s.artist_display_tracks("UCq");
    assert_eq!(shown.len(), 8);
    assert_eq!(shown[0].id, "v0");
    // A late head never clobbers the landed full shelf.
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCq".into(),
        result: Ok(vec![mk_track("vX", "Late", "Queen")]),
    });
    assert_eq!(s.artist_display_tracks("UCq").len(), 8);
}

#[test]
fn capped_detail_insert_evicts_oldest_first() {
    use std::collections::HashMap;
    let mut map = HashMap::new();
    let mut order = Vec::new();
    for i in 0..70 {
        AppState::capped_detail_insert(
            &mut map,
            &mut order,
            format!("k{i}"),
            vec![format!("v{i}")],
        );
    }
    assert_eq!(map.len(), 64);
    assert!(!map.contains_key("k0"));
    assert!(map.contains_key("k69"));
    // Re-insert of a live key neither evicts nor grows the order.
    let evicted =
        AppState::capped_detail_insert(&mut map, &mut order, "k69".into(), vec!["v".into()]);
    assert_eq!(evicted, None);
    assert_eq!(map.len(), 64);
    assert_eq!(order.len(), 64);
}

#[test]
fn detail_pending_only_while_fetch_in_flight() {
    let mut s = test_state();
    // Nothing cached, nothing in flight: catalog paints, no loading
    // row (offline and failed fetches never set the markers).
    assert!(!s.artist_list_pending("UCx"));
    assert!(!s.album_list_pending("MPREx"));
    s.results.artists.push(Artist {
        id: "UCx".into(),
        name: "X".into(),
        thumbnail_url: None,
    });
    s.artist_loading = Some("UCx".into());
    assert!(s.artist_list_pending("UCx"));
    // Cached: never pending (instant live page, no flash).
    s.artist_top.insert("UCx".into(), vec!["v1".into()]);
    s.catalog.insert("v1".into(), mk_track("v1", "Song", "X"));
    assert!(!s.artist_list_pending("UCx"));
    // Resolving marker: pending even before the channel is known.
    s.artist_top.clear();
    s.artist_loading = None;
    s.artist_resolving = Some("X".into());
    assert!(s.artist_list_pending("X"));
    // Album mirror.
    s.results.albums.push(Album {
        id: "MPREx".into(),
        title: "A".into(),
        artist: "X".into(),
        year: None,
        thumbnail_url: None,
    });
    s.album_loading = Some("MPREx".into());
    assert!(s.album_list_pending("MPREx"));
    s.album_loading = None;
    s.album_resolving = Some("A".into());
    assert!(s.album_list_pending("A"));
}

#[test]
fn song_meta_combines_plays_and_duration() {
    // Popup idiom ("213M plays · 3:45"); raw All results carry
    // no durations, so a zero one leaves the bare count (never
    // "· 0:00"). Count-less rows keep the bare duration (blank
    // when zero).
    assert_eq!(song_meta(Some("213M plays"), 225), "213M plays · 3:45");
    assert_eq!(song_meta(Some("1B plays"), 0), "1B plays");
    assert_eq!(song_meta(None, 225), "3:45");
    assert_eq!(song_meta(None, 0), "");
    assert_eq!(song_meta(Some("  "), 225), "3:45");
}

#[test]
fn hero_song_subtitle_skips_empty_parts() {
    assert_eq!(
        AppState::hero_song_subtitle(false, "Queen", Some("A Night at the Opera"), None),
        "Song · Queen · A Night at the Opera"
    );
    assert_eq!(
        AppState::hero_song_subtitle(false, "Queen", None, None),
        "Song · Queen"
    );
    assert_eq!(
        AppState::hero_song_subtitle(false, "Queen", Some("  "), None),
        "Song · Queen"
    );
    assert_eq!(
        AppState::hero_song_subtitle(true, "System Of A Down", None, Some("1.6B views")),
        "Video · System Of A Down · 1.6B views"
    );
    assert_eq!(
        AppState::hero_song_subtitle(
            false,
            "System Of A Down",
            Some("Toxicity"),
            Some("2.1B plays")
        ),
        "Song · System Of A Down · Toxicity · 2.1B plays"
    );
}

#[test]
fn search_rows_differentiate_songs_and_videos() {
    #[allow(unused_imports)]
    use yutu_core::{SearchAll, TopHit, TopKind};
    let mut song = mk_track("s1", "Chop Suey!", "System Of A Down");
    song.plays = Some("2.1B plays".into());
    assert_eq!(Language::English.track_kind(false), "Song");
    let mut video = mk_track("v1", "Chop Suey!", "System Of A Down");
    video.is_video = true;
    video.plays = Some("1.6B views".into());
    assert_eq!(Language::English.track_kind(true), "Video");
    assert_eq!(Language::Spanish.track_kind(false), "Canción");
    assert_eq!(Language::Spanish.track_kind(true), "Vídeo");
    // Rows and heroes build for both kinds (no panic); the hero
    // subtitle carries the kind plus the view/play count.
    let s = test_state();
    let _ = s.search_song_row(&song);
    let _ = s.search_song_row(&video);
    let _ = s.search_track_hero(&song);
    let _ = s.search_track_hero(&video);
    assert_eq!(
        AppState::hero_song_subtitle(
            video.is_video,
            &video.artist,
            video.album.as_deref(),
            video.plays.as_deref()
        ),
        "Video · System Of A Down · 1.6B views"
    );
    // Card-only top hits lend their count when the shelf row has
    // none (the recording exists only as the card).
    let mut s = test_state();
    s.all_results = SearchAll {
        top: Some(TopHit {
            kind: TopKind::Video,
            id: "v1".into(),
            title: "Chop Suey!".into(),
            artist: "System Of A Down".into(),
            thumbnail_url: None,
            plays: Some("1.6B views".into()),
        }),
        ..SearchAll::default()
    };
    let mut bare = mk_track("v1", "Chop Suey!", "System Of A Down");
    bare.is_video = true;
    assert_eq!(s.top_hit_plays("v1"), Some("1.6B views"));
    assert_eq!(s.top_hit_plays("other"), None);
    let _ = s.search_track_hero(&bare);
}

#[test]
fn search_rows_hover_and_collection_menus() {
    #[allow(unused_imports)]
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
    let _ = s.numbered_track_row("search", &t, 1, &RowPlay::Now);
    let _ = s.track_row("search", &t);
    // Play-count rows build with and without counts (search and
    // catalog hits carry "213M plays"; album, playlist, and radio
    // rows often carry none and must keep their layout).
    let mut played = mk_track("v2", "Toxicity", "System of a Down");
    played.plays = Some("213M plays".into());
    let _ = s.search_song_row(&played);
    let _ = s.numbered_track_row("search", &played, 2, &RowPlay::Now);
    let _ = s.track_row("search", &played);
    // Top-song rows build with and without the album subtitle.
    let mut with_album = mk_track("v3", "Bicycle Race", "Queen");
    with_album.album = Some("Jazz".into());
    with_album.plays = Some("76M plays".into());
    let _ = s.artist_top_row(&with_album, 5, "UCq");
    let _ = s.artist_top_row(&played, 6, "UCq");
    s.hovered_album = Some("al1".into());
    s.hovered_artist = Some("ar1".into());
    s.hovered_search_playlist = Some("PL1".into());
    s.hovered_track = Some("v1".into());
    let _ = s.search_album_row(&album);
    let _ = s.search_artist_row(&artist);
    let _ = s.search_playlist_row(&pl);
    let _ = s.search_song_row(&t);
    let _ = s.numbered_track_row("search", &t, 1, &RowPlay::Now);
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
    // Row variants share the source at the small display raster.
    let row = s.album_row_cover(&ytm).expect("row");
    assert!(row.contains("=w120-h120"), "{row}");
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
fn every_playback_path_logs_the_new_current_track() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.queue.push(mk_track("m3", "Three", "A"));
    assert!(s.queue.jump_to(0));
    // Next, QueueJump and Prev all start a load for the new
    // current track, so all of them land in history (no landing
    // or audio device needed: logging happens at load start).
    let _ = s.update(Message::Next);
    assert_eq!(s.history.last(), Some(&"m2".to_string()));
    let _ = s.update(Message::QueueJump(2));
    assert_eq!(s.history.last(), Some(&"m3".to_string()));
    let _ = s.update(Message::Prev);
    assert_eq!(s.history.last(), Some(&"m2".to_string()));
}

#[test]
fn played_tracks_persist_metadata_for_later_sessions() {
    // A logged play must survive a restart with its metadata, so
    // Recent renders days later with no search in between.
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir()
        .join(format!("yutu-recent-{}-{}.db", std::process::id(), n))
        .to_string_lossy()
        .into_owned();
    let (mut s, _) = AppState::with_library_path(path.clone());
    s.queue.push(mk_track("v1", "Song", "A"));
    s.queue.push(mk_track("v2", "Two", "B"));
    assert!(s.queue.jump_to(0));
    let _ = s.update(Message::Next);
    assert!(s.history.contains(&"v2".to_string()));
    let store = LibraryStore::open(&path).unwrap();
    let plays = store.history(10).unwrap();
    assert!(plays.iter().any(|e| e.track_id == "v2"));
    let cached = store.all_cached_tracks().unwrap();
    assert!(cached.iter().any(|t| t.id == "v2" && t.title == "Two"));
    drop(s);
    // Reopen with an empty in-memory catalog: the played track
    // still resolves from the cache.
    let (s2, _) = AppState::with_library_path(path);
    assert!(s2.history.contains(&"v2".to_string()));
    assert_eq!(
        s2.find_track("v2").map(|t| t.title),
        Some("Two".to_string())
    );
    // The timestamped log restores newest-first too.
    assert_eq!(
        s2.history_full.first().map(|e| e.track_id.as_str()),
        Some("v2")
    );
}

/// Fixed Friday noon (2026-10-23 12:00 UTC) for deterministic
/// section tests (mirrors the core bucketing tests).
const HIST_NOW: i64 = 1_792_756_800;

fn hist_entry(id: &str, played_at_unix: i64) -> HistoryEntry {
    HistoryEntry {
        track_id: id.into(),
        played_at_unix,
    }
}

#[test]
fn history_page_groups_by_recency() {
    let mut s = test_state();
    for (id, title) in [
        ("h1", "Today One"),
        ("h2", "Today Two"),
        ("h3", "Yesterday Hit"),
        ("h4", "Week Song"),
        ("h5", "Week Opener"),
        ("h6", "Month Tune"),
        ("h7", "Oldie"),
    ] {
        let mut t = mk_track(id, title, "A");
        t.duration_secs = 200;
        s.catalog.insert(id.into(), t);
    }
    let day = 86_400;
    s.history_full = vec![
        hist_entry("h1", HIST_NOW),
        hist_entry("h2", HIST_NOW - 3_600),
        hist_entry("h3", HIST_NOW - day),
        hist_entry("h4", HIST_NOW - 2 * day),
        hist_entry("h5", HIST_NOW - 4 * day),
        hist_entry("h6", HIST_NOW - 7 * day),
        hist_entry("h7", HIST_NOW - 40 * day),
    ];
    let (groups, total) = s.history_groups(HIST_NOW, usize::MAX);
    assert_eq!(total, 7);
    let headers: Vec<&str> = groups.iter().map(|(h, _)| h.as_str()).collect();
    assert_eq!(
        headers,
        vec![
            "Today",
            "Yesterday",
            "This week",
            "This month",
            "September 2026"
        ]
    );
    let counts: Vec<usize> = groups.iter().map(|(_, rows)| rows.len()).collect();
    assert_eq!(counts, vec![2, 1, 2, 1, 1]);
    // Rows keep their track identity in newest-first order.
    let ids: Vec<&str> = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|t| t.id.as_str()))
        .collect();
    assert_eq!(ids, vec!["h1", "h2", "h3", "h4", "h5", "h6", "h7"]);
}

#[test]
fn history_hover_is_per_row_not_per_track() {
    // Repeat plays share a track id: hovering one row must not
    // highlight its twin (id-keyed hover lit them both up).
    let mut s = test_state();
    let mut t = mk_track("h1", "Today One", "A");
    t.duration_secs = 200;
    s.catalog.insert("h1".into(), t);
    let day = 86_400;
    s.history_full = vec![hist_entry("h1", HIST_NOW), hist_entry("h1", HIST_NOW - day)];
    let _ = s.update(Message::HistoryHover(Some(1)));
    assert_eq!(s.hovered_history, Some(1));
    assert_ne!(s.hovered_history, Some(0));
    // Exit clears, and a filter change drops a stale index.
    let _ = s.update(Message::HistoryHover(None));
    assert_eq!(s.hovered_history, None);
    let _ = s.update(Message::HistoryHover(Some(0)));
    let _ = s.update(Message::HistoryFilterChanged("one".into()));
    assert_eq!(s.hovered_history, None);
    // The rows render with the per-row hover wired.
    s.history_filter = String::new();
    let _ = s.view();
}

#[test]
fn home_song_rows_hover_by_track_and_render() {
    // Home rows follow the app-wide row pattern: art and title
    // both play (no separate play button), hovered via TrackHover.
    let mut s = test_state();
    for (id, title) in [("q1", "One"), ("q2", "Two")] {
        let mut t = mk_track(id, title, "A");
        t.thumbnail_url = Some("http://art".into());
        s.catalog.insert(id.into(), t);
    }
    s.history_full = vec![hist_entry("q1", HIST_NOW), hist_entry("q2", HIST_NOW - 1)];
    s.refresh_home_shelves();
    s.route = Route::Home;
    let _ = s.view();
    let _ = s.update(Message::TrackHover(Some("q1".into())));
    assert_eq!(s.hovered_track.as_deref(), Some("q1"));
    let _ = s.update(Message::TrackHover(None));
    assert_eq!(s.hovered_track, None);
}

#[test]
fn history_search_narrows_to_matching_rows() {
    let mut s = test_state();
    for (id, title) in [("h1", "Today One"), ("h2", "Week Two")] {
        let mut t = mk_track(id, title, "A");
        t.duration_secs = 200;
        s.catalog.insert(id.into(), t);
    }
    let day = 86_400;
    s.history_full = vec![
        hist_entry("h1", HIST_NOW),
        hist_entry("h2", HIST_NOW - 2 * day),
    ];
    let _ = s.update(Message::HistoryFilterChanged("two".into()));
    let (groups, total) = s.history_groups(HIST_NOW, usize::MAX);
    assert_eq!(total, 1);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].0, "This week");
    assert_eq!(groups[0].1.len(), 1);
    assert_eq!(groups[0].1[0].id, "h2");
    // Clearing the filter restores every section.
    let _ = s.update(Message::HistoryFilterChanged(String::new()));
    assert_eq!(s.history_groups(HIST_NOW, usize::MAX).0.len(), 2);
}

#[test]
fn home_shelves_come_from_the_memo_not_live_scans() {
    let mut s = test_state();
    for (id, title, artist) in [("q1", "One", "A"), ("q2", "Two", "A")] {
        let mut t = mk_track(id, title, artist);
        t.thumbnail_url = Some("http://art".into());
        t.album = Some("Alpha".into());
        s.catalog.insert(id.into(), t);
        s.history_full.push(hist_entry(id, HIST_NOW));
    }
    // Stale until refreshed: views never scan the catalog.
    let (groups, _) = s.history_groups(HIST_NOW, usize::MAX);
    assert!(!groups.is_empty());
    assert!(s.home_shelves.quick.is_empty());
    s.refresh_home_shelves();
    assert_eq!(s.home_shelves.quick.len(), 2);
    assert_eq!(s.home_shelves.albums.len(), 1);
    assert_eq!(s.home_shelves.more_artist.as_deref(), Some("A"));
    assert_eq!(s.home_shelves.more.len(), 2);
    // Fresh plays invalidate through the next refresh, not the view.
    s.history_full.insert(0, hist_entry("q2", HIST_NOW + 10));
    assert_eq!(s.home_shelves.quick[0].id, "q1");
    s.refresh_home_shelves();
    assert_eq!(s.home_shelves.quick[0].id, "q2");
    s.route = Route::Home;
    let _ = s.view();
}

#[test]
fn home_library_strip_caps_at_twelve_cards() {
    let mut s = test_state();
    for i in 0..30 {
        s.user_playlists.push(yutu_core::Playlist {
            id: format!("u{i}"),
            title: format!("List {i}"),
            tracks: vec![mk_track(&format!("t{i}"), "Title", "A")],
        });
    }
    // Empty playlists never make cards, and the strip stops at the
    // cap no matter how large the library grows.
    s.user_playlists.push(yutu_core::Playlist {
        id: "empty".into(),
        title: "Empty".into(),
        tracks: Vec::new(),
    });
    assert_eq!(s.home_strip_playlists().len(), 12);
}
#[test]
fn history_page_renders_with_entries_and_filter() {
    let mut s = test_state();
    s.route = Route::History;
    // Empty state renders without panicking.
    let _ = s.view();
    let mut t = mk_track("h1", "Today One", "A");
    t.duration_secs = 200;
    s.catalog.insert("h1".into(), t);
    s.history_full = vec![hist_entry("h1", HIST_NOW)];
    let _ = s.view();
    s.history_filter = "zzz-no-match".into();
    let _ = s.view();
}

#[test]
fn history_page_caps_rendered_rows_with_show_more() {
    let mut s = test_state();
    for i in 0..500 {
        let id = format!("h{i}");
        s.catalog.insert(id.clone(), mk_track(&id, "Title", "A"));
        s.history_full.push(hist_entry(&id, HIST_NOW - i as i64));
    }
    // Counting scans everything borrow-only; only the window
    // resolves (clones) tracks.
    let (groups, total) = s.history_groups(HIST_NOW, s.history_limit);
    assert_eq!(total, 500);
    let shown: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
    assert_eq!(shown, HISTORY_PAGE_SIZE);
    // "Show more" pages the window forward; the filter resets it.
    let _ = s.update(Message::HistoryMore);
    assert_eq!(s.history_limit, 2 * HISTORY_PAGE_SIZE);
    let (groups, _) = s.history_groups(HIST_NOW, s.history_limit);
    let shown: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
    assert_eq!(shown, 2 * HISTORY_PAGE_SIZE);
    let _ = s.update(Message::HistoryFilterChanged("title".into()));
    assert_eq!(s.history_limit, HISTORY_PAGE_SIZE);
}

#[test]
fn fps_ema_glides_without_spikes() {
    // First sample lands on the instant rate.
    assert!((AppState::fps_ema(0.0, 1.0 / 60.0) - 60.0).abs() < 0.01);
    // Later samples blend 20% toward the instant rate.
    assert!((AppState::fps_ema(60.0, 1.0 / 30.0) - 54.0).abs() < 0.01);
    // Bad intervals keep the previous value (never NaN, never 0).
    assert_eq!(AppState::fps_ema(60.0, 0.0), 60.0);
    assert_eq!(AppState::fps_ema(60.0, -1.0), 60.0);
    assert_eq!(AppState::fps_ema(60.0, f32::NAN), 60.0);
    assert!(AppState::fps_ema(60.0, f32::INFINITY) > 0.0);
    // Timer noise floors at ~1000 fps instead of exploding.
    assert!((AppState::fps_ema(0.0, 0.000_001) - 1000.0).abs() < 1.0);
}

#[test]
fn frame_messages_feed_the_meter() {
    use std::time::{Duration, Instant};
    let mut s = test_state();
    assert_eq!(s.fps, 0.0);
    let t0 = Instant::now();
    let _ = s.update(Message::Frame(t0));
    assert_eq!(s.fps, 0.0);
    assert_eq!(s.last_frame, Some(t0));
    let _ = s.update(Message::Frame(t0 + Duration::from_millis(16)));
    assert!((s.fps - 62.5).abs() < 1.0, "got {}", s.fps);
}

/// Shelf track with album + art (recommended-album groups need
/// both; unresolvable rows are skipped by the shelves).
fn shelf_track(id: &str, title: &str, artist: &str, album: Option<&str>) -> Track {
    Track {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        album: album.map(str::to_string),
        duration_secs: 200,
        thumbnail_url: Some(format!("https://example.invalid/{id}.jpg")),
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    }
}

#[test]
fn quick_picks_rank_repeats_then_recency() {
    let mut s = test_state();
    for (id, title) in [("q1", "One"), ("q2", "Two"), ("q3", "Three")] {
        s.catalog.insert(id.into(), mk_track(id, title, "A"));
    }
    // q1 x2, q2 x1 (newer), q3 x1 (older): repeats first, then
    // the most recent single play wins the tie.
    s.history_full = vec![
        hist_entry("q2", HIST_NOW),
        hist_entry("q1", HIST_NOW - 1),
        hist_entry("q1", HIST_NOW - 2),
        hist_entry("q3", HIST_NOW - 3),
    ];
    let ids: Vec<String> = s
        .quick_pick_tracks(8)
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["q1", "q2", "q3"]);
    // Disliked tracks never headline, even on repeats.
    s.disliked.insert("q1".into());
    let ids: Vec<String> = s
        .quick_pick_tracks(8)
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["q2", "q3"]);
}

#[test]
fn top_artist_and_more_from_track_plays() {
    let mut s = test_state();
    for (id, title, artist) in [
        ("x1", "X One", "X"),
        ("x2", "X Two", "X"),
        ("y1", "Y One", "Y"),
    ] {
        s.catalog.insert(id.into(), mk_track(id, title, artist));
    }
    s.history_full = vec![
        hist_entry("x1", HIST_NOW),
        hist_entry("y1", HIST_NOW - 1),
        hist_entry("x2", HIST_NOW - 2),
        hist_entry("x1", HIST_NOW - 3),
    ];
    assert_eq!(s.top_artist().as_deref(), Some("X"));
    // Most-played first; catalog order breaks the rest.
    let ids: Vec<String> = s
        .more_from_artist("X", 8)
        .iter()
        .map(|t| t.id.clone())
        .collect();
    assert_eq!(ids, vec!["x1", "x2"]);
    assert!(s.more_from_artist("Nobody", 8).is_empty());
}

#[test]
fn recommended_albums_group_and_score_by_plays() {
    let mut s = test_state();
    for (id, title, artist, album) in [
        ("a1", "A One", "X", Some("Alpha")),
        ("a2", "A Two", "X", Some("Alpha")),
        ("b1", "B One", "Y", Some("Beta")),
        ("c1", "C One", "Z", None),
    ] {
        let mut t = shelf_track(id, title, artist, album);
        // One artless row: groups need artwork to render.
        if id == "b1" {
            t.thumbnail_url = None;
        }
        s.catalog.insert(id.into(), t);
    }
    s.history_full = vec![
        hist_entry("a1", HIST_NOW),
        hist_entry("a1", HIST_NOW - 1),
        hist_entry("b1", HIST_NOW - 2),
    ];
    let groups = s.recommended_albums(10);
    // Alpha (2 plays) first; the artless Beta and the album-less
    // row never make a card shelf.
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].title, "Alpha");
    assert_eq!(groups[0].artist, "X");
    assert_eq!(groups[0].plays, 2);
    let ids: Vec<&str> = groups[0].tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["a1", "a2"]);
}

#[test]
fn home_shelf_actions_queue_and_log() {
    let mut s = test_state();
    for (id, title, artist, album) in [
        ("q1", "One", "A", Some("Alpha")),
        ("q2", "Two", "A", Some("Alpha")),
        ("q3", "Three", "B", None),
    ] {
        s.catalog
            .insert(id.into(), shelf_track(id, title, artist, album));
    }
    s.history_full = vec![
        hist_entry("q1", HIST_NOW),
        hist_entry("q1", HIST_NOW - 1),
        hist_entry("q2", HIST_NOW - 2),
        hist_entry("q3", HIST_NOW - 3),
    ];
    // Quick picks play the head in rank order and log it.
    let _ = s.update(Message::PlayQuickPicks);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("q1"));
    assert!(s.history.contains(&"q1".to_string()));
    // Album cards queue that album's tracks in id order.
    let _ = s.update(Message::PlayHomeAlbum {
        album: "Alpha".into(),
        artist: "A".into(),
    });
    assert_eq!(s.queue.len(), 3 + 2);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("q1"));
    // Unknown albums report honestly instead of queueing air.
    let _ = s.update(Message::PlayHomeAlbum {
        album: "Ghost".into(),
        artist: "Nobody".into(),
    });
    assert!(s.status.contains("not cached"));
}

#[test]
fn home_shelves_render_with_data() {
    let mut s = test_state();
    s.user_playlists.push(yutu_core::Playlist {
        id: "u1".into(),
        title: "Gym".into(),
        tracks: vec![mk_track("q1", "One", "A")],
    });
    for (id, title, artist, album) in [
        ("q1", "One", "A", Some("Alpha")),
        ("q2", "Two", "A", Some("Alpha")),
    ] {
        s.catalog
            .insert(id.into(), shelf_track(id, title, artist, album));
    }
    s.history_full = vec![hist_entry("q1", HIST_NOW), hist_entry("q2", HIST_NOW - 1)];
    s.route = Route::Home;
    let _ = s.view();
}

#[test]
fn home_cards_right_click_menus() {
    let mut s = test_state();
    for (id, title, artist, album) in [
        ("h1", "One", "A", Some("Alpha")),
        ("h2", "Two", "A", Some("Alpha")),
    ] {
        s.catalog
            .insert(id.into(), shelf_track(id, title, artist, album));
    }
    s.user_playlists.push(yutu_core::Playlist {
        id: "u1".into(),
        title: "Gym".into(),
        tracks: vec![mk_track("h1", "One", "A"), mk_track("h2", "Two", "A")],
    });
    s.route = Route::Home;
    let _ = s.view();
    // Library card right-click opens the user playlist menu.
    let _ = s.update(Message::OpenPlaylistMenu("u1".into()));
    assert_eq!(s.pl_menu.as_deref(), Some("u1"));
    assert!(s.context_menu.is_none() && s.coll_menu.is_none());
    let _ = s.view();
    let _ = s.update(Message::CloseContext);
    assert!(s.pl_menu.is_none());
    // Recommended-album card right-click opens the home album menu.
    let _ = s.update(Message::OpenHomeAlbumMenu {
        album: "Alpha".into(),
        artist: "A".into(),
    });
    assert_eq!(
        s.coll_menu,
        Some(CollectionMenu::HomeAlbum {
            album: "Alpha".into(),
            artist: "A".into(),
        })
    );
    let _ = s.view();
    // Queue appends the album without hijacking playback.
    let _ = s.update(Message::QueueHomeAlbum {
        album: "Alpha".into(),
        artist: "A".into(),
    });
    assert!(s.coll_menu.is_none());
    assert_eq!(s.queue.len(), 2);
    // Shuffle plays the album fresh and shuffled.
    let _ = s.update(Message::ShuffleHomeAlbum {
        album: "Alpha".into(),
        artist: "A".into(),
    });
    assert_eq!(s.queue.len(), 2);
    assert!(s.queue.shuffle);
    // Unknown albums never open a scrim-less modal.
    let _ = s.update(Message::OpenHomeAlbumMenu {
        album: "Ghost".into(),
        artist: "Nobody".into(),
    });
    assert!(s.coll_menu.is_none());
    assert!(s.status.contains("not cached"));
    let _ = s.update(Message::QueueHomeAlbum {
        album: "Ghost".into(),
        artist: "Nobody".into(),
    });
    assert!(s.status.contains("not cached"));
}

#[test]
fn language_parse_labels_and_locale() {
    assert_eq!(Language::parse("es"), Language::Spanish);
    assert_eq!(Language::parse(" Español "), Language::Spanish);
    assert_eq!(Language::parse("es-ES"), Language::Spanish);
    assert_eq!(Language::parse("en"), Language::English);
    assert_eq!(Language::parse("xx"), Language::English);
    assert_eq!(Language::parse(""), Language::English);
    assert_eq!(Language::English.stored(), "en");
    assert_eq!(Language::Spanish.stored(), "es");
    assert_eq!(Language::English.label(), "English");
    assert_eq!(Language::Spanish.label(), "Español");
    assert_eq!(
        Language::from_locale_value("es_ES.UTF-8"),
        Language::Spanish
    );
    assert_eq!(Language::from_locale_value("es:en"), Language::Spanish);
    assert_eq!(
        Language::from_locale_value("en_US.UTF-8"),
        Language::English
    );
    assert_eq!(Language::from_locale_value("C.UTF-8"), Language::English);
    assert_eq!(Language::from_locale_value(""), Language::English);
}

#[test]
fn language_selector_persists_and_renders_spanish() {
    let mut s = test_state();
    assert_eq!(s.language, Language::English);
    let _ = s.update(Message::LanguageChanged(Language::Spanish));
    assert_eq!(s.language, Language::Spanish);
    let store = LibraryStore::open(&s.library_path).unwrap();
    assert_eq!(store.get_pref("language").unwrap().as_deref(), Some("es"));
    // The stored pick wins over the system default on reload.
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(again.language, Language::Spanish);
    // Both languages render the Home and Settings pages with no panic.
    for route in [Route::Home, Route::Settings] {
        s.route = route.clone();
        let _ = s.view();
    }
    assert!(s.language.settings_title() != Language::English.settings_title());
    // Back to English persists too.
    let _ = s.update(Message::LanguageChanged(Language::English));
    let store = LibraryStore::open(&s.library_path).unwrap();
    assert_eq!(store.get_pref("language").unwrap().as_deref(), Some("en"));
}

#[test]
fn about_section_renders_version_in_both_languages() {
    for lang in [Language::English, Language::Spanish] {
        for s in [
            lang.about_section(),
            lang.about_version(),
            lang.about_license(),
            lang.about_source(),
            lang.about_notices(),
            lang.about_backend(),
        ] {
            assert!(!s.is_empty(), "blank about string");
        }
    }
    assert!(Language::English.about_version().contains(APP_VERSION));
    assert!(Language::Spanish.about_version().contains(APP_VERSION));
    assert_ne!(
        Language::English.about_section(),
        Language::Spanish.about_section()
    );
    assert!(GITHUB_URL.starts_with("https://github.com/"));
}

#[test]
fn failure_filter_matches_in_both_languages() {
    use yutu_core::HistorySection as S;
    let en = Language::English;
    let es = Language::Spanish;
    // (english line, spanish line, surfaces in release)
    // `mut` only when the innertube-only import case below compiles in.
    #[cfg_attr(not(feature = "innertube"), allow(unused_mut))]
    let mut cases: Vec<(String, String, bool)> = vec![
        (en.fail_search(), es.fail_search(), true),
        (en.fail_sync(), es.fail_sync(), true),
        (
            en.fail_youtube_op("like", "v1"),
            es.fail_youtube_op("like", "v1"),
            true,
        ),
        (
            en.fail_created_local("Gym"),
            es.fail_created_local("Gym"),
            true,
        ),
        (
            en.playlist_fetch_failed("Gym", "boom", 1, 2),
            es.playlist_fetch_failed("Gym", "boom", 1, 2),
            true,
        ),
        // Honest non-failures stay silent in both.
        (en.album_not_cached("A"), es.album_not_cached("A"), false),
        (en.result_gone(), es.result_gone(), false),
        (en.favorites_locked(), es.favorites_locked(), false),
        (en.playlist_locked("u1"), es.playlist_locked("u1"), false),
        (en.cookie_cleared(), es.cookie_cleared(), false),
        (en.no_lyrics(), es.no_lyrics(), false),
        (en.no_audio_device(), es.no_audio_device(), false),
        (
            en.fail_kept_locally("add x"),
            es.fail_kept_locally("add x"),
            false,
        ),
        (en.fail_export_delete(), es.fail_export_delete(), false),
        (en.queued_tracks("Q", 2), es.queued_tracks("Q", 2), false),
        (
            en.history_section(&S::Older {
                year: 2026,
                month: 9,
            }),
            es.history_section(&S::Older {
                year: 2026,
                month: 9,
            }),
            false,
        ),
    ];
    #[cfg(feature = "innertube")]
    cases.push((
        en.import_done_lists_errors(crate::ImportKind::Sync, 3, 1, 1, 2),
        es.import_done_lists_errors(crate::ImportKind::Sync, 3, 1, 1, 2),
        true,
    ));
    for (en_line, es_line, surfaces) in cases {
        assert_eq!(en.is_failure_line(&en_line), surfaces, "{en_line}");
        assert_eq!(es.is_failure_line(&es_line), surfaces, "{es_line}");
    }
    // Spanish month names render in words, English as-is.
    assert_eq!(
        es.history_section(&S::Older {
            year: 2026,
            month: 9
        }),
        "septiembre de 2026"
    );
    assert_eq!(
        en.history_section(&S::Older {
            year: 2026,
            month: 9
        }),
        "September 2026"
    );
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
    // Reopen: queue, index, position and modes come back paused,
    // but shuffle always starts off (order kept as saved). With an
    // audio backend the restored track starts buffering paused
    // (autoload); without one it waits for the first play.
    let (s2, _) = AppState::with_library_path(path);
    assert_eq!(s2.queue.len(), 5);
    assert_eq!(s2.queue.position(), Some(2));
    assert_eq!(s2.queue.current().map(|t| t.id.as_str()), Some("m3"));
    assert!((s2.position_secs - 42.0).abs() < 1e-6);
    assert!(!s2.queue.shuffle);
    assert_eq!(s2.queue.repeat, yutu_player::RepeatMode::All);
    assert!((s2.volume - 0.5).abs() < f32::EPSILON);
    assert!(s2.paused);
    if s2.audio.is_some() {
        assert!(!s2.restored_pending);
        assert!(s2.autoload);
        assert_eq!(s2.loading_track.as_deref(), Some("m3"));
        assert_eq!(s2.pending_resume, Some(42.0));
        assert!(s2.status.contains("Loading"));
    } else {
        assert!(s2.restored_pending);
        assert!(s2.status.contains("Restored queue"));
    }
}

// Offline builds never start a mix (StartRadio only reports).
#[cfg(feature = "innertube")]
#[test]
fn mix_of_stopped_current_song_starts_it() {
    let mut s = test_state();
    let _ = s.update(Message::PlayNow("m1".into()));
    // Stopped: nothing loading, not paused, no live backend file.
    s.loading_track = None;
    s.paused = false;
    let _ = s.update(Message::StartRadio("m1".into()));
    let gen = s.mix_gen;
    let _ = s.update(Message::MixReady {
        gen,
        seed: "m1".into(),
        result: Ok(vec![
            mk_track("m1", "Seed Song", "Seed Artist"),
            mk_track("v9", "Come Together", "The Beatles"),
        ]),
    });
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    assert_eq!(s.queue.len(), 2);
}

#[test]
fn playing_from_persists_across_restart() {
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
    s.playing_from = Some("Mix of Song - Artist".into());
    s.persist_queue();
    drop(s);
    let (s2, _) = AppState::with_library_path(path);
    assert_eq!(s2.playing_from.as_deref(), Some("Mix of Song - Artist"));
}

#[test]
fn fit_lines_truncates_past_the_line_budget() {
    let short = "Mix of Song - Artist";
    assert_eq!(fit_lines(short, 2000.0, 14.0, 2), short);
    let long = "Mix of 21 de septiembre - El Cuarteto de Nos ".repeat(6);
    let fitted = fit_lines(&long, 220.0, 14.0, 2);
    assert!(fitted.ends_with('…'), "{fitted}");
    assert!(fitted.chars().count() < long.chars().count());
    // Unknown widths (first layout pass) leave the text alone.
    assert_eq!(fit_lines(short, 0.0, 14.0, 2), short);
}

#[test]
fn muted_session_restores_silenced() {
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
    s.volume = 0.5;
    s.muted = true;
    s.persist_queue();
    drop(s);
    // Reopen: the level comes back with output still silenced.
    let (s2, _) = AppState::with_library_path(path);
    assert!((s2.volume - 0.5).abs() < f32::EPSILON);
    assert!(s2.muted);
    let _ = s2.mute_button();
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
            muted: false,
            playing_from: None,
        })
        .unwrap();
    let (s, _) = AppState::with_library_path(path);
    assert!(s.queue.is_empty());
    assert!(!s.restored_pending);
    assert_eq!(s.status, "Ready: search for music.");
}

#[test]
fn window_title_names_the_live_song() {
    let mut s = test_state();
    assert_eq!(s.window_title(), "yutu-mux");
    s.queue.push(mk_track("v1", "Song", "A"));
    assert_eq!(s.window_title(), "Song · A");
    // Artistless rows fall back to the bare title (no dangling
    // separator); paused keeps naming the song.
    s.queue.push(mk_track("v2", "Solo", ""));
    assert!(s.queue.jump_to(1));
    assert_eq!(s.window_title(), "Solo");
    s.paused = true;
    assert_eq!(s.window_title(), "Solo");
    // Padded or empty titles never leak whitespace or dangling
    // separators into the title bar.
    s.queue.push(mk_track("v3", "  Spaced  ", "  B  "));
    assert!(s.queue.jump_to(2));
    assert_eq!(s.window_title(), "Spaced · B");
    s.queue.push(mk_track("v4", "", "Nobody"));
    assert!(s.queue.jump_to(3));
    assert_eq!(s.window_title(), "yutu-mux");
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
    // The display stays on the seek target through the load (it must
    // never flash 00:00 between the press and the landing).
    assert!((s.position_secs - 42.0).abs() < f64::EPSILON);
    // Failure drops the pending seek instead of firing it later.
    let _ = s.update(Message::StreamResolved {
        video_id: "v1".into(),
        result: Err("offline".into()),
    });
    assert_eq!(s.pending_resume, None);
}

#[test]
fn seek_before_first_play_survives_the_load_without_flashing_zero() {
    let mut s = test_state();
    s.queue.push(mk_track("v1", "Song", "A"));
    s.position_secs = 10.0;
    s.restored_pending = true;
    s.paused = true;
    // A seek made before the first play (queued: no file yet) is an
    // explicit target, honored even under the 5s resume threshold.
    s.pending_seek = Some(43.0);
    s.position_secs = 43.0;
    let _ = s.update(Message::TogglePause);
    assert_eq!(s.loading_track.as_deref(), Some("v1"));
    assert!(!s.restored_pending);
    assert!((s.position_secs - 43.0).abs() < f64::EPSILON);
    assert_eq!(s.pending_seek, Some(43.0));
    assert_eq!(s.pending_resume, None);
}

#[test]
fn autoload_buffers_the_restored_track_paused() {
    let mut s = test_state();
    if s.audio.is_none() {
        eprintln!("SKIP: no audio backend in this environment");
        return;
    }
    s.queue.push(mk_track("v1", "Song", "A"));
    s.position_secs = 42.0;
    s.paused = true;
    let _ = s.autoload_current_task();
    // Buffering, display untouched, landing armed paused.
    assert_eq!(s.loading_track.as_deref(), Some("v1"));
    assert!((s.position_secs - 42.0).abs() < f64::EPSILON);
    assert!(s.paused);
    assert!(s.autoload);
    assert_eq!(s.pending_resume, Some(42.0));
    assert!(!s.restored_pending);
    // Buffering is not a play: nothing is logged (the track is
    // already the history tail from last session).
    assert!(s.history.is_empty());
}

#[test]
fn artist_live_top_merges_order_and_counts() {
    let mut s = test_state();
    // Catalog row with art but no count; live rows bring counts.
    let mut cached = mk_track("v1", "Song", "Queen");
    cached.thumbnail_url = Some("http://art".into());
    cached.duration_secs = 0;
    s.catalog.insert("v1".into(), cached);
    s.results.artists.push(Artist {
        id: "UCqueen".into(),
        name: "Queen".into(),
        thumbnail_url: None,
    });
    // Resolvers: direct id, name fallback, unknown none.
    assert_eq!(s.artist_channel_id("UCqueen").as_deref(), Some("UCqueen"));
    assert_eq!(s.artist_channel_id("Queen").as_deref(), Some("UCqueen"));
    assert_eq!(s.artist_channel_id("Nobody"), None);
    // Display falls back to the catalog before the fetch lands.
    let fallback = s.artist_display_tracks("UCqueen");
    assert_eq!(fallback.len(), 1);
    assert!(fallback[0].plays.is_none());
    // Arrival merges in server order (art kept, blanks filled).
    let mut live = mk_track("v1", "Song", "Queen");
    live.plays = Some("76M plays".into());
    live.duration_secs = 200;
    let mut fresh = mk_track("v9", "New Hit", "Queen");
    fresh.plays = Some("1M plays".into());
    s.artist_loading = Some("UCqueen".into());
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCqueen".into(),
        result: Ok(vec![fresh, live]),
    });
    assert_eq!(s.artist_loading, None);
    let shown = s.artist_display_tracks("UCqueen");
    assert_eq!(
        shown.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["v9", "v1"]
    );
    let merged = s.find_track("v1").expect("merged");
    assert_eq!(merged.plays.as_deref(), Some("76M plays"));
    assert_eq!(merged.thumbnail_url.as_deref(), Some("http://art"));
    assert_eq!(merged.duration_secs, 200);
    // Failures name the failure only for the page still showing
    // it (release builds show failures only); arrivals for pages
    // already left (wrong marker or wrong route) stay quiet.
    s.results.artists.push(Artist {
        id: "UCzz".into(),
        name: "Zed".into(),
        thumbnail_url: None,
    });
    s.route = Route::Artist("UCzz".into());
    s.artist_loading = Some("UCzz".into());
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCzz".into(),
        result: Err("offline".into()),
    });
    assert!(s.status.contains("failed"), "{}", s.status);
    s.status = "Ready.".into();
    s.artist_loading = Some("UCother".into());
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCzz".into(),
        result: Err("offline".into()),
    });
    assert_eq!(s.status, "Ready.");
    s.status = "Ready.".into();
    s.route = Route::Home;
    s.artist_loading = Some("UCzz".into());
    let _ = s.update(Message::ArtistTopLoaded {
        channel_id: "UCzz".into(),
        result: Err("offline".into()),
    });
    assert_eq!(s.status, "Ready.");
    // Cache + aliases survive later searches replacing `results`
    // (aliases record at open; direct routes self-resolve anyway).
    let _ = s.update(Message::OpenArtist("UCqueen".into()));
    s.results.artists.clear();
    assert_eq!(s.artist_display_tracks("UCqueen").len(), 2);
    assert_eq!(s.artist_display_tracks("Queen").len(), 2);
}

#[test]
fn album_live_tracklist_merges_track_order_and_counts() {
    let mut s = test_state();
    let mut cached = mk_track("v1", "First", "Queen");
    cached.album = Some("Test Album".into());
    s.catalog.insert("v1".into(), cached);
    s.results.albums.push(Album {
        id: "MPREalbum".into(),
        title: "Test Album".into(),
        artist: "Queen".into(),
        year: None,
        thumbnail_url: None,
    });
    // Resolvers: direct id, title fallback, synthetic cards
    // upgrade via their title, unknown none.
    assert_eq!(s.album_browse_id("MPREalbum").as_deref(), Some("MPREalbum"));
    assert_eq!(
        s.album_browse_id("Test Album").as_deref(),
        Some("MPREalbum")
    );
    assert_eq!(
        s.album_browse_id("ax-Test Album").as_deref(),
        Some("MPREalbum")
    );
    assert_eq!(s.album_browse_id("Unknown"), None);
    // Arrival merges in `track_no` order with counts and durations.
    let mut second = mk_track("v2", "Second", "Queen");
    second.plays = Some("50M plays".into());
    second.duration_secs = 261;
    second.album = Some("Test Album".into());
    let mut first = mk_track("v1", "First", "Queen");
    first.plays = Some("100M plays".into());
    first.duration_secs = 179;
    first.album = Some("Test Album".into());
    s.album_loading = Some("MPREalbum".into());
    let _ = s.update(Message::AlbumLiveLoaded {
        album_id: "MPREalbum".into(),
        result: Ok(vec![first, second]),
    });
    assert_eq!(s.album_loading, None);
    let shown = s.album_display_tracks("MPREalbum");
    assert_eq!(
        shown.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["v1", "v2"]
    );
    assert_eq!(shown[0].plays.as_deref(), Some("100M plays"));
    assert_eq!(shown[1].duration_secs, 261);
    // Cache + aliases survive later searches replacing `results`
    // (recorded at open; direct routes self-resolve anyway).
    let _ = s.update(Message::OpenAlbum("MPREalbum".into()));
    s.results.albums.clear();
    assert_eq!(s.album_display_tracks("MPREalbum").len(), 2);
    assert_eq!(s.album_display_tracks("Test Album").len(), 2);
}

#[test]
fn album_browse_id_disambiguates_duplicate_titles() {
    let mut s = test_state();
    for (id, artist) in [("MPREa", "Queen"), ("MPREb", "ABBA")] {
        s.results.albums.push(Album {
            id: id.into(),
            title: "Greatest Hits".into(),
            artist: artist.into(),
            year: None,
            thumbnail_url: None,
        });
    }
    // The catalog owner wins over first-match.
    let mut t = mk_track("v1", "Song", "ABBA");
    t.album = Some("Greatest Hits".into());
    s.catalog.insert("v1".into(), t);
    assert_eq!(s.album_browse_id("Greatest Hits").as_deref(), Some("MPREb"));
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
    // Presets overwrite all ten slider gains.
    assert_eq!(s.eq_gains, EqPreset::rock().bands);
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
fn eq_band_sliders_go_custom_and_snap_back() {
    let mut s = test_state();
    // One nudge off Flat relabels to Custom and records the gain.
    let _ = s.update(Message::SetEqBand(0, 6.0));
    assert_eq!(s.eq_preset, "Custom");
    assert_eq!(s.eq_gains[0], 6.0);
    assert!(s.eq_gains[1..].iter().all(|g| *g == 0.0));
    assert!(s.status.contains("EQ: Custom"));
    // Out-of-range band indices are ignored, NaN gains too.
    let _ = s.update(Message::SetEqBand(99, 6.0));
    assert_eq!(s.eq_preset, "Custom");
    let _ = s.update(Message::SetEqBand(1, f32::NAN));
    assert_eq!(s.eq_gains[1], 0.0);
    // Clamping keeps sliders inside +/-12 dB.
    let _ = s.update(Message::SetEqBand(1, 99.0));
    assert_eq!(s.eq_gains[1], 12.0);
    // Landing back on a builtin curve restores its name.
    let _ = s.update(Message::SetEqBand(0, 0.0));
    let _ = s.update(Message::SetEqBand(1, 0.0));
    assert_eq!(s.eq_preset, "Flat");
}

#[test]
fn eq_curve_survives_restarts() {
    use std::time::Instant;
    let mut s = test_state();
    // Preset pick persists through the tick saver + reopens intact.
    let _ = s.update(Message::SelectEqPreset("Rock".into()));
    let _ = s.update(Message::Tick(Instant::now()));
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.eq_preset, "Rock");
    assert_eq!(reopened.eq_gains, EqPreset::rock().bands);
    assert!(reopened.eq_enabled);
    // A band nudge persists as Custom with the exact gains.
    let _ = s.update(Message::SetEqBand(0, 6.5));
    let _ = s.update(Message::Tick(Instant::now()));
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.eq_preset, "Custom");
    assert_eq!(reopened.eq_gains[0], 6.5);
    // Bypass persists too (curve kept, toggle off)...
    let _ = s.update(Message::EqToggled(false));
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert!(!reopened.eq_enabled);
    assert_eq!(reopened.eq_gains[0], 6.5);
    // ...and re-enabling persists as well (no stuck bypass).
    let _ = s.update(Message::EqToggled(true));
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert!(reopened.eq_enabled);
    assert_eq!(reopened.eq_preset, "Custom");
    // Garbage gains fall back to Flat.
    let store = LibraryStore::open(&s.library_path).unwrap();
    let _ = store.set_pref("eq_gains", "junk");
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.eq_preset, "Flat");
    assert_eq!(reopened.eq_gains, [0.0; BAND_COUNT]);
}

#[test]
fn eq_drag_throttle_defers_to_tick() {
    use std::time::{Duration, Instant};
    let mut s = test_state();
    // First nudge pushes at once.
    let _ = s.update(Message::SetEqBand(0, 6.0));
    assert_eq!(s.eq_preset, "Custom");
    assert!(!s.eq_dirty);
    // Second nudge inside the cooldown defers to the Tick flush
    // (gains + label still update instantly for the sliders).
    let _ = s.update(Message::SetEqBand(1, 6.0));
    assert!(s.eq_dirty);
    assert!(s.status.contains("tuning"));
    assert_eq!(s.eq_gains[1], 6.0);
    // A tick past the cooldown lands the trailing edge.
    let _ = s.update(Message::Tick(Instant::now() + Duration::from_secs(1)));
    assert!(!s.eq_dirty);
    assert!(s.status.contains("EQ: Custom"));
    assert!(!s.status.contains("tuning"));
}

#[test]
fn eq_bypass_keeps_curve() {
    let mut s = test_state();
    // Off: preset picks are kept, nothing pushes.
    let _ = s.update(Message::EqToggled(false));
    assert!(!s.eq_enabled);
    assert!(s.status.contains("EQ off"));
    let _ = s.update(Message::SelectEqPreset("Rock".into()));
    assert_eq!(s.eq_preset, "Rock");
    assert_eq!(s.eq_gains, EqPreset::rock().bands);
    assert!(s.status.contains("kept"));
    // Slider edits while off keep gains + label, set no flush.
    let _ = s.update(Message::SetEqBand(0, 6.0));
    assert_eq!(s.eq_gains[0], 6.0);
    assert!(!s.eq_dirty);
    // Back on: the stored curve pushes.
    let _ = s.update(Message::EqToggled(true));
    assert!(s.eq_enabled);
    assert!(s.status.contains("EQ: Custom"));
}

#[test]
fn settings_view_builds_with_graph_eq() {
    // Widget-tree smoke: Settings (preset buttons, bypass toggle,
    // canvas graph) constructs without panicking.
    let mut s = test_state();
    s.route = Route::Settings;
    let _ = s.view();
    // Bypassed, custom, and preset states all build.
    let _ = s.update(Message::EqToggled(false));
    let _ = s.view();
    let _ = s.update(Message::SetEqBand(4, 3.5));
    let _ = s.view();
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
fn scrobble_modal_opens_closes_and_keeps_edits() {
    let mut s = test_state();
    assert!(!s.scrobble_modal_open);
    assert!(s.scrobble_dialog().is_none());
    let _ = s.update(Message::OpenScrobbleModal);
    assert!(s.scrobble_modal_open);
    assert!(s.scrobble_dialog().is_some());
    // Entrance restarts on open and eases up on ticks.
    assert_eq!(s.modal_anim, 0.0);
    let _ = s.update(Message::Animate);
    assert!(s.modal_anim > 0.0);
    // Edits inside the dialog stick around the close.
    let _ = s.update(Message::ScrobbleUserChanged("alice".into()));
    let _ = s.update(Message::CloseScrobbleModal);
    assert!(!s.scrobble_modal_open);
    assert_eq!(s.scrobble_user, "alice");
}

#[test]
fn scrobble_modal_dismissed_by_esc() {
    let mut s = test_state();
    let _ = s.update(Message::OpenScrobbleModal);
    assert!(s.scrobble_modal_open);
    let _ = s.update(Message::CloseContext);
    assert!(!s.scrobble_modal_open);
}

#[cfg(feature = "innertube")]
#[test]
fn esc_closes_topmost_scrobble_modal_first() {
    let mut s = test_state();
    s.pending_cookie_cleanup = Some("/tmp/cookies.txt".into());
    // Settled cookie entrance survives opening scrobble on top.
    s.modal_anim = 1.0;
    let _ = s.update(Message::OpenScrobbleModal);
    assert_eq!(s.modal_anim, 1.0);
    let _ = s.update(Message::CloseContext);
    assert!(!s.scrobble_modal_open);
    assert!(s.pending_cookie_cleanup.is_some());
}

#[test]
fn scrobble_toggle_and_user_survive_restart() {
    let mut s = test_state();
    let _ = s.update(Message::ScrobbleToggled(true));
    let _ = s.update(Message::ScrobbleUserChanged("alice".into()));
    // Secrets ride in memory for the session (the keychain is never
    // touched by unit builds) and survive the dialog close.
    let _ = s.update(Message::OpenScrobbleModal);
    let _ = s.update(Message::ScrobbleLbTokenChanged("tok".into()));
    let _ = s.update(Message::CloseScrobbleModal);
    assert_eq!(s.scrobble_lb_token, "tok");
    // Non-secrets round-trip through SQLite; tokens do not.
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert!(again.scrobble_enabled);
    assert_eq!(again.scrobble_user, "alice");
    assert!(again.scrobble_lb_token.is_empty());
}

#[test]
fn seg_button_marks_active_preset() {
    use iced::widget::button::Status;
    let theme = iced::Theme::Dark;
    let on = seg_button(true)(&theme, Status::Hovered);
    let off = seg_button(false)(&theme, Status::Hovered);
    assert_eq!(on.background, Some(iced::Background::Color(ACCENT_HOVER)));
    assert_eq!(off.background, Some(iced::Background::Color(CARD_BG_HOVER)));
    let idle_on = seg_button(true)(&theme, Status::Active);
    let idle_off = seg_button(false)(&theme, Status::Active);
    assert_eq!(idle_on.background, Some(iced::Background::Color(ACCENT)));
    assert_eq!(idle_off.background, None);
    // Pressed counts as hovered; disabled falls back to idle.
    let pressed_on = seg_button(true)(&theme, Status::Pressed);
    let disabled_off = seg_button(false)(&theme, Status::Disabled);
    assert_eq!(
        pressed_on.background,
        Some(iced::Background::Color(ACCENT_HOVER))
    );
    assert_eq!(disabled_off.background, None);
}

#[test]
fn home_greeting_follows_local_hour() {
    let en = Language::English;
    assert_eq!(en.greeting(4), "Good night");
    assert_eq!(en.greeting(5), "Good morning");
    assert_eq!(en.greeting(11), "Good morning");
    assert_eq!(en.greeting(12), "Good afternoon");
    assert_eq!(en.greeting(17), "Good afternoon");
    assert_eq!(en.greeting(18), "Good evening");
    assert_eq!(en.greeting(22), "Good evening");
    assert_eq!(en.greeting(23), "Good night");
    // Same bounds, Spanish words.
    let es = Language::Spanish;
    assert_eq!(es.greeting(4), "Buenas noches");
    assert_eq!(es.greeting(5), "Buenos días");
    assert_eq!(es.greeting(12), "Buenas tardes");
    assert_eq!(es.greeting(22), "Buenas noches");
}

#[test]
fn eq_flow_rows_fill_first_row_like_flex_wrap() {
    let names: Vec<String> = [
        "Flat",
        "Bass Boost",
        "Treble Boost",
        "Vocal",
        "Rock",
        "Pop",
        "Jazz",
        "Classical",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // Wide body: one strip of 8.
    let wide = eq_flow_rows(&names, 1200.0);
    assert_eq!(wide.len(), 1);
    assert_eq!(wide[0].len(), 8);
    // Tight body: greedy fill beats fixed halves ([5, 3], not [4, 4]),
    // order preserved, every row non-empty.
    let narrow = eq_flow_rows(&names, 350.0);
    assert_eq!(narrow.iter().map(Vec::len).collect::<Vec<_>>(), vec![5, 3]);
    let flat: Vec<&String> = narrow.iter().flatten().collect();
    assert_eq!(flat, names.iter().collect::<Vec<_>>());
    // Empty input: no rows (never a panic, never an empty box row).
    assert!(eq_flow_rows(&[], 1200.0).is_empty());
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
    assert!(s.status.contains("failed"), "{}", s.status);
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
    // Stale arrival (user moved on): dropped silently, and the new
    // load's flag survives so its own resolve still lands.
    let _ = s.update(Message::EnqueueTrack("m2".into()));
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Ok(vec![1, 2, 3]),
    });
    assert_eq!(s.loading_track.as_deref(), Some("m2"));
    assert!(s.queue.current().map(|t| t.id.as_str()) == Some("m2"));
    // Fetch failure for the current track retries the same load.
    let _ = s.update(Message::StreamResolved {
        video_id: "m2".into(),
        result: Err("403 Forbidden".into()),
    });
    assert_eq!(s.loading_track.as_deref(), Some("m2"));
    assert!(s.status.contains("retrying"));
}

#[test]
fn stream_resolved_reports_errors() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    // First failure retries the same load (never stuck on one flake).
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("403 Forbidden".into()),
    });
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    assert!(s.status.contains("retrying"));
    // Budget exhausted on a single track: honest error, loader clear.
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("403 Forbidden".into()),
    });
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("403 Forbidden".into()),
    });
    assert!(s.status.contains("end of queue"), "status: {}", s.status);
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
            is_video: false,
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
        is_video: false,
    });
    let _ = s2.update(Message::PlayPlaylist(pid));
    assert_eq!(
        s2.queue.items()[0].thumbnail_url.as_deref(),
        Some("https://example.invalid/cover")
    );
}

#[test]
fn user_playlists_survive_restart() {
    let path = std::env::temp_dir().join(format!("yutu-test-restart-{}.db", std::process::id()));
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
        is_video: false,
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
        is_video: false,
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
            is_video: false,
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
    s.route = Route::Discovery;
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
    s.route = Route::Discovery;
    let _ = s.ensure_visible_tasks();
    assert!(!s.hydrating.contains("retry-me"));
}

#[test]
fn menu_open_warms_the_hero_header_key() {
    // Regression: row slots warm the row raster only, but menu
    // headers render the 544px hero (menus never scroll-sweep).
    redirect_art_cache();
    let mut s = test_state();
    let t = Track {
        id: "menu-hero".into(),
        title: "Menu Hero".into(),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: Some("https://example.invalid/ms=w60-h60-l90-rj".into()),
        thumbnail_large_url: Some("https://example.invalid/ml=w120-h120-l90-rj".into()),
        plays: None,
        is_video: false,
    };
    let hero = yutu_core::hires_thumb_url(t.thumbnail_large_url.as_deref().unwrap(), 544);
    s.catalog.insert(t.id.clone(), t);
    let _ = s.update(Message::OpenContext("menu-hero".into()));
    assert!(
        s.artwork_loading.contains(&hero),
        "menu header hero must warm on open"
    );
}

#[test]
fn failed_hero_cools_down_like_failed_thumbs() {
    // `large_art_task` must respect the failure cooldown, or every
    // chained re-scan refetches dead heroes in a loop.
    redirect_art_cache();
    let mut s = test_state();
    let url = "https://example.invalid/dead-hero=w60-h60-l90-rj".to_string();
    let t = Track {
        id: "dead-hero".into(),
        title: "Dead Hero".into(),
        artist: "Someone".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: Some(url.clone()),
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    };
    s.catalog.insert(t.id.clone(), t.clone());
    let hero = yutu_core::hires_thumb_url(&url, 544);
    let _ = s.update(Message::ArtworkReady {
        url: hero.clone(),
        result: Err("404".into()),
    });
    assert!(s.large_art_task(&t).is_none());
}

#[test]
fn lazy_warmer_covers_hero_keys_for_home_cards() {
    // Regression: Home "Jump back in" cards read the 544px hero key
    // (thumb_hero), but the lazy warmer only queued the row raster, so
    // covers stayed placeholder until a play warmed the hero.
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
        is_video: false,
    };
    let hero = yutu_core::hires_thumb_url(t.thumbnail_large_url.as_deref().unwrap(), 544);
    s.catalog.insert(t.id.clone(), t);
    s.history.push("hero-card".into());
    s.route = Route::Home;
    let _ = s.ensure_visible_tasks();
    assert!(s
        .artwork_loading
        .contains("https://example.invalid/s=w120-h120-l90-rj"));
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
        is_video: false,
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
    // Batch cap (12 tasks per sweep) leaves remainder on long lists;
    // each arrival re-scans visible so the rest still loads.
    redirect_art_cache();
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
                is_video: false,
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

/// Process-unique art cache dir shared by all art tests (same
/// value in every test, so parallel `set_var` races are benign;
/// urls stay unique per test so files never collide; nanos-unique
/// so stale files from past runs cannot linger).
fn redirect_art_cache() -> String {
    pub(crate) static DIR: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let dir = DIR
        .get_or_init(|| {
            let ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            std::env::temp_dir()
                .join(format!("yutu-test-art-{}-{ns}", std::process::id()))
                .to_string_lossy()
                .into_owned()
        })
        .clone();
    std::env::set_var("YUTU_TEST_ART_DIR", &dir);
    dir
}

/// Track with both art keys (rows read the row raster, heroes the
/// 544px key, like real search hits).
fn art_track(id: &str) -> Track {
    Track {
        id: id.into(),
        title: id.into(),
        artist: "A".into(),
        album: None,
        duration_secs: 200,
        thumbnail_url: Some(format!("https://example.invalid/{id}.jpg=w60-h60-l90-rj")),
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    }
}

#[test]
fn scroll_sweep_fetches_only_the_viewport() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.search_query = "ab".into();
    s.route = Route::Search;
    s.search_filter = SearchFilter::Songs;
    s.results.tracks = (0..30).map(|i| art_track(&format!("sw{i}"))).collect();
    // Top of list: head urls fetch, tail does not.
    let _ = s.update(Message::Scrolled("body", 0.0));
    assert!(s.artwork_loading.iter().any(|u| u.contains("sw0.jpg")));
    assert!(!s.artwork_loading.iter().any(|u| u.contains("sw29.jpg")));
    let first = s.artwork_loading.len();
    assert!(first > 0);
    // Same bucket immediately: storm collapses, nothing new.
    let _ = s.update(Message::Scrolled("body", 10.0));
    assert_eq!(s.artwork_loading.len(), first);
    // Deep scroll: a new bucket fetches the window head (cap 12).
    let _ = s.update(Message::Scrolled("body", 1200.0));
    assert!(s.artwork_loading.iter().any(|u| u.contains("sw13.jpg")));
    assert!(!s.artwork_loading.iter().any(|u| u.contains("sw29.jpg")));
    // Completing the head lets the next sweep progress the tail
    // (single thumb key per row now: each 12-task batch covers
    // twice the rows the old two-key batches did).
    for url in s.artwork_loading.clone() {
        let _ = s.update(Message::ArtworkReady {
            url,
            result: Ok(vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x00, 0x10]),
        });
    }
    let _ = s.update(Message::Scrolled("body", 1320.0));
    assert!(s.artwork_loading.iter().any(|u| u.contains("sw25.jpg")));
    assert_eq!(
        s.scroll_offsets.get("body").copied().unwrap_or(-1.0),
        1320.0
    );
}

#[test]
fn history_row_slots_cover_scrolled_tail() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    for i in 0..30 {
        let id = format!("hh{i}");
        s.catalog.insert(id.clone(), art_track(&id));
        s.history_full.push(HistoryEntry {
            track_id: id,
            played_at_unix: now_unix() - i as i64 * 60,
        });
    }
    s.route = Route::History;
    let slots = s.art_slots("body");
    // Tail rows get slots too, not just the nav-time head (12).
    assert!(slots.len() > 12, "slots: {}", slots.len());
    assert!(slots.iter().any(|(u, _)| u.contains("hh0.jpg")));
    assert!(slots.iter().any(|(u, _)| u.contains("hh29.jpg")));
    // Deep scroll fetches the window head first (12-task batches);
    // completing it chains the tail, same as the search sweep.
    let _ = s.update(Message::Scrolled("body", 1200.0));
    assert!(s.artwork_loading.iter().any(|u| u.contains("hh10.jpg")));
    for url in s.artwork_loading.clone() {
        let _ = s.update(Message::ArtworkReady {
            url,
            result: Ok(vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x00, 0x10]),
        });
    }
    let _ = s.update(Message::Scrolled("body", 1320.0));
    assert!(s.artwork_loading.iter().any(|u| u.contains("hh29.jpg")));
}

#[test]
fn search_row_slots_follow_pitch() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.route = Route::Search;
    s.search_filter = SearchFilter::Songs;
    s.results.tracks = vec![art_track("a"), art_track("b"), art_track("c")];
    let slots = s.art_slots("body");
    // Chips + spacing head, then exact 58px pitch per row: one
    // thumb slot per row (heroes warm on demand, never per row).
    let ya: Vec<f32> = slots
        .iter()
        .filter(|(u, _)| u.contains("/a.jpg"))
        .map(|(_, y)| *y)
        .collect();
    let yb: Vec<f32> = slots
        .iter()
        .filter(|(u, _)| u.contains("/b.jpg"))
        .map(|(_, y)| *y)
        .collect();
    assert_eq!(ya, vec![64.0]);
    assert_eq!(yb.len(), 1);
    assert!((yb[0] - ya[0] - 58.0).abs() < 0.01);
}

#[test]
fn queue_slots_use_divider_pitch() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    for id in ["q0", "q1", "q2"] {
        s.queue.push(art_track(id));
    }
    let slots = s.art_slots("queue");
    // One thumb slot per row (heroes warm on demand).
    let y0: Vec<f32> = slots
        .iter()
        .filter(|(u, _)| u.contains("q0.jpg"))
        .map(|(_, y)| *y)
        .collect();
    let y1: Vec<f32> = slots
        .iter()
        .filter(|(u, _)| u.contains("q1.jpg"))
        .map(|(_, y)| *y)
        .collect();
    assert_eq!(y0, vec![0.0]);
    // One row pitch + hairline between (never leading).
    assert_eq!(y1.len(), 1);
    let pitch = QUEUE_ROW_H + DIVIDER_PX;
    assert!(y1.iter().all(|y| (y - pitch).abs() < 0.01));
}

#[test]
fn grid_cols_follow_center_width() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    // (1280 - 16 - 235 - 8 - 285 - 8 - 40) capped at 1110, / 166.
    assert_eq!(s.grid_cols(), 4);
}

#[test]
fn lazy_image_never_fetches_from_view() {
    redirect_art_cache();
    let s = test_state();
    // Placeholder builds with no cache touch and no fetch state.
    let _ = s.lazy_image(None, 40.0);
    assert!(s.artwork.is_empty());
    assert!(s.artwork_loading.is_empty());
    // Cached handle renders from memory (still no fetch).
    let mut s = s;
    s.artwork.insert(
        "https://example.invalid/cached.jpg".into(),
        (
            iced::widget::image::Handle::from_bytes(vec![0u8; 600]),
            std::time::Instant::now(),
        ),
    );
    let _ = s.lazy_image(Some("https://example.invalid/cached.jpg"), 40.0);
    assert!(s.artwork_loading.is_empty());
}

#[test]
fn artwork_ready_saves_every_cover_to_disk() {
    let dir = redirect_art_cache();
    let mut s = test_state();
    let url = "https://i.ytimg.com/vi/saveall/hqdefault.jpg".to_string();
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x00, 0x10];
    jpeg.resize(1024, 0);
    let _ = s.update(Message::ArtworkReady {
        url: url.clone(),
        result: Ok(jpeg),
    });
    assert!(s.artwork.contains_key(&url));
    assert!(
        artwork_path_in(&dir, &url).is_some(),
        "fetched covers persist to disk"
    );
}

/// Backdate a file's mtime by `secs` (wall-clock: hermetic no
/// matter the machine uptime, unlike `Instant` arithmetic).
fn backdate(path: &std::path::Path, secs: u64) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn art_age_fresh_spans_the_ttl() {
    assert!(art_age_fresh(std::time::Duration::from_secs(60)));
    assert!(art_age_fresh(std::time::Duration::from_secs(
        ART_CACHE_TTL_SECS
    )));
    assert!(!art_age_fresh(std::time::Duration::from_secs(
        ART_CACHE_TTL_SECS + 1
    )));
}

#[test]
fn expired_artwork_reads_as_missing() {
    let base = std::env::temp_dir().join(format!(
        "yutu-test-ttl-{}-{}",
        std::process::id(),
        TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let dir = base.to_string_lossy().into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    let fresh_url = "https://i.ytimg.com/vi/ttlfresh/hqdefault.jpg";
    let old_url = "https://i.ytimg.com/vi/ttlold/hqdefault.jpg";
    save_artwork_in(&dir, fresh_url, &[0xFF, 0xD8, 0xFF, 0xE0]);
    save_artwork_in(&dir, old_url, &[0xFF, 0xD8, 0xFF, 0xE0]);
    // Age one cover past the TTL: it reads as missing (the file
    // itself stays until a prune sweep: offline viewers keep
    // their bytes until a fetch can replace them).
    let old_path = artwork_path_in(&dir, old_url).expect("present before ageing");
    backdate(std::path::Path::new(&old_path), ART_CACHE_TTL_SECS + 3600);
    assert!(
        artwork_path_in(&dir, old_url).is_none(),
        "day-old covers refetch"
    );
    assert!(
        std::path::Path::new(&old_path).exists(),
        "expired files survive reads; prune sweeps them"
    );
    assert!(artwork_path_in(&dir, fresh_url).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_sweeps_expired_before_size_cap() {
    let base = std::env::temp_dir().join(format!(
        "yutu-test-ttl-prune-{}-{}",
        std::process::id(),
        TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let dir = base.to_string_lossy().into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(base.join("fresh.bin"), vec![0u8; 8]).unwrap();
    std::fs::write(base.join("old.bin"), vec![0u8; 8]).unwrap();
    backdate(&base.join("old.bin"), ART_CACHE_TTL_SECS + 3600);
    // No size pressure at all: expiry alone sweeps.
    prune_artwork_dir_in(&dir, u64::MAX);
    assert!(!base.join("old.bin").exists());
    assert!(base.join("fresh.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn memory_art_expires_after_ttl() {
    redirect_art_cache();
    let mut s = test_state();
    // Fresh entries render from memory with no fetch.
    s.artwork.insert(
        "https://example.invalid/mem-fresh".into(),
        (
            iced::widget::image::Handle::from_bytes(vec![0u8; 600]),
            std::time::Instant::now(),
        ),
    );
    assert!(s.art_mem("https://example.invalid/mem-fresh").is_some());
    // Unknown urls miss memory (and the empty test dir): the view
    // holds the placeholder and never fetches itself.
    assert!(s.art_mem("https://example.invalid/mem-ghost").is_none());
    let _ = s.lazy_image(Some("https://example.invalid/mem-ghost"), 40.0);
    assert!(s.artwork_loading.is_empty());
    // The TTL comparison itself is pinned by
    // `art_age_fresh_spans_the_ttl` (past `Instant`s cannot be
    // fabricated hermetically: `Instant` is uptime, not wall time).
}

#[test]
fn history_and_lyrics_caches_stay_capped() {
    redirect_art_cache();
    let mut s = test_state();
    for i in 0..HISTORY_CAP + 50 {
        s.push_history(format!("h{i}"));
    }
    assert_eq!(s.history.len(), HISTORY_CAP);
    assert_eq!(s.history.last().unwrap(), "h549");
    #[cfg(feature = "lyrics")]
    {
        for i in 0..LYRICS_CACHE_CAP + 10 {
            s.cache_lyrics(
                format!("l{i}"),
                yutu_core::LyricsResult {
                    instrumental: false,
                    synced: vec![],
                    plain: String::new(),
                },
                LyricsProvider::Lrclib,
            );
        }
        assert!(s.lyrics_cache.len() <= LYRICS_CACHE_CAP);
    }
}

#[test]
fn album_menu_open_warms_the_header_key() {
    // Coll menus never sweep: the 544 header key warms on open
    // (rows stage the small raster only).
    redirect_art_cache();
    let mut s = test_state();
    s.results.albums.push(Album {
        id: "ma1".into(),
        title: "Menu Album".into(),
        artist: "Someone".into(),
        year: None,
        thumbnail_url: Some("https://example.invalid/ma=w60-h60-l90-rj".into()),
    });
    let _ = s.update(Message::OpenAlbumMenu("ma1".into()));
    assert!(s.coll_menu.is_some());
    assert!(
        s.artwork_loading.iter().any(|u| u.contains("=w544-h544")),
        "menu 544 key must warm on open"
    );
}

#[test]
fn stage_search_warms_row_rasters_not_heroes() {
    // Staging both rasters double-fetches under the task gates;
    // rows display the small key, heroes warm on demand.
    redirect_art_cache();
    let mut s = test_state();
    let bundle = bundle_of(SearchResults {
        tracks: vec![],
        albums: vec![Album {
            id: "sa1".into(),
            title: "Staged".into(),
            artist: "Someone".into(),
            year: None,
            thumbnail_url: Some("https://example.invalid/sa=w60-h60-l90-rj".into()),
        }],
        artists: vec![],
        playlists: vec![],
    });
    let _ = s.stage_search(bundle);
    assert!(s.artwork_loading.iter().any(|u| u.contains("sa=w120-h120")));
    assert!(
        !s.artwork_loading.iter().any(|u| u.contains("=w544-h544")),
        "staged coll keys stay small"
    );
}

#[test]
fn mem_cap_evicts_oldest_half_not_everything() {
    redirect_art_cache();
    let mut s = test_state();
    for i in 0..ART_MEM_CAP {
        s.artwork.insert(
            format!("https://example.invalid/old-{i}.jpg"),
            (
                iced::widget::image::Handle::from_bytes(vec![0u8; 600]),
                std::time::Instant::now(),
            ),
        );
    }
    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x00, 0x10];
    jpeg.resize(1024, 0);
    let fresh = "https://example.invalid/fresh.jpg".to_string();
    let _ = s.update(Message::ArtworkReady {
        url: fresh.clone(),
        result: Ok(jpeg),
    });
    // Half the stale entries go (amortized), the fresh arrival stays,
    // and the cap holds: the old `clear()` refetched the viewport at
    // once and spiked straight back to the cap.
    assert_eq!(s.artwork.len(), ART_MEM_CAP / 2 + 1);
    assert!(s.art_mem(&fresh).is_some());
}

#[test]
fn prune_artwork_dir_evicts_oldest_first() {
    let base = std::env::temp_dir().join(format!(
        "yutu-test-prune-{}-{}",
        std::process::id(),
        TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let dir = base.to_string_lossy().into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.bin", "b.bin", "c.bin"] {
        std::fs::write(base.join(name), vec![0u8; 8]).unwrap();
    }
    // 24 bytes live, cap 10: two files must go, one survives.
    prune_artwork_dir_in(&dir, 10);
    let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(left.len(), 1);
    let total: u64 = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    assert!(total <= 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_artwork_replaces_sibling_extensions() {
    let base = std::env::temp_dir().join(format!(
        "yutu-test-ext-{}-{}",
        std::process::id(),
        TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let dir = base.to_string_lossy().into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    let url = "https://i.ytimg.com/vi/changing/hqdefault.jpg";
    save_artwork_in(&dir, url, &[0xFF, 0xD8, 0xFF, 0xE0]);
    assert!(artwork_path_in(&dir, url).is_some());
    // Same stem, new content type: the stale jpg must not shadow it.
    save_artwork_in(&dir, url, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A]);
    let path = artwork_path_in(&dir, url).expect("png wins");
    assert!(path.ends_with(".png"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Synced fixture: two verses, a stanza gap, then a third verse.
/// `position_secs` picks the highlight; geometry is hand-checked
/// below (window 1280x800, default sidebars).
fn synced_state(pos_secs: f64) -> AppState {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.queue.push(mk_track("m1", "T", "A"));
    s.lyrics_cache.insert(
        ("m1".into(), LyricsProvider::Lrclib),
        yutu_core::LyricsResult {
            plain: String::new(),
            synced: vec![
                yutu_core::LyricLine {
                    start_ms: 0,
                    text: "aaa".into(),
                },
                yutu_core::LyricLine {
                    start_ms: 5000,
                    text: String::new(),
                },
                yutu_core::LyricLine {
                    start_ms: 5000,
                    text: "bbb".into(),
                },
                yutu_core::LyricLine {
                    start_ms: 10000,
                    text: "ccc".into(),
                },
            ],
            instrumental: false,
        },
    );
    s.drawer_open = true;
    s.position_secs = pos_secs;
    s
}

#[test]
fn lyric_follow_centers_each_verse() {
    // col 560, viewport 540: plain rows 30.5, current 34.6, gap 16.
    let s = synced_state(1.0);
    assert_eq!(s.lyric_cursor_at("m1"), Some(0));
    // Verse 0 top 444, center 444 + 17.3 - 270.
    let y0 = s.lyric_follow_y("m1", 0).expect("verse 0 targets");
    assert!((y0 - 191.3).abs() < 1.0, "got {y0}");
    // Verse 1 sits past a stanza gap: top 496.5, taller current row.
    let s = synced_state(6.0);
    assert_eq!(s.lyric_cursor_at("m1"), Some(1));
    let y1 = s.lyric_follow_y("m1", 1).expect("verse 1 targets");
    assert!((y1 - 243.8).abs() < 1.0, "got {y1}");
    assert!(y1 > y0);
    // Verse 2 advances again.
    let s = synced_state(11.0);
    let y2 = s.lyric_follow_y("m1", 2).expect("verse 2 targets");
    assert!((y2 - 280.3).abs() < 1.0, "got {y2}");
    assert!(y2 > y1);
    // No highlight (position before the first timestamp): no target.
    let mut s = synced_state(0.0);
    s.lyrics_cache
        .get_mut(&("m1".to_string(), LyricsProvider::Lrclib))
        .unwrap()
        .synced
        .iter_mut()
        .for_each(|l| l.start_ms += 5000);
    assert_eq!(s.lyric_cursor_at("m1"), None);
    // The task path returns before geometry on no highlight, and
    // unknown ids have no rows to center.
    assert!(s.lyric_follow_y("ghost", 0).is_none());
    // Instrumentals never follow, even with stray synced lines.
    let mut s = synced_state(6.0);
    s.lyrics_cache
        .get_mut(&("m1".to_string(), LyricsProvider::Lrclib))
        .unwrap()
        .instrumental = true;
    assert_eq!(s.lyric_cursor_at("m1"), None);
    assert!(s.lyric_follow_y("m1", 1).is_none());
}

#[test]
fn lyric_follow_centers_in_the_measured_viewport() {
    // Short windows: the scrollable reports its own height and the
    // verse centers in it, not in a window-height guess that would
    // park the highlight below the visible area.
    let mut s = synced_state(11.0);
    s.drawer_open = true;
    let _ = s.update(Message::DrawerScrolled(0.0, 600.0));
    let tall = s.lyric_follow_y("m1", 2).unwrap();
    let _ = s.update(Message::DrawerScrolled(0.0, 200.0));
    let short = s.lyric_follow_y("m1", 2).unwrap();
    assert!((short - tall - 200.0).abs() < 0.01);
    // Zero heights (tests, unknown bounds) keep the last measurement.
    let _ = s.update(Message::DrawerScrolled(0.0, 0.0));
    assert_eq!(s.drawer_view_h, Some(200.0));
    // A resize while following drops the recorded verse so the next
    // tick retargets to the new center.
    s.lyrics_follow = true;
    let _ = s.lyric_follow_task();
    assert!(s.lyrics_follow_at.is_some());
    let _ = s.update(Message::DrawerScrolled(s.drawer_scroll, 260.0));
    assert!(s.lyrics_follow_at.is_none());
    let _ = s.lyric_follow_task();
    assert!((s.lyrics_target - (short - 30.0)).abs() < 0.01);
}

#[test]
fn lyric_follow_takes_the_measured_row_center() {
    let mut s = synced_state(11.0);
    s.drawer_open = true;
    s.lyrics_follow = true;
    let _ = s.lyric_follow_task();
    let estimate = s.lyrics_target;
    let geom = |track: &str, at: usize| LyricRowGeom {
        track: track.into(),
        at,
        top: 900.0,
        h: 60.0,
        view_h: 200.0,
        max_y: 5000.0,
    };
    // The live layout wins over the estimate: row center minus half
    // the viewport (wrapped rows and short windows land exactly).
    let _ = s.update(Message::LyricRowMeasured(geom("m1", 2)));
    assert!((s.lyrics_target - 830.0).abs() < 0.01);
    assert!((s.lyrics_target - estimate).abs() > 1.0);
    assert_eq!(s.drawer_view_h, Some(200.0));
    // Stale probes (other verse or track) never move the target.
    let _ = s.update(Message::LyricRowMeasured(geom("m1", 1)));
    let _ = s.update(Message::LyricRowMeasured(geom("ghost", 2)));
    assert!((s.lyrics_target - 830.0).abs() < 0.01);
    // A manual takeover ignores measurements until follow resumes.
    s.lyrics_follow = false;
    let mut far = geom("m1", 2);
    far.top = 1500.0;
    let _ = s.update(Message::LyricRowMeasured(far.clone()));
    assert!((s.lyrics_target - 830.0).abs() < 0.01);
    // Centers clamp to the scrollable range at both ends.
    far.max_y = 1000.0;
    assert!((far.center_y() - 1000.0).abs() < 0.01);
    far.top = 10.0;
    assert_eq!(far.center_y(), 0.0);
}

#[test]
fn lyric_follow_snaps_takeover_and_resumes() {
    let mut s = synced_state(1.0);
    // Opening the drawer arms a fresh follow run.
    s.drawer_open = false;
    let _ = s.update(Message::ToggleDrawer);
    assert!(s.drawer_open);
    assert!(s.lyrics_follow);
    assert!(s.lyrics_follow_at.is_none());
    // First verse records its target (the ticker glides there).
    let _ = s.lyric_follow_task();
    assert_eq!(s.lyrics_follow_at, Some(("m1".into(), 0)));
    let y0 = s.lyrics_target;
    assert!(y0 > 0.0);
    assert!(s.lyrics_gliding());
    // Glide the way the runtime would: each tick commands a step,
    // each echo lands it exactly (never a takeover).
    for _ in 0..80 {
        let _ = s.update(Message::Animate);
        let cmd = s.lyrics_cmds[0];
        let _ = s.update(Message::DrawerScrolled(cmd, 0.0));
        if (s.drawer_scroll - s.lyrics_target).abs() <= 1.0 {
            break;
        }
    }
    assert!((s.drawer_scroll - y0).abs() <= 1.5);
    assert!(s.lyrics_follow);
    assert!(!s.lyrics_gliding());
    // Stale echoes never disengage either: commands outpacing
    // echoes under load still match the ring.
    let _ = s.update(Message::Animate);
    let _ = s.update(Message::Animate);
    let stale = s.lyrics_cmds[2];
    let _ = s.update(Message::DrawerScrolled(stale, 0.0));
    assert!(s.lyrics_follow);
    // Same verse next tick: target holds.
    let _ = s.lyric_follow_task();
    assert!((s.lyrics_target - y0).abs() < 0.01);
    // Verse change retargets and glides forward.
    s.position_secs = 11.0;
    let _ = s.lyric_follow_task();
    assert_eq!(s.lyrics_follow_at, Some(("m1".into(), 2)));
    assert!(s.lyrics_target > y0);
    assert!(s.lyrics_gliding());
    // A real scroll away disengages...
    let _ = s.update(Message::DrawerScrolled(900.0, 0.0));
    assert!(!s.lyrics_follow);
    assert!(!s.lyrics_gliding());
    // ...and scrolling back near the verse resumes silently.
    let back = s.lyrics_target - 50.0;
    let _ = s.update(Message::DrawerScrolled(back, 0.0));
    let _ = s.lyric_follow_task();
    assert!(s.lyrics_follow);
    // Closed drawer and plain lyrics are always a no-op.
    s.drawer_open = false;
    s.lyrics_follow_at = None;
    let _ = s.lyric_follow_task();
    assert!(s.lyrics_follow_at.is_none());
    s.drawer_open = true;
    s.lyrics_cache
        .get_mut(&("m1".to_string(), LyricsProvider::Lrclib))
        .unwrap()
        .synced
        .clear();
    let _ = s.lyric_follow_task();
    assert!(s.lyrics_follow_at.is_none());
}

#[test]
fn lyric_follow_snaps_instantly_without_animations() {
    let mut s = synced_state(1.0);
    s.animations_enabled = false;
    let _ = s.lyric_follow_task();
    assert_eq!(s.lyrics_follow_at, Some(("m1".into(), 0)));
    // No glide: the command is the target, and no ticker runs.
    assert_eq!(s.lyrics_cmds[0], s.lyrics_target);
    assert!(s.lyrics_target > 0.0);
    assert!(!s.lyrics_gliding());
}

#[test]
fn track_change_resets_open_drawer_to_top() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::PlayNow("m1".into()));
    // Drawer open with a stale deep scroll (the user read far down
    // the old lyrics, disengaging follow).
    let _ = s.update(Message::ToggleDrawer);
    let _ = s.update(Message::DrawerScrolled(500.0, 0.0));
    assert!(!s.lyrics_follow);
    // Changing song snaps back to the hero and re-arms follow.
    let _ = s.update(Message::PlayNow("m2".into()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    assert_eq!(s.drawer_scroll, 0.0);
    assert!(s.lyrics_follow);
    assert!(s.lyrics_follow_at.is_none());
    // The 0.0 echo matches the command ring, so no takeover fires.
    let _ = s.update(Message::DrawerScrolled(0.0, 0.0));
    assert!(s.lyrics_follow);
    // Closed drawer: stale offsets are untouched, nothing re-arms.
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::PlayNow("m1".into()));
    s.drawer_scroll = 500.0;
    s.lyrics_follow = false;
    let _ = s.update(Message::PlayNow("m2".into()));
    assert_eq!(s.drawer_scroll, 500.0);
    assert!(!s.lyrics_follow);
}

#[test]
fn home_slots_live_under_body_not_the_strip() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.route = Route::Home;
    // The horizontal strip key sweeps nothing (warmed on nav).
    assert!(s.art_slots("home").is_empty());
    // The vertical page slots live under the body key.
    s.catalog.insert("home-1".into(), art_track("home-1"));
    s.history.push("home-1".into());
    assert!(!s.art_slots("body").is_empty());
}

#[test]
fn sweep_backpressures_past_the_inflight_cap() {
    redirect_art_cache();
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.route = Route::Search;
    s.search_filter = SearchFilter::Songs;
    s.results.tracks = (0..30).map(|i| art_track(&format!("bp{i}"))).collect();
    for i in 0..60 {
        s.artwork_loading
            .insert(format!("https://example.invalid/x{i}"));
    }
    // Forced or not, a deep backlog pauses instead of piling on.
    let _ = s.update(Message::Scrolled("body", 0.0));
    assert!(!s.artwork_loading.iter().any(|u| u.contains("bp0")));
}

#[test]
fn popup_warms_top_tracks_on_suggest() {
    redirect_art_cache();
    let mut s = test_state();
    // Live top-track rows back the open popup: their art warms.
    let _ = s.update(Message::HomeSearchChanged("come".into()));
    assert!(s.show_suggest);
    let _ = s.update(Message::SearchCompleted {
        query: "come".into(),
        result: Ok(Box::new(bundle_of(SearchResults {
            tracks: vec![art_track("come-1")],
            albums: Vec::new(),
            artists: Vec::new(),
            playlists: Vec::new(),
        }))),
    });
    let _ = s.update(Message::SuggestCompleted {
        gen: s.suggest_gen,
        result: Ok(vec!["come together".into()]),
    });
    assert!(s.show_suggest);
    assert!(
        !s.artwork_loading.is_empty(),
        "open popup must warm its visible rows"
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
            is_video: false,
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
    #[allow(unused_imports)]
    use yutu_core::{LyricLine, LyricsResult};
    let mut s = test_state();
    // Unknown id: hydration kicked off, no lyrics fetch.
    let _ = s.update(Message::OpenLyrics("ghost".into()));
    assert_eq!(s.route, Route::Lyrics("ghost".into()));
    assert!(s.lyrics_loading.is_empty());
    // Known track: both providers fetch in parallel (tasks run on
    // the runtime; the test only observes the in-flight markers).
    let _ = s.update(Message::OpenLyrics("m1".into()));
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    {
        assert!(s.lyrics_fetch_pending("m1"));
        assert_eq!(s.lyrics_loading.len(), 2);
    }
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert!(s.lyrics_loading.is_empty());
    // Second open while loading: no duplicate fetch.
    let _ = s.update(Message::OpenLyrics("m1".into()));
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert_eq!(s.lyrics_loading.len(), 2);
    // First arrival caches and displays while the other provider is
    // still in flight.
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::Lrclib,
        result: Ok(LyricsResult {
            plain: "la\nla".into(),
            synced: vec![LyricLine {
                start_ms: 0,
                text: "la".into(),
            }],
            instrumental: false,
        }),
    });
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert!(s.lyrics_fetch_pending("m1"));
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert!(s.lyrics_loading.is_empty());
    let lrclib_key = ("m1".to_string(), LyricsProvider::Lrclib);
    assert_eq!(s.lyrics_cache[&lrclib_key].synced.len(), 1);
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::Lrclib)
    );
    // Second arrival clears the pending set.
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Err("offline".into()),
    });
    assert!(!s.lyrics_fetch_pending("m1"));
    let _ = s.update(Message::LyricsReady {
        track_id: "m2".into(),
        provider: LyricsProvider::Lrclib,
        result: Err("offline".into()),
    });
    #[cfg(feature = "debug-panel")]
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
    let _ = s.update(Message::Navigate(Route::Discovery));
    assert_eq!(s.route, Route::Discovery);
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
    let _ = s.update(Message::Navigate(Route::Settings));
    let _ = s.update(Message::NavForward);
    assert_eq!(s.route, Route::Settings);
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
fn stale_search_completion_fetches_no_art_and_persists_off_thread() {
    let mut s = test_state();
    let _ = s.update(Message::HomeSearchChanged("bonobo".into()));
    let mut old = mk_track("stale-art", "Old Song", "Someone");
    old.thumbnail_url = Some("https://lh3.googleusercontent.com/stale=w60-h60".into());
    let _ = s.update(Message::SearchCompleted {
        query: "bon".into(),
        result: Ok(Box::new(bundle_of(SearchResults {
            tracks: vec![old],
            albums: vec![],
            artists: vec![],
            playlists: vec![],
        }))),
    });
    // Nobody sees stale rows: no artwork fetches queued for them.
    assert!(s.artwork_loading.is_empty());
    assert!(s.find_track("stale-art").is_some());
    // The SQLite write-through lands from the background thread.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let store = LibraryStore::open(&s.library_path).unwrap();
        if store.cached_track("stale-art").unwrap().is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "background track cache write never landed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
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
    assert!(s.status.contains("failed"), "{}", s.status);
}

#[test]
fn play_now_replaces_queue_and_autoplay_appends() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::EnqueueTrack("m2".into()));
    assert_eq!(s.queue.len(), 2);
    // Click-to-play: queue replaced by the single, no source line.
    let _ = s.update(Message::PlayNow("m3".into()));
    assert_eq!(s.queue.len(), 1);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m3"));
    assert_eq!(s.autoplay_for.as_deref(), Some("m3"));
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
fn suggest_popup_pick_and_keyboard_flow() {
    let mut s = test_state();
    let _ = s.update(Message::HomeSearchChanged("bon".into()));
    let gen = s.suggest_gen;
    let _ = s.update(Message::SuggestCompleted {
        gen,
        result: Ok(vec!["bonobo".into(), "bon iver".into()]),
    });
    // Popup text rows come from the suggestion payload.
    assert!(s.suggest_open());
    // Arrows walk text suggestions first, then track rows, wrapping.
    let n = s.suggest_count();
    assert!(n >= 2);
    let _ = s.update(Message::SuggestMoveDown);
    assert_eq!(s.suggest_idx, Some(0));
    let _ = s.update(Message::SuggestMoveUp);
    assert_eq!(s.suggest_idx, Some(n - 1));
    let _ = s.update(Message::SuggestMoveDown);
    assert_eq!(s.suggest_idx, Some(0));
    // Highlighted rows render with no panic.
    let _ = s.view();
    // Enter on a text suggestion searches it and closes the popup.
    let _ = s.update(Message::SuggestPick);
    assert_eq!(s.search_query, "bonobo");
    assert!(!s.show_suggest);
    assert!(s.suggest_idx.is_none());
    // Reopen with a landed live payload ("come" searches once,
    // the completion paints the track row), arrow to it, Enter
    // plays it.
    let _ = s.update(Message::HomeSearchChanged("come".into()));
    let _ = s.update(Message::SearchCompleted {
        query: "come".into(),
        result: Ok(Box::new(bundle_of(SearchResults {
            tracks: vec![mk_track("m1", "Come Together", "The Beatles")],
            albums: Vec::new(),
            artists: Vec::new(),
            playlists: Vec::new(),
        }))),
    });
    let _ = s.update(Message::SuggestCompleted {
        gen: s.suggest_gen,
        result: Ok(vec!["come together".into()]),
    });
    assert!(s.suggest_open());
    assert!(!s.results.tracks.is_empty());
    let _ = s.update(Message::SuggestMoveUp);
    let last = s.suggest_count() - 1;
    assert_eq!(s.suggest_idx, Some(last));
    assert!(last >= s.suggest.len());
    let want = s.results.tracks[last - s.suggest.len()].id.clone();
    let _ = s.update(Message::SuggestPick);
    assert!(!s.show_suggest);
    assert_eq!(
        s.queue.current().map(|t| t.id.as_str()),
        Some(want.as_str())
    );
    // Dead arrows with a closed popup, and scrim clears the rest.
    let _ = s.update(Message::SuggestMoveDown);
    assert!(s.suggest_idx.is_none());
    let _ = s.update(Message::HomeSearchChanged("bon".into()));
    let _ = s.update(Message::SuggestMoveDown);
    assert!(s.suggest_idx.is_some());
    let _ = s.update(Message::CloseSuggest);
    assert!(!s.show_suggest);
    assert!(s.suggest_idx.is_none());
}

#[test]
fn search_submit_closes_the_now_playing_drawer() {
    let mut s = test_state();
    // Now playing open over the Search page: typing, highlighting
    // a text suggestion, Enter must drop the drawer so results
    // take focus (same landing as SearchStay).
    s.route = Route::Search;
    s.drawer_open = true;
    let _ = s.update(Message::HomeSearchChanged("bon".into()));
    let gen = s.suggest_gen;
    let _ = s.update(Message::SuggestCompleted {
        gen,
        result: Ok(vec!["bonobo".into()]),
    });
    assert!(s.suggest_open());
    let _ = s.update(Message::SuggestMoveDown);
    assert_eq!(s.suggest_idx, Some(0));
    let _ = s.update(Message::SuggestPick);
    assert_eq!(s.search_query, "bonobo");
    assert_eq!(s.route, Route::Search);
    assert!(!s.drawer_open);
    // Clicking a suggestion submits the same way.
    s.drawer_open = true;
    let _ = s.update(Message::PickSuggestion("bon iver".into()));
    assert_eq!(s.route, Route::Search);
    assert!(!s.drawer_open);
    // Per-keystroke typing is not a submit: the drawer stays put.
    s.drawer_open = true;
    let _ = s.update(Message::HomeSearchChanged("bonobo".into()));
    assert!(s.drawer_open);
}

#[test]
fn pill_search_button_matches_enter() {
    // The pill magnifier sends SuggestPick, exactly like Enter:
    // empty lands on Search with a type-something hint, typed
    // submits the query there.
    let mut s = test_state();
    s.navigate(Route::Home);
    let _ = s.update(Message::SuggestPick);
    assert_eq!(s.route, Route::Search);
    let mut s = test_state();
    s.navigate(Route::Home);
    let _ = s.update(Message::HomeSearchChanged("bonobo".into()));
    let _ = s.update(Message::SuggestPick);
    assert_eq!(s.route, Route::Search);
    assert_eq!(s.search_query, "bonobo");
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
    #[cfg(feature = "debug-panel")]
    assert!(s.status.contains("Cookie import failed: bad cookies"));
    #[cfg(not(feature = "debug-panel"))]
    assert_eq!(s.status, "Cookie import failed");
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_import_success_arms_cleanup_modal() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src =
        std::env::temp_dir().join(format!("yutu-cleanup-modal-{}-{n}.txt", std::process::id()));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    // Simulate the dispatch capture: the modal must name the file
    // actually imported, not the live path field.
    s.pending_cookie_source = Some(src.to_string_lossy().into_owned());
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Cookie,
        result: Ok(staged(
            vec![mk_track("c1", "Cookie Song", "Someone")],
            vec!["c1".into()],
            vec![],
        )),
    });
    assert_eq!(
        s.pending_cookie_cleanup.as_deref(),
        Some(src.to_string_lossy().as_ref())
    );
    // Entrance restarts on arm and eases up on ticks.
    assert_eq!(s.modal_anim, 0.0);
    let _ = s.update(Message::Animate);
    assert!(s.modal_anim > 0.0);
    let _ = std::fs::remove_file(&src);
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_modal_shows_on_connect_before_playlists_finish() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src =
        std::env::temp_dir().join(format!("yutu-cleanup-early-{}-{n}.txt", std::process::id()));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    s.pending_cookie_source = Some(src.to_string_lossy().into_owned());
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Cookie,
        result: Ok(staged(
            vec![mk_track("c1", "Cookie Song", "Someone")],
            vec!["c1".into()],
            vec![("PLserver1", "Road Trip")],
        )),
    });
    // Playlists still loading, but the modal is already up: the
    // connection (and the secure stash) succeeded.
    assert!(s.import_active);
    assert_eq!(
        s.pending_cookie_cleanup.as_deref(),
        Some(src.to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_file(&src);
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_session_reimport_never_arms_cleanup() {
    let mut s = test_state();
    assert!(s.pending_cookie_source.is_none());
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Cookie,
        result: Ok(staged(
            vec![mk_track("c1", "Cookie Song", "Someone")],
            vec!["c1".into()],
            vec![],
        )),
    });
    assert!(s.pending_cookie_cleanup.is_none());
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_import_dispatch_captures_source() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src = std::env::temp_dir().join(format!(
        "yutu-cleanup-dispatch-{}-{n}.txt",
        std::process::id()
    ));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    s.cookie_path = src.to_string_lossy().into_owned();
    let _ = s.update(Message::CookieImport);
    assert_eq!(
        s.pending_cookie_source.as_deref(),
        Some(src.to_string_lossy().as_ref())
    );
    assert!(s.pending_cookie_cleanup.is_none());
    let _ = std::fs::remove_file(&src);
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_file_picked_sets_path_and_cancel_keeps() {
    let mut s = test_state();
    assert!(s.cookie_path.is_empty());
    let _ = s.update(Message::CookieFilePicked(Some("/tmp/cookies.txt".into())));
    assert_eq!(s.cookie_path, "/tmp/cookies.txt");
    let _ = s.update(Message::CookieFilePicked(None));
    assert_eq!(s.cookie_path, "/tmp/cookies.txt");
    let _ = s.update(Message::CookieFilePicked(Some("   ".into())));
    assert_eq!(s.cookie_path, "/tmp/cookies.txt");
}

#[cfg(feature = "innertube")]
#[test]
fn trim_cookie_path_tails_long_paths() {
    assert_eq!(
        trim_cookie_path("/a/cookies.txt", COOKIE_PATH_SHOWN),
        "/a/cookies.txt"
    );
    assert_eq!(trim_cookie_path("abc", 0), "abc");
    let long = "/home/someone/very/long/nested/download/folder/with/extra/dirs/cookies.txt";
    let shown = trim_cookie_path(long, COOKIE_PATH_SHOWN);
    assert_eq!(shown.chars().count(), COOKIE_PATH_SHOWN);
    assert!(shown.starts_with("..."));
    assert!(shown.ends_with("cookies.txt"));
    // Multi-byte names never split mid-codepoint.
    let wide = format!("/h/{}/cookies.txt", "é".repeat(100));
    let shown_wide = trim_cookie_path(&wide, COOKIE_PATH_SHOWN);
    assert_eq!(shown_wide.chars().count(), COOKIE_PATH_SHOWN);
    assert!(shown_wide.ends_with("cookies.txt"));
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_row_roomy_needs_field_plus_import() {
    assert!(cookie_row_roomy(SETTINGS_INPUT_W + COOKIE_IMPORT_ROOM));
    assert!(cookie_row_roomy(2000.0));
    assert!(!cookie_row_roomy(
        SETTINGS_INPUT_W + COOKIE_IMPORT_ROOM - 1.0
    ));
    assert!(!cookie_row_roomy(400.0));
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_path_budget_follows_field_width() {
    assert_eq!(cookie_path_budget(SETTINGS_INPUT_W), COOKIE_PATH_SHOWN);
    assert_eq!(cookie_path_budget(2000.0), COOKIE_PATH_SHOWN);
    // Narrow field: fitted budget, still tail-trimmed by the caller.
    assert_eq!(cookie_path_budget(400.0), 51);
    // Tiny field: floor keeps short names whole.
    assert_eq!(cookie_path_budget(0.0), 24);
    assert_eq!(cookie_path_budget(100.0), 24);
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_pick_valid_needs_txt() {
    assert!(cookie_pick_valid("/tmp/cookies.txt"));
    assert!(cookie_pick_valid("/tmp/COOKIES.TXT"));
    assert!(!cookie_pick_valid(""));
    assert!(!cookie_pick_valid("   "));
    assert!(!cookie_pick_valid("/tmp/cookies.json"));
    assert!(!cookie_pick_valid("/tmp/txt-backup"));
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_cleanup_delete_removes_export() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src = std::env::temp_dir().join(format!(
        "yutu-cleanup-delete-{}-{n}.txt",
        std::process::id()
    ));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    s.cookie_path = src.to_string_lossy().into_owned();
    s.pending_cookie_cleanup = Some(src.to_string_lossy().into_owned());
    s.import_active = false;
    let _ = s.update(Message::CookieCleanupDelete);
    assert!(s.pending_cookie_cleanup.is_none());
    assert!(s.cookie_path.is_empty());
    assert!(!src.exists());
    assert!(s.status.contains("deleted"));
}

#[cfg(feature = "innertube")]
#[test]
fn cookie_cleanup_keep_preserves_export() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src =
        std::env::temp_dir().join(format!("yutu-cleanup-keep-{}-{n}.txt", std::process::id()));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    s.cookie_path = src.to_string_lossy().into_owned();
    s.pending_cookie_cleanup = Some(src.to_string_lossy().into_owned());
    s.import_active = false;
    let _ = s.update(Message::CookieCleanupKeep);
    assert!(s.pending_cookie_cleanup.is_none());
    assert!(src.exists());
    let _ = std::fs::remove_file(&src);
}

#[cfg(feature = "innertube")]
#[test]
fn cleanup_modal_dismissed_by_esc_as_keep() {
    let n = TEST_DB_SEQ.fetch_add(1, Ordering::SeqCst);
    let src = std::env::temp_dir().join(format!("yutu-cleanup-esc-{}-{n}.txt", std::process::id()));
    std::fs::write(&src, "SID=x; SAPISID=y").unwrap();
    let mut s = test_state();
    s.pending_cookie_cleanup = Some(src.to_string_lossy().into_owned());
    s.import_active = false;
    let _ = s.update(Message::CloseContext);
    assert!(s.pending_cookie_cleanup.is_none());
    assert!(src.exists());
    let _ = std::fs::remove_file(&src);
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

#[test]
fn button_helpers_build() {
    // Labels and icons share one construction path for every
    // button; smoke-build both states (int and float sizes).
    // The idle remove slot paints zero pixels (a blank frame, not a
    // transparent-tinted x: iced's svg tint keeps source alpha and
    // replaces RGB, so TRANSPARENT would paint solid black).
    assert!(!EMPTY_SLOT_SVG.contains("<path"));
    assert!(!EMPTY_SLOT_SVG.contains("currentColor"));
    let _ = AppState::empty_slot_icon(11.0);
    let _ = AppState::hover_remove_slot(None);
    // The app logo renders its own colors (no tint surgery).
    let _ = logo_mark(26.0);
    let _ = btn_text("Sync now", 12);
    let _ = btn_text(format!("Clear queue ({})", 3), 12.0);
    let _ = btn_icon("play", 14.0, iced::Color::WHITE);
    // Twice: cache hit on the second call, plus a variable name.
    let name = if true { "play" } else { "pause" };
    let _ = btn_icon(name, 14.0, iced::Color::WHITE);
    let _ = btn_icon("ghost-name", 14.0, iced::Color::WHITE);
    // The stroke surgery fires on real assets (and only there).
    let raw = thicken_svg(include_bytes!("../assets/icons/play.svg"));
    let thick = String::from_utf8_lossy(&raw);
    assert!(thick.contains("stroke-width=\"2.5\""));
    assert!(!thick.contains("stroke-width=\"2\""));
    // The play triangle is optically centered at runtime (verbatim
    // asset stays pinned): Lucide spans 5-19 (geometric center 12)
    // but the centroid sits ~2.3 left, so +2 right puts visual mass
    // on center without touching the file.
    let optical = optical_play(thicken_svg(include_bytes!("../assets/icons/play.svg")));
    assert!(String::from_utf8_lossy(&optical).contains("points=\"7 3 21 12 7 21 7 3\""));
    // Filled transport glyphs stay optically centered in their
    // 18px boxes: the play file spans 5-19 (geometric center 12,
    // matching pause), and the skip bars are filled rects (a stroked
    // line with stroke none paints nothing, leaving a one-sided
    // triangle that reads as off-center).
    let play = include_str!("../assets/icons/play.svg");
    assert!(play.contains("points=\"5 3 19 12 5 21 5 3\""));
    assert!(include_str!("../assets/icons/skip-back.svg").contains("<rect"));
    assert!(include_str!("../assets/icons/skip-forward.svg").contains("<rect"));
    assert!(!include_str!("../assets/icons/skip-back.svg").contains("<line"));
    assert!(!include_str!("../assets/icons/skip-forward.svg").contains("<line"));
    // The repeat button is icon-only (no text badge): Repeat Off
    // renders the muted glyph, All/One render the white glyph on the
    // red CTA, and One uses the repeat-1 asset (the 1 is in the icon).
    let _ = repeat_button("repeat", false, 6);
    let _ = repeat_button("repeat-1", true, 6);
    assert_eq!(iced::font::Weight::Medium, medium_font().weight);
}

#[cfg(feature = "innertube")]
#[test]
fn sync_stays_quiet_with_spinner_state() {
    let mut s = test_state();
    // A sync pull stages through the same pipeline as an import,
    // but stays quiet: no top banner. Spinner visibility is exactly
    // `syncing()` (Elements are not comparable, so the state bit is
    // the assertion; building the element locks the render path).
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Sync,
        result: Ok(staged(vec![], vec![], vec![("PL1", "One")])),
    });
    assert!(s.import_active);
    assert!(s.syncing());
    assert!(s.import_banner_view().is_none());
    let _ = s.sync_spinner();
    // The spinner turns while a sync runs: one frame per tick.
    let f0 = s.spin_frame;
    let _ = s.update(Message::Animate);
    assert_eq!(s.spin_frame, f0 + 1);
    let _ = s.sync_spinner();
    // Final arrival ends the sync (spinner drops, still no banner).
    let gen = s.import_gen;
    let _ = s.update(Message::PlaylistImported {
        kind: ImportKind::Sync,
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
    assert!(!s.syncing());
    assert!(s.import_banner_view().is_none());
    let _ = s.sync_spinner();
    // Idle ticks never advance the spinner.
    let f1 = s.spin_frame;
    let _ = s.update(Message::Animate);
    assert_eq!(s.spin_frame, f1);
    // Error paths clear the pair too: a failed stage must never
    // wedge the UI into a silent stuck sync.
    s.import_active = true;
    s.sync_active = true;
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Sync,
        result: Err("boom".into()),
    });
    assert!(!s.import_active);
    assert!(!s.syncing());
    assert!(s.import_banner_view().is_none());
    // Login-only busy (no import yet) still turns the button
    // spinners: the ticker runs on any pipeline busy.
    s.login_busy = true;
    assert!(s.pipeline_busy());
    let f2 = s.spin_frame;
    let _ = s.update(Message::Animate);
    assert_eq!(s.spin_frame, f2 + 1);
    let _ = s.busy_content("Sync now", 12.0);
    s.login_busy = false;
    assert!(!s.pipeline_busy());
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
        seeded: Vec::new(),
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
        seeded: Vec::new(),
        result: Err("500".into()),
    });
    assert!(s.user_playlists.iter().any(|p| p.id == local2));
    assert!(s.status.contains("locally"));
    // Unknown shell (deleted mid-flight): orphan reports, no panic.
    let _ = s.update(Message::ServerPlaylistCreated {
        local_id: "u999".into(),
        title: "Ghost".into(),
        seeded: Vec::new(),
        result: Ok("PLghost1".into()),
    });
    assert!(s.status.contains("PLghost1"));
    assert!(!s.user_playlists.iter().any(|p| p.id == "PLghost1"));
    // Failed import never flips the session flag by itself.
    let _ = s.update(Message::ImportStageReady {
        kind: ImportKind::Cookie,
        result: Err("boom".into()),
    });
    #[cfg(feature = "debug-panel")]
    assert!(s.status.contains("Cookie import failed: boom"));
    #[cfg(not(feature = "debug-panel"))]
    assert_eq!(s.status, "Cookie import failed");
}

#[test]
fn eq_selection_reports_backend_honestly() {
    let mut s = test_state();
    let _ = s.update(Message::SelectEqPreset("Rock".into()));
    assert_eq!(s.eq_preset, "Rock");
    assert!(s.status.contains("EQ: Rock"));
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
fn drag_display_order_opens_gap_at_commit_target() {
    // Origin held by the ghost: the list shows the other rows plus one
    // gap (None) at the commit target.
    assert_eq!(
        drag_display_order(5, 1, 3),
        (
            vec![Some(0), Some(2), Some(3), None, Some(4)],
            vec![0, 1, 3, 4, 4, 5],
        )
    );
    // Own spot: gap sits where the origin was (rest layout, content held).
    assert_eq!(
        drag_display_order(3, 0, 0),
        (vec![None, Some(1), Some(2)], vec![0, 0, 2, 3])
    );
    // Dragging the last row to the top.
    assert_eq!(
        drag_display_order(4, 3, 0),
        (vec![None, Some(0), Some(1), Some(2)], vec![0, 0, 1, 2, 3],)
    );
    // Every lane id is a valid slot for its list length.
    for len in 2..=6 {
        for from in 0..len {
            for to in 0..len {
                let (seq, lanes) = drag_display_order(len, from, to);
                assert_eq!(seq.len(), len);
                assert_eq!(lanes.len(), len + 1);
                assert_eq!(seq.iter().filter(|s| s.is_none()).count(), 1);
                assert!(lanes.iter().all(|&s| s <= len));
            }
        }
    }
}

#[test]
fn drag_ghost_follows_cursor_and_clamps() {
    use iced::Size;
    let win = Some(Size::new(1280.0, 800.0));
    // Grab offset preserved: pressing 30px into the row keeps the cursor
    // 30px into the ghost (no jump on grab).
    assert_eq!(
        drag_ghost_pos(
            Some(iced::Point::new(500.0, 400.0)),
            win,
            300.0,
            64.0,
            Some(130.0),
            100.0,
        ),
        Some((470.0, 368.0))
    );
    // Unknown press: centered under the cursor.
    assert_eq!(
        drag_ghost_pos(
            Some(iced::Point::new(500.0, 400.0)),
            win,
            300.0,
            64.0,
            None,
            100.0
        ),
        Some((472.0, 368.0))
    );
    // Clamped to the window on every edge.
    assert_eq!(
        drag_ghost_pos(
            Some(iced::Point::new(10.0, 10.0)),
            win,
            300.0,
            64.0,
            Some(0.0),
            0.0
        ),
        Some((10.0, 0.0))
    );
    assert_eq!(
        drag_ghost_pos(
            Some(iced::Point::new(1270.0, 790.0)),
            win,
            300.0,
            64.0,
            Some(290.0),
            900.0,
        ),
        Some((980.0, 736.0))
    );
    // No cursor, no ghost.
    assert_eq!(drag_ghost_pos(None, win, 300.0, 64.0, None, 100.0), None);
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
fn empty_collapsed_rail_stays_visible() {
    let mut s = test_state();
    let _ = s.update(Message::ToggleRightPin);
    let _ = s.update(Message::CursorMoved(iced::Point::new(640.0, 400.0)));
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    assert!(!s.right_open());
    assert_eq!(s.right_target(), RIGHT_RAIL_W);
    // One queued song keeps the same rail while still collapsed.
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    assert_eq!(s.right_target(), RIGHT_RAIL_W);
    assert!(!s.right_open());
}

#[test]
fn collapsed_rail_window_pins_live_song() {
    // First song: live sits 1st.
    assert_eq!(collapsed_rail_window(Some(0), 10, 6), (0, 6));
    // One previous song: live stays 2nd while scrolling.
    assert_eq!(collapsed_rail_window(Some(1), 10, 6), (0, 6));
    assert_eq!(collapsed_rail_window(Some(5), 10, 6), (4, 10));
    // Tail cannot fill below: live sinks to 3rd+.
    assert_eq!(collapsed_rail_window(Some(9), 10, 6), (4, 10));
    assert_eq!(collapsed_rail_window(Some(8), 10, 6), (4, 10));
    // Short queue: everything shows.
    assert_eq!(collapsed_rail_window(Some(2), 4, 6), (0, 4));
    assert_eq!(collapsed_rail_window(None, 10, 6), (0, 6));
    assert_eq!(collapsed_rail_window(Some(0), 0, 6), (0, 0));
}

#[test]
fn collapsed_rail_capacity_fills_tall_windows() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    // 800px window minus player reserve fills well past the old 6.
    assert!(s.collapsed_rail_capacity() > 6);
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 400.0)));
    // Tiny windows keep the old minimum instead of vanishing.
    assert_eq!(s.collapsed_rail_capacity(), RAIL_MIN_CAP);
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
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert!(s.lyrics_fetch_pending("m1"));
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert!(s.lyrics_loading.is_empty());
    let _ = s.update(Message::ToggleDrawer);
    assert!(s.drawer_open);
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert!(s.lyrics_fetch_pending("m1"));
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert!(s.lyrics_loading.is_empty());
    // Re-open while loading does not double-fetch.
    let _ = s.update(Message::ToggleDrawer);
    let _ = s.update(Message::ToggleDrawer);
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert!(s.lyrics_fetch_pending("m1"));
}

#[test]
fn heart_icon_fills_when_liked() {
    pub(crate) const HEART: &str = include_str!("../assets/icons/heart.svg");
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
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert!(s.lyrics_fetch_pending("m1"));
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert!(s.lyrics_loading.is_empty());
    // A late arrival only clears its own provider key, never the
    // other provider's in-flight fetch for another track.
    s.lyrics_loading
        .insert(("m2".into(), LyricsProvider::Lrclib));
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::Lrclib,
        result: Err("gone".into()),
    });
    assert!(s.lyrics_fetch_pending("m2"));
    let _ = s.update(Message::LyricsReady {
        track_id: "m2".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Err("gone".into()),
    });
    assert!(s.lyrics_fetch_pending("m2"));
    let _ = s.update(Message::LyricsReady {
        track_id: "m2".into(),
        provider: LyricsProvider::Lrclib,
        result: Err("gone".into()),
    });
    assert!(!s.lyrics_fetch_pending("m2"));
}

#[test]
fn lyrics_provider_switch_is_instant_when_preloaded() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    assert_eq!(s.lyrics_provider, LyricsProvider::Lrclib);
    // Both providers preload at song start: no waiting on switch.
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert_eq!(s.lyrics_loading.len(), 2);
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Ok(LyricsResult {
            plain: "hello".into(),
            synced: vec![],
            instrumental: false,
        }),
    });
    // Switching to the preloaded provider needs no fetch: no
    // loading marker, no status spam.
    let _ = s.update(Message::SetLyricsProvider(LyricsProvider::YouTubeMusic));
    assert_eq!(s.lyrics_provider, LyricsProvider::YouTubeMusic);
    let stored = LibraryStore::open(&s.library_path)
        .expect("test store")
        .get_pref("lyrics_provider")
        .expect("pref read");
    assert_eq!(stored.as_deref(), Some("youtubemusic"));
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::YouTubeMusic)
    );
    // The `< >` stepper cycles back (and persists the default);
    // LRCLIB may still be in flight, but nothing new is spawned
    // for it.
    let pending_before = s.lyrics_loading.len();
    let _ = s.update(Message::CycleLyricsProvider(-1));
    assert_eq!(s.lyrics_provider, LyricsProvider::Lrclib);
    assert_eq!(s.lyrics_loading.len(), pending_before);
}

#[test]
fn lyrics_fallback_shows_silently_after_default_fails() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    // Non-default answers first while the default is in flight:
    // nothing displays yet. (Without fetch features nothing is
    // ever in flight, so it displays at once.)
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Ok(LyricsResult {
            plain: "hello".into(),
            synced: vec![],
            instrumental: false,
        }),
    });
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert_eq!(s.lyrics_display_entry("m1"), None);
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::YouTubeMusic)
    );
    // Once the default settles without a result, the cached
    // fallback displays (with no "via" label).
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::Lrclib,
        result: Err("no lyrics found".into()),
    });
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::YouTubeMusic)
    );
    assert!(!s.lyrics_fetch_pending("m1"));
}

#[test]
fn lyrics_waits_for_default_before_fallback() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::SetLyricsProvider(LyricsProvider::YouTubeMusic));
    // Fallback answers first while the default is in flight:
    // still loading, never a "via" flash. (Without fetch features
    // nothing is ever in flight, so it displays at once.)
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::Lrclib,
        result: Ok(LyricsResult {
            plain: "hola".into(),
            synced: vec![],
            instrumental: false,
        }),
    });
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    {
        assert_eq!(s.lyrics_display_entry("m1"), None);
        assert!(s.lyrics_fetch_pending("m1"));
    }
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::Lrclib)
    );
    // Default failure falls back to the cached result.
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Err("gone".into()),
    });
    assert_eq!(
        s.lyrics_display_entry("m1").map(|(p, _)| p),
        Some(LyricsProvider::Lrclib)
    );
    assert!(!s.lyrics_fetch_pending("m1"));
}

#[test]
fn lyrics_label_names_displayed_provider() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    // Nothing cached yet: label names the default we wait on.
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::Lrclib);
    // Fallback cached while the default is still in flight: still
    // loading, so the label still names the default. (Without fetch
    // features nothing is ever in flight, so it shows at once.)
    s.lyrics_cache.insert(
        ("m1".to_string(), LyricsProvider::YouTubeMusic),
        LyricsResult {
            plain: "hello".into(),
            synced: vec![],
            instrumental: false,
        },
    );
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::Lrclib);
    #[cfg(not(any(feature = "lyrics", feature = "innertube")))]
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::YouTubeMusic);
    // Default settled without a result: label follows the fallback
    // actually on screen (both directions).
    s.lyrics_loading.clear();
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::YouTubeMusic);
    let _ = s.update(Message::SetLyricsProvider(LyricsProvider::YouTubeMusic));
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::YouTubeMusic);
    s.lyrics_cache.insert(
        ("m1".to_string(), LyricsProvider::Lrclib),
        LyricsResult {
            plain: "hola".into(),
            synced: vec![],
            instrumental: false,
        },
    );
    assert_eq!(s.lyrics_shown_provider("m1"), LyricsProvider::YouTubeMusic);
}

#[test]
fn lyrics_manual_pin_disables_fallback() {
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    // Fallback answers first while the default is pending.
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::YouTubeMusic,
        result: Ok(LyricsResult {
            plain: "hello".into(),
            synced: vec![],
            instrumental: false,
        }),
    });
    #[cfg(any(feature = "lyrics", feature = "innertube"))]
    assert_eq!(s.lyrics_display_entry("m1"), None);
    // Pinning via the stepper disables the fallback: the pinned
    // default has nothing, so the empty state shows even though
    // the other provider has lyrics cached.
    let _ = s.update(Message::CycleLyricsProvider(1));
    assert_eq!(s.lyrics_provider, LyricsProvider::YouTubeMusic);
    let _ = s.update(Message::CycleLyricsProvider(-1));
    assert_eq!(s.lyrics_provider, LyricsProvider::Lrclib);
    let _ = s.update(Message::LyricsReady {
        track_id: "m1".into(),
        provider: LyricsProvider::Lrclib,
        result: Err("no lyrics found".into()),
    });
    assert_eq!(s.lyrics_display_entry("m1"), None);
    assert!(!s.lyrics_fetch_pending("m1"));
    // The preloaded fallback stays cached (a later switch back to
    // it is instant), it just never displays while pinned.
    let ytm_key = ("m1".to_string(), LyricsProvider::YouTubeMusic);
    assert!(s.lyrics_cache.contains_key(&ytm_key));
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
            is_video: false,
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
fn same_titled_albums_by_different_artists_stay_apart() {
    let mut s = test_state();
    for (id, artist) in [
        ("p1", "The Police"),
        ("p2", "The Police"),
        ("c1", "The Cure"),
    ] {
        let mut t = mk_track(id, id, artist);
        t.album = Some("Greatest Hits".into());
        s.catalog.insert(id.into(), t);
    }
    // Songs: each artist's page lists only that artist's songs.
    let police = AppState::album_route("Greatest Hits", "The Police");
    let cure = AppState::album_route("Greatest Hits", "The Cure");
    let ids = |s: &AppState, r: &str| -> Vec<String> {
        let mut v: Vec<String> = s.album_tracks(r).into_iter().map(|t| t.id).collect();
        v.sort();
        v
    };
    assert_eq!(ids(&s, &police), ["p1", "p2"]);
    assert_eq!(ids(&s, &cure), ["c1"]);
    assert_eq!(s.resolve_album_title(&police), "Greatest Hits");
    // Lookup: searched albums match on artist, never the first title hit.
    s.results.albums = vec![
        Album {
            id: "MPRE_cure".into(),
            title: "Greatest Hits".into(),
            artist: "The Cure".into(),
            year: Some(2001),
            thumbnail_url: None,
        },
        Album {
            id: "MPRE_police".into(),
            title: "Greatest Hits".into(),
            artist: "The Police".into(),
            year: Some(1992),
            thumbnail_url: None,
        },
    ];
    assert_eq!(s.album_browse_id(&police).as_deref(), Some("MPRE_police"));
    assert_eq!(s.album_browse_id(&cure).as_deref(), Some("MPRE_cure"));
    // Cache: opening one artist's album never answers for the other's.
    let _ = s.update(Message::OpenAlbum(cure.clone()));
    s.results.albums.clear();
    assert_eq!(s.album_browse_id(&cure).as_deref(), Some("MPRE_cure"));
    assert_eq!(s.album_browse_id(&police), None);
    let _ = s.update(Message::OpenAlbum(police.clone()));
    assert_eq!(s.route, Route::Album(police));
    let _ = s.view();
}

#[test]
fn album_page_cover_matches_the_card_not_a_video_frame() {
    let mut s = test_state();
    // Library holds the album's music video (video-frame thumbnail).
    let mut v = mk_track("vid", "Every Breath You Take", "The Police");
    v.album = Some("Greatest Hits".into());
    v.is_video = true;
    v.thumbnail_url = Some("https://example.invalid/video-frame.jpg".into());
    s.catalog.insert(v.id.clone(), v);
    let route = AppState::album_route("Greatest Hits", "The Police");
    // Video art only stands in when nothing better exists.
    assert_eq!(
        s.album_header_cover(&route).as_deref(),
        Some("https://example.invalid/video-frame.jpg")
    );
    // A song from the album beats the video.
    let mut song = mk_track("song", "Roxanne", "The Police");
    song.album = Some("Greatest Hits".into());
    song.thumbnail_url = Some("https://example.invalid/song.jpg".into());
    s.catalog.insert(song.id.clone(), song);
    assert_eq!(
        s.album_header_cover(&route).as_deref(),
        Some("https://example.invalid/song.jpg")
    );
    // The searched album's own art (what the card shows) wins.
    s.results.albums = vec![Album {
        id: "MPRE_police".into(),
        title: "Greatest Hits".into(),
        artist: "The Police".into(),
        year: Some(1992),
        thumbnail_url: Some("https://example.invalid/cover.jpg".into()),
    }];
    let card = s.album_cover(&s.results.albums[0].clone());
    assert_eq!(s.album_header_cover(&route), card);
    assert_eq!(s.album_header_cover("MPRE_police"), card);
    // A same-titled album by someone else never lends its art.
    let cure = Album {
        id: AppState::album_route("Greatest Hits", "The Cure"),
        title: "Greatest Hits".into(),
        artist: "The Cure".into(),
        year: None,
        thumbnail_url: None,
    };
    assert_eq!(s.album_cover(&cure), None);
}

#[test]
fn track_album_owner_accepts_featured_credits() {
    assert!(AppState::artist_matches_owner("Queen", "Queen"));
    assert!(AppState::artist_matches_owner(
        "Queen & David Bowie",
        "Queen"
    ));
    assert!(AppState::artist_matches_owner(
        "Queen, David Bowie",
        "Queen"
    ));
    assert!(!AppState::artist_matches_owner("Queensryche", "Queen"));
    assert!(!AppState::artist_matches_owner("The Cure", "The Police"));
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
            is_video: false,
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
            is_video: false,
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
            is_video: false,
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
        is_video: false,
    };
    assert!(s.hero_art_for(&stale).is_some());
}

#[test]
fn clear_hovers_resets_all() {
    let mut s = test_state();
    let _ = s.update(Message::TrackHover(Some("m1".into())));
    let _ = s.update(Message::PlaylistHover(Some("u1".into())));
    let _ = s.update(Message::PanelHover(2));
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
fn search_fields_grow_while_focused() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    let settle = |s: &mut AppState| {
        for _ in 0..200 {
            if !s.animating() {
                break;
            }
            let _ = s.update(Message::Animate);
        }
    };
    settle(&mut s);
    // Collapsed at rest: narrower than the full pill.
    let (_, rest) = s.search_pill_geom();
    let full = s.search_width();
    assert!(rest < full);
    // Collapsed widths fit the whole placeholder (measured text plus
    // the field chrome), never a share of the expanded width.
    let ph = text_line_width(s.language.search_placeholder());
    assert!(ph > 50.0, "placeholder measured {ph}");
    assert!((rest - (ph + SEARCH_PILL_CHROME)).abs() < 0.5);
    let pl_ph = text_line_width(s.language.playlist_filter_placeholder());
    assert!((s.pl_search_width() - (pl_ph + PL_SEARCH_CHROME)).abs() < 0.5);
    // Focus grows it to full width; blur collapses it again.
    let _ = s.update(Message::SearchFocused(true));
    assert!(s.animating());
    // Fixed clock, independent of the Settings speed: one tick moves
    // exactly tick/duration, and the eased width starts gently.
    s.anim_ms = ANIM_MAX_MS;
    let _ = s.update(Message::Animate);
    assert!((s.search_expand - ANIM_TICK_MS as f32 / SEARCH_ANIM_MS).abs() < 1e-4);
    let (_, first) = s.search_pill_geom();
    assert!(first > rest && first - rest < (full - rest) * s.search_expand);
    settle(&mut s);
    let (_, open) = s.search_pill_geom();
    assert!((open - full).abs() < 0.5);
    let _ = s.update(Message::SearchFocused(false));
    settle(&mut s);
    assert!((s.search_pill_geom().1 - rest).abs() < 0.5);
    // Typing in the top search counts as focus.
    let _ = s.update(Message::HomeSearchChanged("a".into()));
    assert!(s.search_focused);
    // Playlist search: focus expands, navigation drops the flag.
    let _ = s.update(Message::PlaylistSearchFocused(true));
    settle(&mut s);
    assert_eq!(s.pl_search_expand, 1.0);
    // It grows past its placeholder width but never past the header
    // pill's max (and its own action row caps it further).
    assert!(s.pl_search_width() > pl_ph + PL_SEARCH_CHROME);
    assert!(s.pl_search_width() <= s.search_full_width() + 0.5);
    assert!((s.search_full_width() - full).abs() < 0.5);
    s.navigate(Route::History);
    assert!(!s.pl_search_focused);
    settle(&mut s);
    assert_eq!(s.pl_search_expand, 0.0);
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
fn layout_mode_breakpoints() {
    assert_eq!(layout_mode(360.0), LayoutMode::Narrow);
    assert_eq!(layout_mode(799.0), LayoutMode::Narrow);
    assert_eq!(layout_mode(800.0), LayoutMode::Medium);
    assert_eq!(layout_mode(1099.0), LayoutMode::Medium);
    assert_eq!(layout_mode(1100.0), LayoutMode::Normal);
    assert_eq!(layout_mode(1599.0), LayoutMode::Normal);
    assert_eq!(layout_mode(1600.0), LayoutMode::Wide);
    assert_eq!(layout_mode(2560.0), LayoutMode::Wide);
    assert!(LayoutMode::Narrow.panels_overlay());
    assert!(LayoutMode::Medium.panels_overlay());
    assert!(!LayoutMode::Normal.panels_overlay());
    assert!(!LayoutMode::Wide.panels_overlay());
}

#[test]
fn center_width_is_window_minus_panels_and_gaps() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.sidebar_w = 235.0;
    s.right_w = 285.0;
    // 1280 - 16 pad - (235 + 8) - (285 + 8)
    assert!((s.center_width() - 728.0).abs() < 0.01);
    // Body: capped center minus 2 x 20 padding.
    assert!((s.content_width() - 688.0).abs() < 0.01);
    assert!((s.lyrics_col_w() - 560.0).abs() < 0.01);
}

fn settle(s: &mut AppState) {
    for _ in 0..80 {
        let _ = s.update(Message::Animate);
    }
}

#[test]
fn medium_mode_rails_ignore_pins_and_expand_over_content() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    assert!(s.sidebar_pinned && s.right_pinned);
    let _ = s.update(Message::WindowResized(iced::Size::new(900.0, 800.0)));
    settle(&mut s);
    // Pinned, but Medium collapses both panels to rails.
    assert!(!s.sidebar_open());
    assert!(!s.right_open());
    assert!((s.sidebar_slot_w() - SIDEBAR_RAIL_W).abs() < 0.01);
    assert!((s.right_slot_w() - RIGHT_RAIL_W).abs() < 0.01);
    assert!(s.sidebar_overlay_w().is_none());
    // Hovering the rail expands the panel above the content: the slot
    // (and so the center width) never moves.
    let center = s.center_width();
    let _ = s.update(Message::CursorMoved(iced::Point::new(20.0, 200.0)));
    settle(&mut s);
    assert!(s.sidebar_open());
    assert_eq!(s.sidebar_overlay_w(), Some(SIDEBAR_FULL_W));
    assert!((s.center_width() - center).abs() < 0.01);
    // Leaving the panel collapses it again.
    let _ = s.update(Message::CursorMoved(iced::Point::new(600.0, 200.0)));
    assert!(!s.sidebar_open());
    // Wide again: the saved pins come back.
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    settle(&mut s);
    assert!(s.sidebar_open() && s.right_open());
    assert!((s.sidebar_slot_w() - SIDEBAR_FULL_W).abs() < 0.01);
}

#[test]
fn narrow_mode_uses_drawers() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(420.0, 800.0)));
    settle(&mut s);
    assert_eq!(s.layout(), LayoutMode::Narrow);
    // No rails: the center gets the whole window.
    assert_eq!(s.sidebar_slot_w(), 0.0);
    assert_eq!(s.right_slot_w(), 0.0);
    assert!((s.center_width() - (420.0 - WINDOW_PAD_X)).abs() < 0.01);
    // Hover never opens a drawer.
    let _ = s.update(Message::CursorMoved(iced::Point::new(10.0, 200.0)));
    assert!(!s.sidebar_open());
    // Menu button opens the library drawer; the queue button swaps.
    let _ = s.update(Message::ToggleLibraryDrawer);
    assert!(s.sidebar_open() && s.drawers_open());
    let _ = s.update(Message::ToggleQueueDrawer);
    assert!(!s.sidebar_open() && s.right_open());
    settle(&mut s);
    // The queue drawer goes full width under 500px.
    assert_eq!(s.right_overlay_w(), Some(420.0 - WINDOW_PAD_X));
    // Esc closes it.
    let _ = s.update(Message::CloseContext);
    assert!(!s.drawers_open());
    // Picking a page from the library drawer closes it.
    let _ = s.update(Message::ToggleLibraryDrawer);
    let _ = s.update(Message::Navigate(Route::History));
    assert!(!s.sidebar_open());
    assert_eq!(s.route, Route::History);
}

/// Click at `at` on the real view (headless) and return the messages.
fn click_view(s: &AppState, size: iced::Size, at: iced::Point) -> Vec<Message> {
    let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
    sim.point_at(at);
    let _ = sim.simulate(iced_test::simulator::click());
    sim.into_messages().collect()
}

#[test]
fn narrow_window_stays_clickable_without_a_drawer() {
    let mut s = test_state();
    let size = iced::Size::new(420.0, 800.0);
    let _ = s.update(Message::WindowResized(size));
    settle(&mut s);
    assert!(!s.drawers_open());
    // Header menu button (left edge, header row): with no drawer open
    // the click reaches it instead of a leftover scrim.
    let menu = iced::Point::new(8.0 + 21.0, 16.0 + 21.0);
    let msgs = click_view(&s, size, menu);
    assert!(
        msgs.iter()
            .any(|m| matches!(m, Message::ToggleLibraryDrawer)),
        "menu click swallowed: {msgs:?}"
    );
    assert!(!msgs.iter().any(|m| matches!(m, Message::CloseDrawers)));
    // With the drawer open, the dimmed content closes it.
    let _ = s.update(Message::ToggleLibraryDrawer);
    settle(&mut s);
    let msgs = click_view(&s, size, iced::Point::new(400.0, 300.0));
    assert!(
        msgs.iter().any(|m| matches!(m, Message::CloseDrawers)),
        "scrim click missed: {msgs:?}"
    );
}

#[test]
fn tiny_windows_compact_the_library_and_rows() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    assert!(!s.sidebar_short());
    // Smallest window: the library panel scrolls as a whole, and song
    // rows drop the play count so the title keeps its room.
    let _ = s.update(Message::WindowResized(iced::Size::new(
        MIN_WINDOW_W,
        MIN_WINDOW_H,
    )));
    assert!(s.sidebar_short());
    let mut t = mk_track("pc1", "Song", "A");
    t.plays = Some("220K plays".into());
    assert_eq!(s.row_plays(&t), None);
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    assert_eq!(s.row_plays(&t), Some("220K plays"));
}

#[test]
fn narrow_header_swaps_gear_for_drawer_buttons() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    settle(&mut s);
    assert!(s.header_nav_arrows());
    let wide_lead = s.header_lead_w();
    let _ = s.update(Message::WindowResized(iced::Size::new(360.0, 720.0)));
    settle(&mut s);
    // Menu button only: the arrows have no room at 360px.
    assert!(!s.header_nav_arrows());
    assert!(s.header_lead_w() < wide_lead);
    // The pill starts right after the menu button and always keeps a
    // usable width.
    assert!((s.search_pill_left() - (8.0 + s.header_lead_w())).abs() < 0.01);
    let (_, w) = s.search_pill_geom();
    assert!(w >= 140.0, "pill too narrow: {w}");
}

#[test]
fn content_adapts_to_width() {
    let mut s = test_state();
    // Narrow phone-sized window: one quick-picks column, no row
    // heart, stacked headers, short titles, stacked Now Playing.
    let _ = s.update(Message::WindowResized(iced::Size::new(360.0, 720.0)));
    settle(&mut s);
    assert_eq!(s.quick_picks_cols(), 1);
    assert!(!s.rows_show_heart());
    assert!(s.hero_stacked());
    assert!(!s.np_two_col());
    assert!(row_title_budget(s.content_width()) < 45);
    // Normal window with both panels pinned: today's layout.
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    settle(&mut s);
    assert_eq!(s.quick_picks_cols(), 2);
    assert!(s.rows_show_heart());
    assert!(!s.hero_stacked());
    assert!(!s.np_two_col());
    assert_eq!(row_title_budget(s.content_width()), 45);
    assert_eq!(s.content_cap(), CENTER_MAX_W);
    // Wide: raised cap, side-by-side Now Playing.
    let _ = s.update(Message::WindowResized(iced::Size::new(1920.0, 1000.0)));
    settle(&mut s);
    assert_eq!(s.content_cap(), CENTER_MAX_W_WIDE);
    assert!(s.np_two_col());
    assert!(s.lyrics_col_w() <= 640.0 && s.lyrics_col_w() >= 280.0);
    let _ = s.update(Message::WindowResized(iced::Size::new(2560.0, 1080.0)));
    settle(&mut s);
    assert!((s.content_width() - (CENTER_MAX_W_WIDE - 2.0 * BODY_PAD_X)).abs() < 0.01);
}

#[test]
fn now_playing_backdrop_caches_and_caps() {
    let mut s = test_state();
    let px = ART_BLUR_PX as usize;
    let rgba = vec![128u8; px * px * 4];
    // A decode failure stores nothing and clears the in-flight mark.
    s.art_blur_loading.insert("u0".into());
    let _ = s.update(Message::ArtBlurReady {
        url: "u0".into(),
        rgba: None,
    });
    assert!(s.art_blur.is_empty());
    assert!(!s.art_blur_loading.contains("u0"));
    for i in 0..ART_BLUR_CAP + 1 {
        let _ = s.update(Message::ArtBlurReady {
            url: format!("u{i}"),
            rgba: Some(rgba.clone()),
        });
    }
    // Never grows past the cap; the latest cover is always kept.
    assert!(s.art_blur.len() <= ART_BLUR_CAP);
    assert!(s.art_blur.contains_key(&format!("u{ART_BLUR_CAP}")));
}

#[test]
fn backdrop_only_decodes_while_now_playing_is_open() {
    let mut s = test_state();
    let mut t = mk_track("bd1", "Song", "A");
    t.thumbnail_url = Some("https://example.invalid/bd1.jpg".into());
    s.queue.push(t);
    s.drawer_open = false;
    let _ = s.art_blur_task();
    assert!(s.art_blur_loading.is_empty());
    // Open but the cover is not on disk yet: nothing to decode.
    s.drawer_open = true;
    let _ = s.art_blur_task();
    assert!(s.art_blur_loading.is_empty());
}

#[test]
fn playlist_cover_is_the_first_song() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Covers".into()));
    let i = s
        .user_playlists
        .iter()
        .position(|p| p.title == "Covers")
        .unwrap();
    let pid = s.user_playlists[i].id.clone();
    assert!(s.playlist_cover_track(&pid).is_none());
    s.user_playlists[i].tracks = vec![mk_track("c1", "First", "A"), mk_track("c2", "Second", "B")];
    assert_eq!(s.playlist_cover_track(&pid).unwrap().id, "c1");
}

#[test]
fn page_backdrop_follows_collection_pages() {
    let mut s = test_state();
    assert!(s.page_backdrop_url().is_none());
    let _ = s.update(Message::CreatePlaylist("Glow".into()));
    let i = s
        .user_playlists
        .iter()
        .position(|p| p.title == "Glow")
        .unwrap();
    let mut t = mk_track("g1", "First", "A");
    t.thumbnail_url = Some("https://example.invalid/g1.jpg".into());
    s.user_playlists[i].tracks = vec![t.clone()];
    let pid = s.user_playlists[i].id.clone();
    let _ = s.update(Message::OpenPlaylist(pid));
    // The playlist page glows with its cover (the first song's hero art).
    assert_eq!(s.page_backdrop_url(), s.hero_art_for(&t));
    assert!(s.page_backdrop_url().is_some());
    // Other pages have no backdrop.
    s.navigate(Route::Settings);
    assert!(s.page_backdrop_url().is_none());
}

#[test]
fn page_backdrop_scrolls_away_with_the_page() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    let _ = s.update(Message::CreatePlaylist("Glow".into()));
    let i = s
        .user_playlists
        .iter()
        .position(|p| p.title == "Glow")
        .unwrap();
    let mut t = mk_track("g2", "First", "A");
    t.thumbnail_url = Some("https://example.invalid/g2.jpg".into());
    s.user_playlists[i].tracks = vec![t];
    let pid = s.user_playlists[i].id.clone();
    let _ = s.update(Message::OpenPlaylist(pid));
    // Nothing decoded yet: no layer.
    assert!(s.page_backdrop_layer().is_none());
    let url = s.page_backdrop_url().unwrap();
    s.art_blur.insert(
        url,
        iced::widget::image::Handle::from_rgba(1, 1, vec![200, 40, 40, 255]),
    );
    assert!(s.page_backdrop_layer().is_some());
    // Scrolled past the band: the layer is gone.
    let _ = s.update(Message::Scrolled("body", PAGE_BACKDROP_H + 1.0));
    assert!(s.page_backdrop_layer().is_none());
}

#[test]
fn playlist_cover_falls_back_to_row_art() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Fallback".into()));
    let i = s
        .user_playlists
        .iter()
        .position(|p| p.title == "Fallback")
        .unwrap();
    let mut t = mk_track("f1", "First", "A");
    t.thumbnail_url = Some("https://example.invalid/f1.jpg".into());
    s.user_playlists[i].tracks = vec![t.clone()];
    let pid = s.user_playlists[i].id.clone();
    // Neither loaded: the large URL (placeholder until it lands).
    assert_eq!(s.playlist_cover_url(&pid), s.hero_art_for(&t));
    // Only the row art is cached: the header uses it meanwhile.
    let small = s.art_url_for(&t, 40.0).unwrap();
    s.artwork.insert(
        small.clone(),
        (
            iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
            std::time::Instant::now(),
        ),
    );
    assert_eq!(s.playlist_cover_url(&pid), Some(small));
}

#[test]
fn playlist_search_never_outgrows_its_row() {
    // Every layout width (Medium rails included) and both languages:
    // fully expanded (focused) the search still leaves room for play,
    // Shuffle and the menu (Narrow: menu on its own line), measured the
    // way the pills render.
    for lang in [Language::English, Language::Spanish] {
        for w in (360..=2600).step_by(20) {
            let mut s = test_state();
            s.language = lang;
            let _ = s.update(Message::WindowResized(iced::Size::new(w as f32, 800.0)));
            settle(&mut s);
            s.pl_search_expand = 1.0;
            let row = if s.hero_stacked() {
                s.content_width()
            } else {
                s.content_width() - PLAYLIST_COVER_PX - 20.0
            };
            let shuffle = icon_pill_width(&s.language.shuffle_label());
            // Narrow puts the menu on its own line: the row holds play,
            // Shuffle and the search only.
            let menu = if s.pl_menu_own_line() {
                0.0
            } else {
                PILL_H + 8.0
            };
            let used = s.pl_search_width() + PILL_H + shuffle + 2.0 * 8.0 + menu;
            assert!(used <= row, "{lang:?} w={w}: row overflows: {used} > {row}");
        }
    }
}

#[test]
fn song_glow_uses_row_art_when_hero_is_missing() {
    let mut s = test_state();
    let mut t = mk_track("gl1", "Song", "A");
    t.thumbnail_url = Some("https://example.invalid/gl1.jpg".into());
    assert!(s.track_blur(&t).is_none());
    let small = s.art_url_for(&t, 40.0).unwrap();
    s.art_blur.insert(
        small,
        iced::widget::image::Handle::from_rgba(1, 1, vec![9, 9, 9, 255]),
    );
    assert!(s.track_blur(&t).is_some());
}

#[test]
fn playlist_glow_is_ready_on_open() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Ready".into()));
    let i = s
        .user_playlists
        .iter()
        .position(|p| p.title == "Ready")
        .unwrap();
    let mut t = mk_track("rd1", "First", "A");
    t.thumbnail_url = Some("https://example.invalid/rd1.jpg".into());
    s.user_playlists[i].tracks = vec![t.clone()];
    let pid = s.user_playlists[i].id.clone();
    // Decoded ahead (here: from the row art) before the page opens.
    let small = s.art_url_for(&t, 40.0).unwrap();
    s.art_blur.insert(
        small,
        iced::widget::image::Handle::from_rgba(1, 1, vec![1, 2, 3, 255]),
    );
    let _ = s.update(Message::OpenPlaylist(pid));
    // The first frame already has the glow.
    assert!(s.page_backdrop_handle().is_some());
    assert!(s.page_backdrop_layer().is_some());
}

#[test]
fn history_search_follows_the_playlist_search_rules() {
    let mut s = test_state();
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    s.navigate(Route::History);
    settle(&mut s);
    // Compact at rest: just the placeholder plus field chrome.
    let ph = text_line_width(s.language.history_filter_placeholder());
    let rest = s.history_search_width();
    assert!((rest - (ph + PL_SEARCH_CHROME)).abs() < 0.5);
    // Typing counts as focus and grows it, within the content width.
    let _ = s.update(Message::HistoryFilterChanged("a".into()));
    settle(&mut s);
    let open = s.history_search_width();
    assert!(open > rest);
    assert!(open <= s.content_width() + 0.5);
    // Leaving the page drops the focus, so it starts compact next time.
    s.navigate(Route::Home);
    settle(&mut s);
    assert_eq!(s.pl_search_expand, 0.0);
}

#[test]
fn collection_total_length_format() {
    assert_eq!(fmt_total(0), "0 min");
    assert_eq!(fmt_total(20), "1 min");
    assert_eq!(fmt_total(46 * 60 + 40), "46 min");
    assert_eq!(fmt_total(2 * 3600 + 51 * 60), "2 h 51 min");
}

#[test]
fn row_title_budget_bounds() {
    assert_eq!(row_title_budget(0.0), 12);
    assert_eq!(row_title_budget(2000.0), 45);
    let mid = row_title_budget(400.0);
    assert!(mid > 12 && mid < 45);
}

#[test]
fn window_has_a_minimum_size() {
    let settings = window_settings();
    assert_eq!(
        settings.min_size,
        Some(iced::Size::new(MIN_WINDOW_W, MIN_WINDOW_H))
    );
}

/// Fake cover backdrop for screenshots (no network): a warm-to-cool
/// 8x8 ramp stored under `url`.
fn fake_backdrop(s: &mut AppState, url: String) {
    let px = ART_BLUR_PX as usize;
    let mut rgba = Vec::with_capacity(px * px * 4);
    for y in 0..px {
        for x in 0..px {
            let fx = x as f32 / (px - 1) as f32;
            let fy = y as f32 / (px - 1) as f32;
            rgba.extend_from_slice(&[
                (230.0 * (1.0 - fx)) as u8,
                (60.0 + 120.0 * fy) as u8,
                (200.0 * fx) as u8,
                255,
            ]);
        }
    }
    s.art_blur.insert(
        url,
        iced::widget::image::Handle::from_rgba(ART_BLUR_PX, ART_BLUR_PX, rgba),
    );
}

/// Headless layout screenshots (no display, no network): renders the
/// whole window for each page and panel state at every layout width
/// and writes PNGs to `YUTU_SNAPSHOT_DIR` (default
/// `target/layout-snapshots`). Art stays on placeholders. Run with
/// `cargo test -p yutu-ui --lib layout_snapshots -- --ignored`.
#[test]
#[ignore]
fn layout_snapshots() {
    let dir = std::env::var("YUTU_SNAPSHOT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/layout-snapshots")
        });
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("YUTU_SNAPSHOT_ONLY").unwrap_or_default();
    let sizes: [(f32, f32); 9] = [
        // Smallest allowed window (short: the library panel scrolls).
        (MIN_WINDOW_W, MIN_WINDOW_H),
        (360.0, 720.0),
        (420.0, 800.0),
        (600.0, 800.0),
        // Narrow layout with the full-width player bar (tightest bar).
        (720.0, 800.0),
        (900.0, 800.0),
        (1280.0, 800.0),
        (1920.0, 1000.0),
        (2560.0, 1080.0),
    ];
    let names = [
        "home",
        "search",
        "playlist",
        "album",
        "settings",
        "nowplaying",
        "history",
        "library",
        "queue",
        "newplaylist",
        "importplaylist",
        "importsummary",
        "favorites",
    ];
    for (w, h) in sizes {
        for name in names {
            if !only.is_empty() && !only.split(',').any(|o| o == name) {
                continue;
            }
            let mut s = test_state();
            let tracks: Vec<Track> = (0..14)
                .map(|i| {
                    let mut t = mk_track(
                        &format!("t{i}"),
                        &format!("Song title number {i} with a longer name"),
                        &format!("Artist {i}"),
                    );
                    t.album = Some("A Rather Long Album Title".into());
                    t.thumbnail_url = Some(format!("https://example.invalid/t{i}.jpg"));
                    t.plays = Some("220K plays".into());
                    t
                })
                .collect();
            for t in &tracks {
                s.queue.push(t.clone());
            }
            s.queue.jump_to(2);
            s.results.tracks = tracks.clone();
            s.all_results.tracks = tracks.clone();
            s.all_results.mixed.clear();
            for pl in [
                "Favorites mix",
                "Rave",
                "Chill music",
                "Rock en espa\u{f1}ol",
            ] {
                let _ = s.update(Message::CreatePlaylist(pl.into()));
            }
            let _ = s.update(Message::WindowResized(iced::Size::new(w, h)));
            match name {
                "search" => {
                    s.search_query = "song".into();
                    s.navigate(Route::Search);
                }
                "playlist" => {
                    s.user_playlists[1].tracks = tracks.clone();
                    let pid = s.user_playlists[1].id.clone();
                    let _ = s.update(Message::OpenPlaylist(pid));
                }
                "album" => {
                    let _ = s.update(Message::OpenAlbum("A Rather Long Album Title".into()));
                }
                "settings" => s.navigate(Route::Settings),
                "history" => s.navigate(Route::History),
                "nowplaying" => {
                    s.drawer_open = true;
                    let url = s.queue.current().and_then(|t| s.hero_art_for(t)).unwrap();
                    fake_backdrop(&mut s, url);
                }
                "library" => {
                    if s.layout().panels_overlay() {
                        s.sidebar_latched = true;
                    }
                    // Hovered row shows its play button.
                    s.user_playlists[1].tracks = tracks.clone();
                    s.hovered_playlist = Some(s.user_playlists[1].id.clone());
                }
                "queue" => {
                    if s.layout().panels_overlay() {
                        s.right_latched = true;
                    }
                }
                "favorites" => {
                    for t in &tracks[..5] {
                        s.like_id(&t.id);
                    }
                    let _ = s.update(Message::OpenPlaylist(AppState::FAVORITES_ID.into()));
                }
                "newplaylist" => {
                    s.navigate(Route::Home);
                    let _ = s.update(Message::OpenNewPlaylist);
                    let _ = s.update(Message::NewPlaylistTitleChanged("Road trip".into()));
                }
                "importplaylist" => {
                    s.navigate(Route::Home);
                    let _ = s.update(Message::OpenNewPlaylist);
                    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
                    let _ = s.update(Message::NewPlaylistLinkChanged(
                        "https://music.youtube.com/watch?v=x".into(),
                    ));
                    let _ = s.update(Message::SubmitNewPlaylist);
                }
                #[cfg(feature = "innertube")]
                "importsummary" => {
                    s.navigate(Route::Home);
                    let _ = s.update(Message::OpenNewPlaylist);
                    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
                    let _ = s.update(Message::NewPlaylistLinkChanged(
                        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".into(),
                    ));
                    let _ = s.update(Message::SubmitNewPlaylist);
                    let gen = s.new_playlist_gen;
                    let _ = s.update(Message::PlaylistImportFetched {
                        generation: gen,
                        platform: "Spotify".into(),
                        result: Ok(LinkImport {
                            title: "Salsita".into(),
                            tracks: tracks.clone(),
                            swapped: 0,
                            missing: (0..1)
                                .map(|i| format!("Some Artist {i} - A song that was not found"))
                                .collect(),
                            limit: None,
                            source_rows: 138,
                            repeats: 123,
                        }),
                    });
                }
                _ => s.navigate(Route::Home),
            }
            if let Some(url) = s.page_backdrop_url() {
                fake_backdrop(&mut s, url);
            }
            // Optional body scroll (px) to check the backdrop scrolls away.
            if let Some(y) = std::env::var("YUTU_SNAPSHOT_SCROLL")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
            {
                s.scroll_offsets.insert("body", y);
            }
            for _ in 0..80 {
                let _ = s.update(Message::Animate);
            }
            let settings = iced::Settings {
                fonts: vec![include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf")
                    .as_slice()
                    .into()],
                default_font: iced::Font::with_name("Adwaita Sans"),
                ..iced::Settings::default()
            };
            let theme = s.theme();
            let mut sim =
                iced_test::Simulator::with_size(settings, iced::Size::new(w, h), s.view());
            let snap = sim.snapshot(&theme).unwrap();
            // The renderer suffixes the file name (e.g. `-wgpu`), and an
            // existing file is compared instead of rewritten: clear both.
            let tag = if h < 600.0 {
                format!("{w}x{h}")
            } else {
                format!("{w}")
            };
            let path = dir.join(format!("{name}-{tag}.png"));
            for f in std::fs::read_dir(&dir).unwrap().flatten() {
                let n = f.file_name().to_string_lossy().into_owned();
                if n.starts_with(&format!("{name}-{tag}-")) || n == format!("{name}-{tag}.png") {
                    let _ = std::fs::remove_file(f.path());
                }
            }
            snap.matches_image(&path).unwrap();
        }
    }
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
fn mute_toggle_keeps_level_and_persists() {
    let mut s = test_state();
    assert!(!s.muted);
    // Muting keeps the slider level (backend output goes silent).
    let _ = s.update(Message::SetVolume(0.6));
    let _ = s.update(Message::ToggleMute);
    assert!(s.muted);
    assert_eq!(s.volume, 0.6);
    let _ = s.update(Message::ToggleMute);
    assert!(!s.muted);
    assert_eq!(s.volume, 0.6);
    // Dragging the slider always unmutes.
    let _ = s.update(Message::ToggleMute);
    let _ = s.update(Message::SetVolume(0.3));
    assert!(!s.muted);
    assert_eq!(s.volume, 0.3);
    // The flag survives in the session snapshot.
    let snap = s.queue_snapshot();
    assert!(!snap.muted);
    s.muted = true;
    let snap = s.queue_snapshot();
    assert!(snap.muted);
    assert_eq!(snap.volume, 0.3);
    // Both player layouts build with the button in place.
    let _ = s.update(Message::WindowResized(iced::Size::new(1280.0, 800.0)));
    let _ = s.view();
    let _ = s.update(Message::WindowResized(iced::Size::new(400.0, 800.0)));
    let _ = s.view();
    let _ = s.mute_button();
}

#[test]
fn mouse_backdismisses_menu_before_navigating() {
    let mut s = test_state();
    s.navigate(Route::Search);
    s.navigate(Route::Discovery);
    let _ = s.update(Message::OpenContext("m1".into()));
    assert!(s.context_menu.is_some());
    let _ = s.update(Message::NavBack);
    assert!(s.context_menu.is_none());
    assert_eq!(s.route, Route::Discovery);
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
        is_video: false,
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
    // Rows take the small display raster, not the raw upstream URL
    // (bounds per-image decode cost on long lists).
    let row = s.art_url_for(&ytm, 40.0).expect("row");
    assert!(row.contains("=w120-h120"), "{row}");
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
        thumbnail_large_url: Some("https://yt3.googleusercontent.com/x=w120-h120-l90-rj".into()),
        plays: None,
        is_video: false,
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
    // The choice persists: a fresh state on the same library path
    // reopens with it (garbage falls back to the default).
    let _ = s.update(Message::AnimMsChanged(250.0));
    assert_eq!(s.anim_ms, 250);
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.anim_ms, 250);
    let Ok(store) = LibraryStore::open(&s.library_path) else {
        panic!("test library must open");
    };
    let _ = store.set_pref("anim_ms", "junk");
    let (fallback, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(fallback.anim_ms, ANIM_DEFAULT_MS);
}

#[test]
fn font_prefs_default_parse_and_persist() {
    // Adwaita Sans renders the same outlines on Linux and macOS, so it is
    // the default; System keeps the old platform-sans behavior.
    let s = test_state();
    assert_eq!(s.font_family, FontFamily::Adwaita);
    assert_eq!(FontFamily::parse("system"), FontFamily::System);
    assert_eq!(FontFamily::parse("SYSTEM "), FontFamily::System);
    assert_eq!(FontFamily::parse("junk"), FontFamily::Adwaita);
    assert_eq!(FontFamily::parse(""), FontFamily::Adwaita);
    assert_eq!(FontFamily::parse("name:"), FontFamily::Adwaita);
    assert_eq!(
        FontFamily::parse("NAME:DejaVu Sans"),
        FontFamily::parse("name:DejaVu Sans")
    );
    // Custom families round-trip through storage.
    let custom = FontFamily::parse("name:DejaVu Sans");
    assert_eq!(custom, FontFamily::Custom(intern_font_name("DejaVu Sans")));
    assert_eq!(FontFamily::parse(&custom.stored()), custom);
    assert_eq!(custom.label(), "DejaVu Sans");
    // Picker rows map back: heads are special, the rest is raw.
    assert_eq!(
        FontFamily::from_label("Adwaita Sans (bundled)"),
        FontFamily::Adwaita
    );
    assert_eq!(FontFamily::from_label("System default"), FontFamily::System);
    assert_eq!(FontFamily::from_label("DejaVu Sans"), custom);
    // Interning leaks once: the same name returns one pointer.
    let (a, b) = (
        intern_font_name("DejaVu Sans"),
        intern_font_name("DejaVu Sans"),
    );
    assert!(std::ptr::eq(a, b));
    // The choice persists: a fresh state on the same library path
    // reopens with it (garbage falls back to the default).
    let mut s = s;
    let _ = s.update(Message::FontFamilyChanged(custom));
    assert_eq!(s.font_family, custom);
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.font_family, custom);
    let Ok(store) = LibraryStore::open(&s.library_path) else {
        panic!("test library must open");
    };
    let _ = store.set_pref("font_family", "junk");
    let (fallback, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(fallback.font_family, FontFamily::Adwaita);
}

#[test]
fn system_family_rows_come_sorted_and_deduped() {
    // Memory-loaded database (no system scan): the bundled variable
    // face loads as one family.
    let mut db = fontdb::Database::new();
    let bytes = include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf").as_slice();
    db.load_font_data(bytes.to_vec());
    assert_eq!(family_names_of(&db), vec!["Adwaita Sans".to_string()]);
    assert!(family_names_of(&fontdb::Database::new()).is_empty());
    // The picker heads the list with the two pinned rows; an
    // installed Adwaita Sans stays covered by the bundled head.
    let rows = font_option_rows(&[
        "DejaVu Sans".to_string(),
        "Adwaita Sans".to_string(),
        "DejaVu Sans".to_string(),
    ]);
    assert_eq!(rows[0], "Adwaita Sans (bundled)");
    assert_eq!(rows[1], "System default");
    assert!(rows.contains(&"DejaVu Sans".to_string()));
    assert!(!rows.iter().any(|r| r == "Adwaita Sans"));
    // The async scan lands through the message (heads only until
    // then, since tests never run the startup task).
    let mut s = test_state();
    assert_eq!(
        s.font_rows,
        vec![
            "Adwaita Sans (bundled)".to_string(),
            "System default".to_string()
        ]
    );
    let _ = s.update(Message::SystemFontsLoaded(vec![
        "B".to_string(),
        "A".to_string(),
        "A".to_string(),
    ]));
    assert_eq!(s.system_font_rows, vec!["A".to_string(), "B".to_string()]);
    assert_eq!(s.font_rows[2..], ["A".to_string(), "B".to_string()]);
}

#[test]
fn missing_font_note_only_for_gone_custom_families() {
    let system = vec!["DejaVu Sans".to_string()];
    assert!(!is_font_missing(FontFamily::Adwaita, &system));
    assert!(!is_font_missing(FontFamily::System, &system));
    // Bundled Adwaita Sans renders without any installed namesake.
    assert!(!is_font_missing(
        FontFamily::Custom(intern_font_name("Adwaita Sans")),
        &system
    ));
    assert!(!is_font_missing(
        FontFamily::Custom(intern_font_name("Gone")),
        &[]
    ));
    assert!(is_font_missing(
        FontFamily::Custom(intern_font_name("Gone")),
        &system
    ));
    assert!(!is_font_missing(
        FontFamily::Custom(intern_font_name("DejaVu Sans")),
        &system
    ));
}

#[test]
fn font_pending_rule() {
    assert!(!font_pending(FontFamily::Adwaita, FontFamily::Adwaita));
    assert!(!font_pending(FontFamily::System, FontFamily::System));
    assert!(font_pending(FontFamily::System, FontFamily::Adwaita));
    let (a, b) = (
        FontFamily::Custom(intern_font_name("DejaVu Sans")),
        FontFamily::Custom(intern_font_name("Noto Sans")),
    );
    assert!(!font_pending(a, a));
    assert!(font_pending(a, b));
    assert!(font_pending(
        FontFamily::Custom(intern_font_name("DejaVu Sans")),
        FontFamily::Adwaita
    ));
}

#[test]
fn font_change_status_names_the_restart() {
    // Pure in both args: no test depends on the process-wide
    // effective family (only run() sets it, and tests never do).
    assert_eq!(
        font_pick_status(FontFamily::Adwaita, FontFamily::Adwaita),
        None
    );
    assert_eq!(
        font_pick_status(FontFamily::System, FontFamily::System),
        None
    );
    let note = font_pick_status(FontFamily::System, FontFamily::Adwaita)
        .expect("changed pick must note the restart");
    assert!(note.contains("relaunch"), "{note}");
    // The no-op pick leaves the status alone (release builds show
    // failures only, so it must never erase a visible failure).
    let mut s = test_state();
    let before = s.status.clone();
    let _ = s.update(Message::FontFamilyChanged(FontFamily::Adwaita));
    assert_eq!(s.status, before);
    let _ = s.update(Message::FontFamilyChanged(FontFamily::System));
    assert!(s.status.contains("relaunch"), "{}", s.status);
}

#[test]
fn ui_scale_clamp_persist_and_nan_fallback() {
    let s = test_state();
    assert_eq!(s.ui_scale, UI_SCALE_DEFAULT);
    assert_eq!(clamp_scale(99.0), UI_SCALE_MAX);
    assert_eq!(clamp_scale(0.01), UI_SCALE_MIN);
    assert_eq!(clamp_scale(f32::NAN), UI_SCALE_DEFAULT);
    assert_eq!(clamp_scale(f32::INFINITY), UI_SCALE_DEFAULT);
    // The slider clamps into range and persists across restarts.
    let mut s = s;
    let _ = s.update(Message::UiScaleChanged(99.0));
    assert_eq!(s.ui_scale, UI_SCALE_MAX);
    let _ = s.update(Message::UiScaleChanged(0.01));
    assert_eq!(s.ui_scale, UI_SCALE_MIN);
    let _ = s.update(Message::UiScaleChanged(1.25));
    assert_eq!(s.ui_scale, 1.25);
    let (reopened, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(reopened.ui_scale, 1.25);
    let Ok(store) = LibraryStore::open(&s.library_path) else {
        panic!("test library must open");
    };
    let _ = store.set_pref("ui_scale", "junk");
    let (fallback, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(fallback.ui_scale, UI_SCALE_DEFAULT);
    let _ = store.set_pref("ui_scale", "NaN");
    let (nan_fallback, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(nan_fallback.ui_scale, UI_SCALE_DEFAULT);
}

#[test]
fn button_fonts_follow_the_effective_family() {
    // Explicit-weight fonts must share the body family (a pinned
    // SansSerif here would split buttons from Adwaita Sans body text).
    assert_eq!(medium_font().weight, iced::font::Weight::Medium);
    assert_eq!(bold_font().weight, iced::font::Weight::Bold);
    assert_eq!(medium_font().family, effective_font().to_iced_font().family);
    assert_eq!(bold_font().family, effective_font().to_iced_font().family);
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
#[cfg(feature = "debug-panel")]
fn status_text_is_full_log_in_debug() {
    let mut s = test_state();
    s.status = "Loading x".into();
    assert_eq!(s.status_text(), "Loading x");
}

#[test]
#[cfg(not(feature = "debug-panel"))]
fn status_text_without_flag_shows_failures_only() {
    let mut s = test_state();
    s.status = "Loading x".into();
    assert_eq!(s.status_text(), "");
    s.status = "Search failed".into();
    assert_eq!(s.status_text(), "Search failed");
}

#[test]
#[cfg(feature = "debug-panel")]
fn failure_status_keeps_detail_in_debug() {
    // The flag shows the full line (release label shape is pinned
    // by the test below).
    assert_eq!(
        AppState::failure_status("Search failed".into(), "boom"),
        "Search failed: boom"
    );
    // End to end: a search failure still reports the line here.
    let mut s = test_state();
    s.search_query = "x".into();
    let _ = s.update(Message::SearchCompleted {
        query: "x".into(),
        result: Err("boom".into()),
    });
    assert_eq!(s.status, "Search failed: boom");
}

#[test]
#[cfg(not(feature = "debug-panel"))]
fn failure_status_without_flag_shows_label_only() {
    assert_eq!(
        AppState::failure_status("Search failed".into(), "boom"),
        "Search failed"
    );
    let mut s = test_state();
    s.search_query = "x".into();
    let _ = s.update(Message::SearchCompleted {
        query: "x".into(),
        result: Err("boom".into()),
    });
    assert_eq!(s.status, "Search failed");
}

#[test]
#[cfg(feature = "debug-panel")]
fn status_row_collapses_when_empty() {
    let mut s = test_state();
    s.status.clear();
    assert!(s.status_row().is_none());
    s.status = "Loading x".into();
    assert!(s.status_row().is_some());
}

#[test]
#[cfg(not(feature = "debug-panel"))]
fn status_row_without_flag_hides_chatter() {
    let mut s = test_state();
    s.status.clear();
    assert!(s.status_row().is_none());
    s.status = "Loading x".into();
    assert!(s.status_row().is_none());
    s.status = "Search failed".into();
    assert!(s.status_row().is_some());
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
fn drag_arms_invisible_until_moved() {
    // A press arms without going live: no lanes, no slot, no gap,
    // so a plain click never shifts rows under the cursor.
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::EnqueueTrack("m2".into()));
    let _ = s.update(Message::EnqueueTrack("m3".into()));
    let at = iced::Point::new(10.0, 10.0);
    let _ = s.update(Message::CursorMoved(at));
    let _ = s.update(Message::PanelHover(0));
    let _ = s.update(Message::DragStart(0));
    assert!(s.drag.is_some());
    assert!(!s.dragging());
    assert_eq!(s.effective_drag_slot(), None);
    assert_eq!(s.drag_gap_target(), 0.0);
    // Tremor inside slop stays armed (still a click on release).
    let _ = s.update(Message::CursorMoved(iced::Point::new(12.0, 11.0)));
    assert!(!s.dragging());
    // Past slop goes live and the lanes appear.
    let _ = s.update(Message::CursorMoved(iced::Point::new(40.0, 40.0)));
    assert!(s.dragging());
}

#[test]
fn armed_slot_without_move_never_reorders() {
    // A stale slot latched on an armed (unmoved) press must not
    // commit: only live drags reorder. Direct field write keeps
    // `live` false the way the DragSlot message never can.
    let mut s = test_state();
    let _ = s.update(Message::EnqueueTrack("m1".into()));
    let _ = s.update(Message::EnqueueTrack("m2".into()));
    let _ = s.update(Message::EnqueueTrack("m3".into()));
    let _ = s.update(Message::PanelHover(0));
    let _ = s.update(Message::DragStart(0));
    assert!(!s.dragging());
    if let Some(drag) = s.drag.as_mut() {
        drag.slot = Some(2);
    }
    assert_eq!(s.effective_drag_slot(), None);
    assert_eq!(s.drag_gap_target(), 0.0);
    let _ = s.update(Message::DragEnd);
    let ids: Vec<&str> = s.queue.items().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["m1", "m2", "m3"]);
}

#[test]
fn live_drag_extra_height_is_constant() {
    // Rest height = rows + dividers; live drag reuses the dividers
    // for middle slots and adds only the two 10px edge lanes, so the
    // delta is a constant 20px for any queue length (never O(n)).
    let rest = |n: usize| {
        if n == 0 {
            0.0
        } else {
            n as f32 * QUEUE_ROW_H + (n as f32 - 1.0) * DIVIDER_PX
        }
    };
    let live_idle = |n: usize| rest(n) + 20.0;
    for n in [2usize, 3, 10, 100] {
        assert_eq!(live_idle(n) - rest(n), 20.0, "n = {n}");
    }
    // The constants behind the math: dividers stay 5px, edges 10px.
    assert_eq!(DIVIDER_PX, 5.0);
    assert_eq!(QUEUE_ROW_H, 64.0);
}

#[test]
fn panel_hover_tracks_cursor() {
    let mut s = test_state();
    assert!(s.hovered_queue.is_none());
    let _ = s.update(Message::PanelHover(2));
    assert_eq!(s.hovered_queue, Some(2));
    let _ = s.update(Message::QueueExit(2));
    assert!(s.hovered_queue.is_none());
    // Id-keyed row hover for non-index lists (payloads carry the
    // scope-joined key; see `track_hover_key`).
    assert!(s.hovered_track.is_none());
    let _ = s.update(Message::TrackHover(Some("m1".into())));
    assert_eq!(s.hovered_track.as_deref(), Some("m1"));
    let _ = s.update(Message::TrackHover(None));
    assert!(s.hovered_track.is_none());
}

#[test]
fn stale_exit_never_wipes_fresh_hover() {
    // Iced publishes enter/exit in tree order per event, so a fast
    // multi-row jump emits enter(new) before exit(old). The exit
    // must only clear when it still names the hovered row.
    let mut s = test_state();
    // Index-keyed (queue): enter row 1, then the stale exit for
    // row 5 from the same batch arrives late.
    let _ = s.update(Message::PanelHover(1));
    let _ = s.update(Message::QueueExit(5));
    assert_eq!(s.hovered_queue, Some(1));
    // Matching exit still clears.
    let _ = s.update(Message::QueueExit(1));
    assert!(s.hovered_queue.is_none());
    // Scope-keyed (track rows across the whole app).
    let _ = s.update(Message::TrackHover(Some("m2".into())));
    let _ = s.update(Message::TrackExit("m1".into()));
    assert_eq!(s.hovered_track.as_deref(), Some("m2"));
    let _ = s.update(Message::TrackExit("m2".into()));
    assert!(s.hovered_track.is_none());
    // Cross-family: leaving an autoplay row must not wipe a fresh
    // queue hover latched earlier in the same batch.
    let _ = s.update(Message::PanelHover(0));
    let _ = s.update(Message::AutoExit(3));
    assert_eq!(s.hovered_queue, Some(0));
    // Remove-button hover: a stale exit must not disarm the × the
    // cursor sits on (a press there must delete, never drag).
    let _ = s.update(Message::RemoveHover(Some(2)));
    let _ = s.update(Message::RemoveExit(0));
    assert_eq!(s.hover_remove, Some(2));
    let _ = s.update(Message::RemoveExit(2));
    assert!(s.hover_remove.is_none());
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
    // Retries stay quiet; only the give-up is logged.
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("403 Forbidden".into()),
    });
    assert!(s.error_log.is_empty());
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("403 Forbidden".into()),
    });
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

#[test]
fn prefetch_due_fires_two_seconds_before_end() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    // Window opens exactly 2s before the end (inclusive).
    assert_eq!(s.prefetch_due(198.0, 200.0).as_deref(), Some("m2"));
    assert_eq!(s.prefetch_due(197.9, 200.0), None);
    // Unknown/short durations never time an end.
    assert_eq!(s.prefetch_due(0.0, 0.0), None);
    assert_eq!(s.prefetch_due(0.0, 2.0), None);
    // Busy states hold their fire.
    s.paused = true;
    assert_eq!(s.prefetch_due(199.0, 200.0), None);
    s.paused = false;
    s.prefetched = Some(("m2".into(), "https://example.invalid/x".into()));
    assert_eq!(s.prefetch_due(199.0, 200.0), None);
    s.prefetched = None;
    s.loading_track = Some("m1".into());
    assert_eq!(s.prefetch_due(199.0, 200.0), None);
}

/// Failed loads retry the same track, then skip forward.
/// Returns the task so tests prove a refetch actually fires.
fn failed_load(s: &mut AppState) -> iced::Task<Message> {
    let id = s.loading_track.clone().expect("a load is in flight");
    s.update(Message::UrlResolved {
        track_id: id,
        result: Err("boom".into()),
    })
}

#[test]
fn failed_resolve_retries_then_skips() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.loading_track = Some("m1".into());
    // Two failures retry the same load (never stuck, never moved).
    let t = failed_load(&mut s);
    assert!(t.units() > 0, "retry must refetch");
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    assert_eq!(s.resolve_attempts, 1);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    let t = failed_load(&mut s);
    assert!(t.units() > 0, "retry must refetch");
    assert_eq!(s.resolve_attempts, 2);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    // The third failure skips to the next song with a fresh budget.
    let t = failed_load(&mut s);
    assert!(t.units() > 0, "skip must start the next load");
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    assert_eq!(s.loading_track.as_deref(), Some("m2"));
    assert_eq!(s.resolve_attempts, 0);
    // The skip note is permanent in the error log (the loading
    // status takes over the line immediately).
    assert!(s.error_log.iter().any(|l| l.contains("Giving up on m1")));
}

#[test]
fn resolve_gives_up_at_end_of_queue() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.loading_track = Some("m1".into());
    let _ = failed_load(&mut s);
    let _ = failed_load(&mut s);
    let t = failed_load(&mut s);
    assert_eq!(t.units(), 0, "give-up issues no work");
    // Nowhere to go: honest error, loader cleared, no retry loop.
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert!(s.loading_track.is_none());
    assert!(s.status.contains("end of queue"));
    // A fourth failure cannot arrive (nothing loading): silent.
    let before = s.status.clone();
    let _ = s.update(Message::UrlResolved {
        track_id: "m1".into(),
        result: Err("boom".into()),
    });
    assert_eq!(s.status, before);
}

#[test]
fn eof_idle_advances_a_silenced_track() {
    use std::time::Instant;
    // Predicate shape: silent backend + heard playback qualifies;
    // every other state stays put. Pinned to no audio device so the
    // clock-only shape is deterministic with or without libmpv.
    let mut s = test_state();
    s.audio = None;
    s.queue.push(mk_track("m1", "One", "A"));
    assert!(!s.eof_idle(0.0, 0.0), "load gap never qualifies");
    s.heard_audio = true;
    assert!(s.eof_idle(0.0, 0.0));
    s.paused = true;
    assert!(!s.eof_idle(0.0, 0.0), "paused holds");
    s.paused = false;
    s.loading_track = Some("m1".into());
    assert!(!s.eof_idle(0.0, 0.0), "loading holds");
    s.loading_track = None;
    s.end_advanced = true;
    assert!(!s.eof_idle(0.0, 0.0), "already advanced holds");
    s.end_advanced = false;
    assert!(!s.eof_idle(3.0, 200.0), "live clock holds");
    assert!(!s.eof_idle(0.0, 200.0), "known duration holds");
    // Full tick: no audio device reads 0/0 (the EOF-idle shape),
    // so a heard track advances exactly once.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.heard_audio = true;
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    // The next load is in flight (its flag pins the net until audio).
    assert_eq!(s.loading_track.as_deref(), Some("m2"));
    // Unheard (a load that never started) never advances.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    // Stop disarms even with nothing heard since.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.heard_audio = true;
    let _ = s.update(Message::Stop);
    assert!(!s.heard_audio);
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
}

#[test]
#[cfg(feature = "mpv")]
fn eof_idle_advances_on_idle_backend_with_stale_duration() {
    // The duration property can outlive the file on some builds, so
    // 0/0 alone can miss the end: an idle backend (no file) is the
    // second witness. Needs real libmpv; skipped without it.
    let backend = match MpvBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("SKIP: mpv unavailable in this environment: {e}");
            return;
        }
    };
    assert!(backend.is_idle(), "fresh backend holds no file");
    let mut s = test_state();
    s.audio = Some(AudioOut::Mpv(backend));
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    assert!(!s.eof_idle(0.0, 200.0), "unheard load never qualifies");
    s.heard_audio = true;
    assert!(
        s.eof_idle(0.0, 200.0),
        "idle backend advances despite stale duration"
    );
    s.paused = true;
    assert!(!s.eof_idle(0.0, 200.0), "paused holds");
    s.paused = false;
    s.loading_track = Some("m1".into());
    assert!(!s.eof_idle(0.0, 200.0), "loading holds");
    s.loading_track = None;
    s.end_advanced = true;
    assert!(!s.eof_idle(0.0, 200.0), "already advanced holds");
    s.end_advanced = false;
    // Full tick advances exactly once on the idle witness.
    use std::time::Instant;
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
}

#[test]
fn mpris_pause_during_drag_cancels_resume() {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.paused = false;
    let _ = s.update(Message::SeekPreview(50.0));
    assert!(s.seek_paused);
    let handle: yutu_desktop::SharedPlayer =
        Arc::new(Mutex::new(yutu_desktop::MprisPlayer::default()));
    s.mpris = Some(handle.clone());
    // Explicit headset pause wins: release must not resume.
    handle
        .lock()
        .unwrap()
        .push_command(yutu_desktop::PlayerCommand::Pause);
    let _ = s.update(Message::Tick(Instant::now()));
    assert!(!s.seek_paused);
    let _ = s.update(Message::SeekRelease);
    assert!(s.paused);
}

#[test]
fn mpris_stop_then_play_does_not_skip() {
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.heard_audio = true;
    let handle: yutu_desktop::SharedPlayer =
        Arc::new(Mutex::new(yutu_desktop::MprisPlayer::default()));
    s.mpris = Some(handle.clone());
    // Widget stop disarms the net...
    handle
        .lock()
        .unwrap()
        .push_command(yutu_desktop::PlayerCommand::Stop);
    let _ = s.update(Message::Tick(Instant::now()));
    assert!(!s.heard_audio);
    // ...so a later resume on the silent backend holds instead of
    // skipping to the next track.
    handle
        .lock()
        .unwrap()
        .push_command(yutu_desktop::PlayerCommand::Play);
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
}

#[test]
fn seek_drag_pauses_and_commits_on_release() {
    use std::time::Instant;
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.paused = false;
    // Preview: display follows, live playback pauses exactly once.
    let _ = s.update(Message::SeekPreview(50.0));
    assert_eq!(s.seek_drag, Some(50.0));
    assert_eq!(s.position_secs, 50.0);
    assert!(s.paused);
    assert!(s.seek_paused);
    let _ = s.update(Message::SeekPreview(60.0));
    assert!(s.seek_paused, "second preview keeps the pause source");
    // Tick holds the preview (no backend reads, no advance).
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.position_secs, 50.0 + 10.0);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    // Release commits + resumes (idle backend reports the seek
    // honestly; the state transitions are what matter here).
    let _ = s.update(Message::SeekRelease);
    assert_eq!(s.seek_drag, None);
    assert!(!s.seek_paused);
    assert!(!s.paused);
    // A pre-paused song stays paused through the whole gesture.
    s.paused = true;
    let _ = s.update(Message::SeekPreview(30.0));
    assert!(!s.seek_paused);
    let _ = s.update(Message::SeekRelease);
    assert!(s.paused);
}

#[test]
fn seek_intents_preview_while_held() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.paused = false;
    let _ = s.update(Message::SeekPreview(50.0));
    // Keyboard/lyrics/MPRIS seeks become preview, never commits.
    let _ = s.update(Message::SeekTo(60.0));
    assert_eq!(s.seek_drag, Some(60.0));
    assert_eq!(s.position_secs, 60.0);
    let _ = s.update(Message::SeekToMs(70_000));
    assert_eq!(s.seek_drag, Some(70.0));
}

#[test]
fn orphaned_drags_expire_from_tick() {
    use std::time::{Duration, Instant};
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.paused = false;
    let _ = s.update(Message::SeekPreview(50.0));
    assert!(s.seek_paused);
    // Motionless for 10s (canvas flag lost): cancel + resume.
    s.seek_drag_at = Some(Instant::now() - Duration::from_secs(11));
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.seek_drag, None);
    assert!(!s.seek_paused);
    assert!(!s.paused);
}

#[test]
fn stop_clears_a_held_drag() {
    use std::time::Instant;
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.paused = false;
    let _ = s.update(Message::SeekPreview(50.0));
    let _ = s.update(Message::Stop);
    assert_eq!(s.seek_drag, None);
    assert!(!s.seek_paused);
    // Nothing held: the tick cannot advance or resume anything.
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
}

#[test]
fn seek_drag_to_the_end_never_advances_mid_drag() {
    use std::time::Instant;
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.heard_audio = true;
    // Preview parked at the end: both advance paths hold fire.
    let _ = s.update(Message::SeekPreview(199.0));
    assert_eq!(s.position_secs, 199.0);
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert!(s.loading_track.is_none());
}

#[test]
fn prev_restarts_past_threshold_otherwise_steps_back() {
    // Past 3s: restart in place, no new load starts.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    assert!(s.queue.jump_to(1));
    s.position_secs = 5.0;
    let _ = s.update(Message::Prev);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m2"));
    assert!(s.loading_track.is_none());
    // Boundary is exclusive: exactly 3.0 steps back instead.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    assert!(s.queue.jump_to(1));
    s.position_secs = 3.0;
    let _ = s.update(Message::Prev);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    // Head with no previous song restarts instead of reloading.
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    s.position_secs = 1.0;
    let _ = s.update(Message::Prev);
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert!(s.loading_track.is_none());
}

#[test]
fn stream_open_watchdog_needs_an_idle_stream_backend() {
    use std::time::{Duration, Instant};
    // No device (and rodio) never reports idle: the watchdog can
    // never fire there, whatever the clock says.
    let mut s = test_state();
    s.audio = None;
    s.queue.push(mk_track("m1", "One", "A"));
    let t0 = Instant::now();
    s.stream_opened_at = Some((t0, "m1".into()));
    assert!(!s.stream_open_failed(t0 + Duration::from_secs(60)));
}

#[test]
#[cfg(feature = "mpv")]
fn stream_open_watchdog_catches_a_load_that_never_opened() {
    use std::time::{Duration, Instant};
    // `loadfile` is async: a URL the edge refuses (HTTP 403) leaves
    // mpv idle with no error at the call site. A fresh backend is
    // exactly that shape (idle, nothing heard). Needs real libmpv.
    let backend = match MpvBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("SKIP: mpv unavailable in this environment: {e}");
            return;
        }
    };
    let mut s = test_state();
    s.audio = Some(AudioOut::Mpv(backend));
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    let t0 = Instant::now();
    let late = t0 + STREAM_OPEN_GRACE + Duration::from_millis(100);
    assert!(!s.stream_open_failed(late), "no load handed over yet");
    s.stream_opened_at = Some((t0, "m2".into()));
    assert!(
        !s.stream_open_failed(late),
        "armed for another track: never retargets the current one"
    );
    s.stream_opened_at = Some((t0, "m1".into()));
    assert!(
        !s.stream_open_failed(t0 + Duration::from_millis(500)),
        "inside the grace mpv may not have picked the file up"
    );
    assert!(s.stream_open_failed(late));
    s.paused = true;
    assert!(
        !s.stream_open_failed(late),
        "paused holds (caught on resume)"
    );
    s.paused = false;
    s.heard_audio = true;
    assert!(
        !s.stream_open_failed(late),
        "heard audio is the EOF net's job"
    );
    s.heard_audio = false;
    s.loading_track = Some("m1".into());
    assert!(!s.stream_open_failed(late), "a resolve in flight holds");
    s.loading_track = None;
    // Full tick: the dead URL is dropped and the same track
    // re-resolves through the normal retry budget.
    s.url_cache.insert(
        "m1".into(),
        ("https://example.invalid/dead".into(), u64::MAX),
    );
    s.prefetched = Some(("m1".into(), "https://example.invalid/dead".into()));
    let _ = s.update(Message::Tick(late));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert_eq!(s.loading_track.as_deref(), Some("m1"), "retrying");
    assert_eq!(s.resolve_attempts, 1);
    assert!(s.stream_opened_at.is_none(), "one retry per failed open");
    assert!(!s.url_cache.contains_key("m1"), "dead URL never reused");
    assert!(s.prefetched.is_none());
}

#[test]
#[cfg(feature = "mpv")]
fn stream_handoff_arms_the_watchdog_and_new_loads_disarm_it() {
    let backend = match MpvBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("SKIP: mpv unavailable in this environment: {e}");
            return;
        }
    };
    let mut s = test_state();
    s.audio = Some(AudioOut::Mpv(backend));
    let t = mk_track("m1", "One", "A");
    s.queue.push(t.clone());
    assert!(s.try_play_url("https://r1.googlevideo.com/audio", &t));
    assert!(s.stream_opened_at.is_some(), "handoff arms the watchdog");
    let _ = s.update(Message::Stop);
    assert!(s.stream_opened_at.is_none(), "Stop disarms it");
    assert!(s.try_play_url("https://r1.googlevideo.com/audio", &t));
    // No cached/prefetched URL: the new load resolves, disarmed.
    let _ = s.play_current_task();
    assert_eq!(s.loading_track.as_deref(), Some("m1"));
    assert!(s.stream_opened_at.is_none(), "new loads disarm it");
}

#[test]
#[cfg(feature = "mpv")]
fn removing_a_stuck_last_song_never_restarts_the_previous_one() {
    use std::time::{Duration, Instant};
    let backend = match MpvBackend::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("SKIP: mpv unavailable in this environment: {e}");
            return;
        }
    };
    let mut s = test_state();
    s.audio = Some(AudioOut::Mpv(backend));
    s.queue.push(mk_track("m1", "One", "A"));
    let t2 = mk_track("m2", "Two", "A");
    s.queue.push(t2.clone());
    s.queue.jump_to(1);
    assert!(s.try_play_url("https://r1.googlevideo.com/stuck", &t2));
    // Remove the stuck last song: no next one, so playback stops
    // and the index falls back to m1. The user stopped: nothing
    // may re-resolve m1 behind their back.
    let _ = s.update(Message::RemoveFromQueue("m2".into()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    let late = Instant::now() + STREAM_OPEN_GRACE + Duration::from_millis(100);
    assert!(!s.stream_open_failed(late));
    let _ = s.update(Message::Tick(late));
    assert!(s.loading_track.is_none(), "no retry of the previous song");
    assert_eq!(s.resolve_attempts, 0);
}

#[test]
fn eof_idle_holds_at_end_of_queue() {
    use std::time::Instant;
    // Nowhere to go: the net fires the predicate but the queue
    // holds silently (no advance loop, no tick spam).
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.heard_audio = true;
    assert!(s.eof_idle(0.0, 0.0));
    let before = s.status.clone();
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert!(s.loading_track.is_none());
    let _ = s.update(Message::Tick(Instant::now()));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert_eq!(s.status, before, "holding must not spam status");
}

#[test]
fn repeat_one_never_loops_on_failure() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.repeat = yutu_player::RepeatMode::One;
    s.loading_track = Some("m1".into());
    let _ = failed_load(&mut s);
    let _ = failed_load(&mut s);
    let _ = failed_load(&mut s);
    // Repeat would replay the same dead track: stop instead.
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
    assert!(s.loading_track.is_none());
    assert!(s.status.contains("end of queue"));
}

#[test]
fn stale_stream_errors_are_ignored() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "One", "A"));
    s.queue.push(mk_track("m2", "Two", "A"));
    // The user moved on mid-fetch: the old error must not retry
    // or skip the new song.
    s.loading_track = Some("m2".into());
    let _ = s.update(Message::StreamResolved {
        video_id: "m1".into(),
        result: Err("boom".into()),
    });
    assert_eq!(s.resolve_attempts, 0);
    assert_eq!(s.loading_track.as_deref(), Some("m2"));
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("m1"));
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

#[test]
fn tick_skips_home_shelf_ranking_until_inputs_change() {
    let mut s = test_state();
    s.route = Route::Home;
    let t0 = std::time::Instant::now();
    let _ = s.update(Message::Tick(t0));
    let key = s.home_shelves_key.expect("first tick ranks the shelves");
    // Nothing changed: the next tick keeps the memo (same stamp).
    let _ = s.update(Message::Tick(t0 + std::time::Duration::from_millis(500)));
    assert_eq!(s.home_shelves_key.map(|(_, at)| at), Some(key.1));
    // A new play moves the fingerprint: the next tick re-ranks.
    s.history.push("fresh-play".into());
    let _ = s.update(Message::Tick(t0 + std::time::Duration::from_millis(1000)));
    let (k2, _) = s.home_shelves_key.unwrap();
    assert_ne!(k2, key.0);
    assert_eq!(k2, s.home_shelves_fingerprint());
}

#[test]
fn durationless_rows_never_erase_a_known_length() {
    let mut s = test_state();
    let _ = s.register_tracks(vec![mk_track("len-1", "Song", "A")]);
    let mut flat = mk_track("len-1", "Song", "A");
    flat.duration_secs = 0;
    let out = s.register_tracks_mem(vec![flat]);
    assert_eq!(out[0].duration_secs, 200);
    assert_eq!(s.find_track("len-1").unwrap().duration_secs, 200);
}

#[test]
fn stored_rows_borrow_the_catalog_length_and_unknown_is_blank() {
    let mut s = test_state();
    let _ = s.register_tracks(vec![mk_track("len-2", "Song", "A")]);
    // A playlist/history row saved before the length was known.
    let mut stored = mk_track("len-2", "Song", "A");
    stored.duration_secs = 0;
    assert_eq!(s.track_secs(&stored), 200);
    let mut unknown = mk_track("len-none", "Video", "B");
    unknown.duration_secs = 0;
    assert_eq!(s.track_secs(&unknown), 0);
    assert_eq!(fmt_len(0), "");
    assert_eq!(fmt_len(301), "5:01");
}

#[test]
fn playback_teaches_a_missing_length_once() {
    let mut s = test_state();
    let mut video = mk_track("len-3", "Video", "B");
    video.duration_secs = 0;
    let _ = s.register_tracks(vec![video.clone()]);
    s.learn_duration(&video, 143.4);
    assert_eq!(s.find_track("len-3").unwrap().duration_secs, 143);
    assert_eq!(s.track_secs(&video), 143);
    // A known length is never overwritten by a later backend report.
    s.learn_duration(&video, 999.0);
    assert_eq!(s.find_track("len-3").unwrap().duration_secs, 143);
    // Persisted for the next launch (background write).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let store = LibraryStore::open(&s.library_path).unwrap();
        if store
            .cached_track("len-3")
            .unwrap()
            .is_some_and(|t| t.duration_secs == 143)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "learned length never persisted"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
/// Selector that runs [`LyricRowProbe`]-equivalent logic over the
/// headless simulator tree (iced_test exposes selectors, not raw ops).
struct ProbeSel {
    row_id: iced::widget::Id,
    scroll: Option<(iced::Rectangle, iced::Rectangle)>,
}

impl iced_test::Selector for ProbeSel {
    type Output = (iced::Rectangle, iced::Rectangle, iced::Rectangle);
    fn select(&mut self, c: iced_test::selector::Candidate<'_>) -> Option<Self::Output> {
        use iced_test::selector::Candidate;
        match c {
            Candidate::Scrollable {
                id,
                bounds,
                content_bounds,
                ..
            } if id == Some(&iced::widget::Id::new(LYRICS_SCROLL_ID)) => {
                self.scroll = Some((bounds, content_bounds));
                None
            }
            Candidate::Container { id, bounds, .. } if id == Some(&self.row_id) => {
                self.scroll.map(|(b, cb)| (b, cb, bounds))
            }
            _ => None,
        }
    }
    fn description(&self) -> String {
        "lyric row probe".into()
    }
}

/// Short windows: the live drawer layout puts the followed verse in
/// the middle of the measured viewport, and the estimate alone does
/// not (wrapping drift). Run with `--ignored` (needs the renderer).
#[test]
#[ignore]
fn lyric_probe_measures_live_layout_in_short_window() {
    let mut s = synced_state(0.0);
    let line = "Ni perder el vigor, ni decir sin rigor que todo tiempo";
    let synced: Vec<yutu_core::LyricLine> = (0..40)
        .map(|i| yutu_core::LyricLine {
            start_ms: i * 3000,
            text: if i % 5 == 4 {
                String::new()
            } else {
                format!("{line} {i}")
            },
        })
        .collect();
    s.lyrics_cache
        .get_mut(&("m1".to_string(), LyricsProvider::Lrclib))
        .unwrap()
        .synced = synced;
    let (w, h) = (370.0, 490.0);
    let _ = s.update(Message::WindowResized(iced::Size::new(w, h)));
    s.position_secs = 70.0;
    s.drawer_open = true;
    s.lyrics_follow = true;
    // Let the drawer slide finish (it renders off-screen until then).
    for _ in 0..80 {
        let _ = s.update(Message::Animate);
    }
    let at = s.lyric_cursor_at("m1").unwrap();
    let _ = s.lyric_follow_task();
    let estimate = s.lyrics_target;
    let settings = iced::Settings {
        fonts: vec![include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf")
            .as_slice()
            .into()],
        default_font: iced::Font::with_name("Adwaita Sans"),
        ..iced::Settings::default()
    };
    let mut sim = iced_test::Simulator::with_size(settings, iced::Size::new(w, h), s.view());
    let (b, cb, row) = sim
        .find(ProbeSel {
            row_id: lyric_row_id(at),
            scroll: None,
        })
        .expect("probe finds the followed row");
    drop(sim);
    let g = LyricRowGeom {
        track: "m1".into(),
        at,
        top: row.y - cb.y,
        h: row.height,
        view_h: b.height,
        max_y: cb.height - b.height,
    };
    eprintln!(
        "at={at} estimate={estimate:.1} measured={:.1} view_h={:.1} row_top={:.1} row_h={:.1}",
        g.center_y(),
        g.view_h,
        g.top,
        g.h
    );
    let _ = s.update(Message::LyricRowMeasured(g.clone()));
    assert!((s.lyrics_target - g.center_y()).abs() < 0.01);
    // At the measured target the row sits fully inside the viewport.
    let row_top_on_screen = g.top - s.lyrics_target;
    assert!(row_top_on_screen >= 0.0 && row_top_on_screen + g.h <= g.view_h);
}

#[test]
fn volume_slider_stays_capped_on_wide_windows() {
    let mut s = test_state();
    let size = iced::Size::new(1786.0, 900.0);
    let _ = s.update(Message::WindowResized(size));
    settle(&mut s);
    let hits = |x: f32| {
        // Scan the player bar rows for any volume vote at column `x`.
        (0..60).step_by(4).any(|dy| {
            click_view(&s, size, iced::Point::new(x, size.height - 8.0 - dy as f32))
                .iter()
                .any(|m| matches!(m, Message::SetVolume(_)))
        })
    };
    // Inside the 120px cap, right of the mute button and left of the
    // Now Playing toggle: the slider answers.
    assert!(hits(size.width - 100.0), "slider not found inside its cap");
    // Far left of the cap (where the uncapped slider used to reach):
    // nothing.
    assert!(!hits(size.width - 350.0), "volume slider grew past its cap");
}

#[test]
fn new_playlist_dialog_creates_and_opens_the_list() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    assert!(s.new_playlist.is_some());
    assert!(s.modal_open());
    // Empty title: the dialog stays open with an inline error.
    let _ = s.update(Message::SubmitNewPlaylist);
    let form = s.new_playlist.as_ref().unwrap();
    assert!(form.error.is_some());
    assert!(s.user_playlists.is_empty());
    // Editing clears the error.
    let _ = s.update(Message::NewPlaylistTitleChanged("Road Trip".into()));
    assert!(s.new_playlist.as_ref().unwrap().error.is_none());
    let _ = s.update(Message::NewPlaylistDescriptionChanged("Long drives".into()));
    let _ = s.update(Message::NewPlaylistPrivacyChanged(PlaylistPrivacy::Public));
    assert_eq!(
        s.new_playlist.as_ref().unwrap().privacy,
        PlaylistPrivacy::Public
    );
    let _ = s.update(Message::SubmitNewPlaylist);
    assert!(s.new_playlist.is_none());
    assert_eq!(s.user_playlists.len(), 1);
    let id = s.user_playlists[0].id.clone();
    assert_eq!(s.user_playlists[0].title, "Road Trip");
    assert_eq!(s.route, Route::PlaylistDetail(id));
}

#[test]
fn new_playlist_dialog_closes_on_esc_and_cancel() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::CloseContext);
    assert!(s.new_playlist.is_none());
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistTitleChanged("Kept?".into()));
    let _ = s.update(Message::CloseNewPlaylist);
    assert!(s.new_playlist.is_none());
    assert!(s.user_playlists.is_empty());
    // Reopening starts from a blank form.
    let _ = s.update(Message::OpenNewPlaylist);
    assert_eq!(s.new_playlist.as_ref().unwrap().title, "");
}

#[test]
fn import_rejects_links_that_name_no_playlist() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged(
        "https://music.youtube.com/watch?v=dQw4w9WgXcQ".into(),
    ));
    let gen = s.new_playlist_gen;
    let _ = s.update(Message::SubmitNewPlaylist);
    let form = s.new_playlist.as_ref().unwrap();
    assert!(!form.busy);
    assert_eq!(
        form.error.as_deref(),
        Some(s.language.new_playlist_bad_link().as_str())
    );
    assert_eq!(s.new_playlist_gen, gen);
}

#[cfg(feature = "innertube")]
#[test]
fn import_from_link_copies_the_source_playlist() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged(
        "https://music.youtube.com/playlist?list=PLsource1234567&si=abc".into(),
    ));
    let _ = s.update(Message::SubmitNewPlaylist);
    assert!(s.new_playlist.as_ref().unwrap().busy);
    let gen = s.new_playlist_gen;
    let tracks = vec![
        mk_track("i1", "One", "A"),
        mk_track("i2", "Two", "B"),
        mk_track("i1", "One", "A"),
    ];
    // A stale arrival (older generation) is dropped.
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen - 1,
        platform: "YouTube Music".into(),
        result: Ok(LinkImport {
            title: "Stale".into(),
            tracks: tracks.clone(),
            swapped: 0,
            missing: Vec::new(),
            limit: None,
            source_rows: 0,
            repeats: 0,
        }),
    });
    assert!(s.user_playlists.is_empty());
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "YouTube Music".into(),
        result: Ok(LinkImport {
            title: "Summer Hits".into(),
            tracks,
            swapped: 0,
            missing: Vec::new(),
            limit: None,
            source_rows: 3,
            repeats: 0,
        }),
    });
    // The repeated song is explained, so the dialog ends on a summary.
    let sum = s.new_playlist.as_ref().unwrap().summary.clone().unwrap();
    assert_eq!((sum.total, sum.imported, sum.repeats), (3, 2, 1));
    let _ = s.update(Message::CloseNewPlaylist);
    assert!(s.new_playlist.is_none());
    assert_eq!(s.user_playlists.len(), 1);
    let pl = &s.user_playlists[0];
    // Source title by default, duplicates collapsed.
    assert_eq!(pl.title, "Summer Hits");
    let ids: Vec<&str> = pl.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["i1", "i2"]);
    assert_eq!(s.route, Route::PlaylistDetail(pl.id.clone()));
    // Persisted: a fresh load sees the copy with its songs.
    let store = LibraryStore::open(&s.library_path).unwrap();
    let saved = store.user_playlists().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].tracks.len(), 2);
}

#[cfg(feature = "innertube")]
#[test]
fn import_keeps_a_typed_title_and_reports_failures_inline() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged("PLsource1234567".into()));
    let _ = s.update(Message::NewPlaylistTitleChanged("Mine".into()));
    let _ = s.update(Message::SubmitNewPlaylist);
    let gen = s.new_playlist_gen;
    // Fetch failure: the dialog stays, unlocked, with a human error.
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "YouTube Music".into(),
        result: Err("HTTP 404".into()),
    });
    let form = s.new_playlist.as_ref().unwrap();
    assert!(!form.busy);
    assert_eq!(
        form.error.as_deref(),
        Some(s.language.new_playlist_import_failed().as_str())
    );
    // Retry succeeds and the typed title wins over the source's.
    let _ = s.update(Message::SubmitNewPlaylist);
    let gen = s.new_playlist_gen;
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "YouTube Music".into(),
        result: Ok(LinkImport {
            title: "Source".into(),
            tracks: vec![mk_track("i1", "One", "A")],
            swapped: 0,
            missing: Vec::new(),
            limit: None,
            source_rows: 0,
            repeats: 0,
        }),
    });
    assert_eq!(s.user_playlists[0].title, "Mine");
}

#[cfg(feature = "innertube")]
#[test]
fn closing_the_dialog_drops_a_running_import() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged("PLsource1234567".into()));
    let _ = s.update(Message::SubmitNewPlaylist);
    let gen = s.new_playlist_gen;
    let _ = s.update(Message::CloseNewPlaylist);
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "YouTube Music".into(),
        result: Ok(LinkImport {
            title: "Late".into(),
            tracks: vec![mk_track("i1", "One", "A")],
            swapped: 0,
            missing: Vec::new(),
            limit: None,
            source_rows: 0,
            repeats: 0,
        }),
    });
    assert!(s.user_playlists.is_empty());
    assert!(s.new_playlist.is_none());
}

#[cfg(feature = "innertube")]
#[test]
fn import_shows_song_search_progress_and_reports_swaps() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged("PLsource1234567".into()));
    let _ = s.update(Message::SubmitNewPlaylist);
    let gen = s.new_playlist_gen;
    // Stale ticks are ignored, live ones land on the dialog.
    let _ = s.update(Message::PlaylistImportProgress {
        generation: gen - 1,
        done: 9,
        total: 9,
    });
    assert_eq!(s.new_playlist.as_ref().unwrap().progress, None);
    let _ = s.update(Message::PlaylistImportProgress {
        generation: gen,
        done: 3,
        total: 8,
    });
    assert_eq!(s.new_playlist.as_ref().unwrap().progress, Some((3, 8)));
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "YouTube Music".into(),
        result: Ok(LinkImport {
            title: "Mixed".into(),
            tracks: vec![mk_track("s1", "One", "A"), mk_track("s2", "Two", "B")],
            swapped: 2,
            missing: Vec::new(),
            limit: None,
            source_rows: 0,
            repeats: 0,
        }),
    });
    assert!(s.new_playlist.is_none());
    assert_eq!(
        s.status,
        "Imported Mixed (2 songs, 2 videos swapped for their song version)."
    );
}

#[test]
fn clicking_inside_a_dialog_card_never_dismisses_it() {
    let mut s = test_state();
    let size = iced::Size::new(1280.0, 800.0);
    let _ = s.update(Message::WindowResized(size));
    let _ = s.update(Message::OpenNewPlaylist);
    for _ in 0..80 {
        let _ = s.update(Message::Animate);
    }
    // A plain label inside the card: no widget of its own claims the
    // press, so it used to fall through to the scrim and close the card.
    let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
    sim.click("Visibility")
        .or_else(|_| sim.click("Title"))
        .expect("a card label is on screen");
    let msgs: Vec<Message> = sim.into_messages().collect();
    assert!(
        !msgs.iter().any(|m| matches!(m, Message::CloseNewPlaylist)),
        "inside click dismissed: {msgs:?}"
    );
    // The backdrop outside the card still dismisses.
    let msgs = click_view(&s, size, iced::Point::new(8.0, 400.0));
    assert!(
        msgs.iter().any(|m| matches!(m, Message::CloseNewPlaylist)),
        "outside click kept it open: {msgs:?}"
    );
}

#[cfg(feature = "innertube")]
fn start_import(s: &mut AppState, link: &str) -> u64 {
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::NewPlaylistLinkChanged(link.into()));
    let _ = s.update(Message::SubmitNewPlaylist);
    assert!(
        s.new_playlist.as_ref().unwrap().busy,
        "{link} did not start"
    );
    s.new_playlist_gen
}

#[cfg(feature = "innertube")]
#[test]
fn every_platform_link_and_csv_path_starts_an_import() {
    for link in [
        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=x",
        "https://www.deezer.com/en/playlist/3155776842",
        "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb",
        "/home/me/export.csv",
    ] {
        let mut s = test_state();
        let _ = start_import(&mut s, link);
    }
}

#[cfg(feature = "innertube")]
#[test]
fn picked_files_fill_the_link_field() {
    let mut s = test_state();
    let _ = s.update(Message::OpenNewPlaylist);
    let _ = s.update(Message::NewPlaylistModeChanged(NewPlaylistMode::Import));
    let _ = s.update(Message::ImportFilePicked(None));
    assert_eq!(s.new_playlist.as_ref().unwrap().link, "");
    let _ = s.update(Message::ImportFilePicked(Some("/tmp/mix.csv".into())));
    assert_eq!(s.new_playlist.as_ref().unwrap().link, "/tmp/mix.csv");
}

#[cfg(feature = "innertube")]
#[test]
fn foreign_imports_with_misses_end_on_a_summary() {
    let mut s = test_state();
    let gen = start_import(
        &mut s,
        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
    );
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "Spotify".into(),
        result: Ok(LinkImport {
            title: "Top Hits".into(),
            tracks: vec![mk_track("s1", "One", "A"), mk_track("s2", "Two", "B")],
            swapped: 0,
            missing: vec!["C - Three".into()],
            limit: Some(yutu_core::playlist_sources::ReadLimit::SpotifyCap(100)),
            source_rows: 0,
            repeats: 0,
        }),
    });
    // The playlist exists and is open behind the dialog...
    assert_eq!(s.user_playlists.len(), 1);
    assert_eq!(
        s.route,
        Route::PlaylistDetail(s.user_playlists[0].id.clone())
    );
    // ...and the dialog stays on the summary until Done.
    let sum = s.new_playlist.as_ref().unwrap().summary.clone().unwrap();
    assert_eq!(sum.imported, 2);
    assert_eq!(sum.missing, ["C - Three"]);
    let _ = s.update(Message::CloseNewPlaylist);
    assert!(s.new_playlist.is_none());
    assert_eq!(s.user_playlists.len(), 1);
}

#[cfg(feature = "innertube")]
#[test]
fn foreign_import_failures_name_the_platform() {
    let mut s = test_state();
    let gen = start_import(&mut s, "https://www.deezer.com/en/playlist/3155776842");
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "Deezer".into(),
        result: Err("deezer: no data".into()),
    });
    let form = s.new_playlist.as_ref().unwrap();
    assert!(!form.busy);
    assert!(form.error.as_deref().unwrap().contains("Deezer"));
    // Nothing matched at all: an error, no empty playlist.
    let _ = s.update(Message::SubmitNewPlaylist);
    let gen = s.new_playlist_gen;
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "Deezer".into(),
        result: Ok(LinkImport {
            title: "X".into(),
            tracks: Vec::new(),
            swapped: 0,
            missing: vec!["A - B".into()],
            limit: None,
            source_rows: 0,
            repeats: 0,
        }),
    });
    assert_eq!(
        s.new_playlist.as_ref().unwrap().error.as_deref(),
        Some(s.language.new_playlist_none_found().as_str())
    );
    assert!(s.user_playlists.is_empty());
}

#[cfg(feature = "innertube")]
#[test]
fn import_summary_numbers_add_up_to_the_source() {
    let mut s = test_state();
    let gen = start_import(
        &mut s,
        "https://music.apple.com/co/playlist/salsita/pl.u-r2yBDVETePo6xWV",
    );
    // 10 source rows: 3 dropped as repeats before searching, 7 searched;
    // 5 found (two of them the same YouTube Music song), 2 not found.
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "Apple Music".into(),
        result: Ok(LinkImport {
            title: "Salsita".into(),
            tracks: vec![
                mk_track("a", "A", "X"),
                mk_track("b", "B", "X"),
                mk_track("c", "C", "X"),
                mk_track("d", "D", "X"),
                mk_track("a", "A", "X"),
            ],
            swapped: 0,
            missing: vec!["Y - E".into(), "Y - F".into()],
            limit: None,
            source_rows: 10,
            repeats: 3,
        }),
    });
    let sum = s.new_playlist.as_ref().unwrap().summary.clone().unwrap();
    assert_eq!(sum.total, 10);
    assert_eq!(sum.imported, 4);
    assert_eq!(sum.repeats, 4);
    assert_eq!(sum.imported + sum.repeats + sum.missing.len(), sum.total);
    assert_eq!(s.user_playlists[0].tracks.len(), 4);
}

#[cfg(feature = "innertube")]
#[test]
fn repeats_alone_still_explain_the_count() {
    let mut s = test_state();
    let gen = start_import(&mut s, "https://www.deezer.com/en/playlist/3155776842");
    let _ = s.update(Message::PlaylistImportFetched {
        generation: gen,
        platform: "Deezer".into(),
        result: Ok(LinkImport {
            title: "Twice".into(),
            tracks: vec![mk_track("a", "A", "X")],
            swapped: 0,
            missing: Vec::new(),
            limit: None,
            source_rows: 2,
            repeats: 1,
        }),
    });
    let sum = s.new_playlist.as_ref().unwrap().summary.clone().unwrap();
    assert_eq!((sum.total, sum.imported, sum.repeats), (2, 1, 1));
}

/// Headless right click (press + release) at the simulator's pointer.
fn right_click() -> Vec<iced::Event> {
    use iced::mouse::{Button, Event as M};
    vec![
        iced::Event::Mouse(M::ButtonPressed(Button::Right)),
        iced::Event::Mouse(M::ButtonReleased(Button::Right)),
    ]
}

#[test]
fn right_click_menu_selects_and_cuts_input_text() {
    let mut s = test_state();
    let size = iced::Size::new(1280.0, 800.0);
    let _ = s.update(Message::WindowResized(size));
    s.search_query = "hello world".into();
    let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
    let input = sim
        .find(iced::advanced::widget::Id::new(SEARCH_INPUT_ID))
        .expect("search input on screen")
        .bounds();
    let at = iced::Point::new(input.x + 40.0, input.center_y());
    // Menu rows sit just below-right of the right-click point:
    // card top = point + 2, then 4px padding, 30px rows.
    let row = |i: f32| iced::Point::new(at.x + 40.0, at.y + 2.0 + 4.0 + 30.0 * i + 15.0);
    // Right click opens the menu; "Select all" (row 3) selects the text.
    sim.point_at(at);
    let _ = sim.simulate(right_click());
    sim.point_at(row(3.0));
    let _ = sim.simulate(iced_test::simulator::click());
    // Right click again keeps that selection; "Cut" (row 0) empties it.
    sim.point_at(at);
    let _ = sim.simulate(right_click());
    sim.point_at(row(0.0));
    let _ = sim.simulate(iced_test::simulator::click());
    let msgs: Vec<Message> = sim.into_messages().collect();
    assert!(
        msgs.iter()
            .any(|m| matches!(m, Message::HomeSearchChanged(v) if v.is_empty())),
        "cut never edited the field: {msgs:?}"
    );
}

#[test]
fn right_click_menu_rows_are_disabled_without_a_selection() {
    let mut s = test_state();
    let size = iced::Size::new(1280.0, 800.0);
    let _ = s.update(Message::WindowResized(size));
    s.search_query = "hello".into();
    let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
    let input = sim
        .find(iced::advanced::widget::Id::new(SEARCH_INPUT_ID))
        .unwrap()
        .bounds();
    let at = iced::Point::new(input.x + 40.0, input.center_y());
    sim.point_at(at);
    let _ = sim.simulate(right_click());
    // Cut with nothing selected does nothing.
    sim.point_at(iced::Point::new(at.x + 40.0, at.y + 2.0 + 4.0 + 15.0));
    let _ = sim.simulate(iced_test::simulator::click());
    let msgs: Vec<Message> = sim.into_messages().collect();
    assert!(
        !msgs
            .iter()
            .any(|m| matches!(m, Message::HomeSearchChanged(_))),
        "{msgs:?}"
    );
}

#[test]
fn playlist_menu_gets_its_own_line_only_in_narrow_windows() {
    for (w, own) in [
        (360.0, true),
        (620.0, true),
        (799.0, true),
        (800.0, false),
        (1280.0, false),
    ] {
        let mut s = test_state();
        let _ = s.update(Message::WindowResized(iced::Size::new(w, 800.0)));
        settle(&mut s);
        assert_eq!(s.pl_menu_own_line(), own, "w={w}");
    }
}

#[test]
fn playlist_export_follows_the_on_screen_order() {
    let mut s = test_state();
    let _ = s.update(Message::CreatePlaylist("Mix".into()));
    let pid = s.user_playlists[0].id.clone();
    s.user_playlists[0].tracks = vec![mk_track("b", "Bravo", "Zed"), mk_track("a", "Alpha", "Amy")];
    // Manual order: as stored.
    let (title, csv) = s.playlist_export_csv(&pid).unwrap();
    assert_eq!(title, "Mix");
    let back = yutu_core::playlist_sources::parse_csv(&csv, "x").unwrap();
    let titles: Vec<&str> = back.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Bravo", "Alpha"]);
    assert_eq!(back.title, "Mix");
    // A title sort reorders the export the same way the page does.
    s.playlist_sorts.insert(pid.clone(), PlaylistSort::Title);
    let (_, csv) = s.playlist_export_csv(&pid).unwrap();
    let back = yutu_core::playlist_sources::parse_csv(&csv, "x").unwrap();
    let titles: Vec<&str> = back.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Alpha", "Bravo"]);
    assert!(s.playlist_export_csv("u404").is_none());
}

#[test]
fn playlist_export_results_report_honestly() {
    for lang in [Language::English, Language::Spanish] {
        let mut s = test_state();
        s.language = lang;
        s.status = "quiet".into();
        // Cancelled save dialog: nothing to say.
        let _ = s.update(Message::PlaylistExported {
            title: "Mix".into(),
            result: Ok(None),
        });
        assert_eq!(s.status, "quiet");
        let _ = s.update(Message::PlaylistExported {
            title: "Mix".into(),
            result: Ok(Some("/tmp/Mix.csv".into())),
        });
        assert!(s.status.contains("/tmp/Mix.csv"));
        assert!(!s.language.is_failure_line(&s.status));
        // Write errors surface as failures (visible in release builds).
        let _ = s.update(Message::PlaylistExported {
            title: "Mix".into(),
            result: Err("permission denied".into()),
        });
        assert!(
            s.language.is_failure_line(&s.status),
            "{lang:?}: {}",
            s.status
        );
    }
}

#[test]
fn favorites_sort_like_any_playlist() {
    let mut s = test_state();
    let fav = AppState::FAVORITES_ID.to_string();
    for (id, title, artist) in [
        ("b", "Bravo", "Zed"),
        ("a", "Alpha", "Amy"),
        ("c", "Charlie", "Moe"),
    ] {
        s.catalog.insert(id.into(), mk_track(id, title, artist));
        s.like_id(id);
    }
    let titles = |s: &AppState| -> Vec<String> {
        let p = s.find_playlist(AppState::FAVORITES_ID).unwrap();
        s.playlist_ordered_tracks(&p)
            .into_iter()
            .map(|t| t.title)
            .collect()
    };
    // Default: like order (oldest first), as before.
    assert_eq!(s.playlist_sort_cached(&fav), PlaylistSort::OldestFirst);
    assert_eq!(titles(&s), ["Bravo", "Alpha", "Charlie"]);
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::NewestFirst,
    });
    assert_eq!(titles(&s), ["Charlie", "Alpha", "Bravo"]);
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::Title,
    });
    assert_eq!(titles(&s), ["Alpha", "Bravo", "Charlie"]);
    // Picks Favorites does not offer fall back to its default.
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::LikedFirst,
    });
    assert_eq!(s.playlist_sort_cached(&fav), PlaylistSort::OldestFirst);
    // The pick survives a reload of the cache (persisted pref).
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::Artist,
    });
    s.playlist_sorts.clear();
    assert_eq!(s.playlist_sort(&fav), PlaylistSort::Artist);
    // Export follows the on-screen order.
    let (_, csv) = s.playlist_export_csv(&fav).unwrap();
    let back = yutu_core::playlist_sources::parse_csv(&csv, "x").unwrap();
    let artists: Vec<&str> = back.tracks.iter().map(|t| t.artist.as_str()).collect();
    assert_eq!(artists, ["Amy", "Moe", "Zed"]);
}

#[test]
fn favorites_sort_picker_lists_only_its_picks() {
    assert!(PlaylistSort::FAVORITES.contains(&PlaylistSort::Manual));
    assert!(!PlaylistSort::FAVORITES.contains(&PlaylistSort::LikedFirst));
    assert_eq!(
        PlaylistSort::LikedFirst.for_favorites(),
        PlaylistSort::OldestFirst
    );
    assert_eq!(PlaylistSort::Album.for_favorites(), PlaylistSort::Album);
}

#[test]
fn favorites_manual_order_is_dragged_and_kept() {
    let mut s = test_state();
    let fav = AppState::FAVORITES_ID.to_string();
    for id in ["f0", "f1", "f2"] {
        s.catalog.insert(id.into(), mk_track(id, id, "X"));
        s.like_id(id);
    }
    let order = |s: &AppState| -> Vec<String> {
        let p = s.find_playlist(AppState::FAVORITES_ID).unwrap();
        s.playlist_ordered_tracks(&p)
            .into_iter()
            .map(|t| t.id)
            .collect()
    };
    let _ = s.update(Message::OpenPlaylist(fav.clone()));
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::Manual,
    });
    // No custom order yet: like order.
    assert_eq!(order(&s), ["f0", "f1", "f2"]);
    // Drag the first song to the end.
    let _ = s.update(Message::PlDragStart {
        playlist_id: fav.clone(),
        index: 0,
    });
    let _ = s.update(Message::PlDragSlot(3));
    let _ = s.update(Message::DragEnd);
    assert!(s.status.contains("reordered"));
    assert_eq!(order(&s), ["f1", "f2", "f0"]);
    // Likes themselves are untouched (Oldest first still like order).
    assert_eq!(s.liked_order, ["f0", "f1", "f2"]);
    // A new like lands after the custom order; an unlike drops out.
    s.catalog.insert("f3".into(), mk_track("f3", "f3", "X"));
    s.like_id("f3");
    s.unlike_id("f2");
    assert_eq!(order(&s), ["f1", "f0", "f3"]);
    // The order survives a restart (pref reload).
    let (s2, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(s2.favorites_order, ["f1", "f2", "f0"]);
    // Other sorts ignore it.
    let _ = s.update(Message::PlaylistSortSelected {
        playlist_id: fav.clone(),
        sort: PlaylistSort::OldestFirst,
    });
    assert_eq!(order(&s), ["f0", "f1", "f3"]);
}

#[test]
fn favorites_drag_needs_manual() {
    let mut s = test_state();
    let fav = AppState::FAVORITES_ID.to_string();
    for id in ["f0", "f1"] {
        s.catalog.insert(id.into(), mk_track(id, id, "X"));
        s.like_id(id);
    }
    // Default sort (Oldest first, never picked): a drag never commits.
    let _ = s.update(Message::OpenPlaylist(fav.clone()));
    let _ = s.update(Message::PlDragStart {
        playlist_id: fav.clone(),
        index: 0,
    });
    let _ = s.update(Message::PlDragSlot(2));
    let _ = s.update(Message::DragEnd);
    assert!(s.favorites_order.is_empty());
    assert!(s.pl_drag.is_none());
}

#[test]
fn mini_ticker_window_centers_on_the_current_verse() {
    use crate::mini_player::ticker_window;
    // Middle: two above, two below.
    assert_eq!(
        ticker_window(10, Some(5)),
        vec![(3, -2), (4, -1), (5, 0), (6, 1), (7, 2)]
    );
    // Edges clamp instead of padding with phantom lines.
    assert_eq!(ticker_window(10, Some(0)), vec![(0, 0), (1, 1), (2, 2)]);
    assert_eq!(ticker_window(10, Some(9)), vec![(7, -2), (8, -1), (9, 0)]);
    // Before the first timestamp the opening lines are upcoming.
    assert_eq!(ticker_window(10, None), vec![(0, 1), (1, 2), (2, 3)]);
    assert_eq!(ticker_window(1, None), vec![(0, 1)]);
    // No lines, or a stale cursor past the end: nothing to show.
    assert!(ticker_window(0, None).is_empty());
    assert!(ticker_window(0, Some(0)).is_empty());
    assert_eq!(ticker_window(2, Some(7)), vec![(0, 1), (1, 2)]);
}

#[test]
fn mini_window_size_follows_the_ticker_and_the_ui_scale() {
    use crate::mini_player::*;
    let collapsed = mini_window_size(false, 1.0);
    let open = mini_window_size(true, 1.0);
    assert_eq!(collapsed.width, MINI_W + 2.0 * MINI_MARGIN);
    assert_eq!(open.width, collapsed.width);
    // The lyrics grow the window downward by exactly the ticker.
    assert_eq!(open.height - collapsed.height, MINI_LYRICS_H);
    // Scaled UI scales the window, so the content never clips.
    let big = mini_window_size(false, 1.5);
    assert_eq!(big.width, (collapsed.width * 1.5).ceil());
    assert_eq!(big.height, (collapsed.height * 1.5).ceil());
}

#[test]
fn mini_player_opens_once_toggles_lyrics_and_closes() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "T", "A"));
    assert!(s.mini.is_none());
    let _ = s.update(Message::OpenMiniPlayer);
    let mini = s.mini.expect("mini player open");
    assert!(!mini.lyrics && !mini.hovered);
    // A second open focuses the same window instead of stacking one.
    let _ = s.update(Message::OpenMiniPlayer);
    assert_eq!(s.mini.unwrap().id, mini.id);
    // Hover shows the chips; leaving hides them.
    let _ = s.update(Message::MiniHover(true));
    assert!(s.mini.unwrap().hovered);
    let _ = s.update(Message::MiniHover(false));
    assert!(!s.mini.unwrap().hovered);
    // Lyrics toggle flips and is remembered for the next open.
    let _ = s.update(Message::MiniToggleLyrics);
    assert!(s.mini.unwrap().lyrics);
    assert!(s.mini_lyrics_pref);
    let _ = s.update(Message::MiniClose);
    assert!(s.mini.is_none());
    let _ = s.update(Message::OpenMiniPlayer);
    assert!(s.mini.unwrap().lyrics, "reopens with the ticker open");
    // The pref survives a restart (same library).
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert!(again.mini_lyrics_pref);
    // Restore closes the mini window too; drags with none open no-op.
    let _ = s.update(Message::MiniRestore);
    assert!(s.mini.is_none());
    let _ = s.update(Message::MiniDrag);
    let _ = s.update(Message::MiniToggleLyrics);
    assert!(s.mini.is_none());
}

#[test]
fn mini_window_closed_from_outside_is_forgotten() {
    let mut s = test_state();
    let main = iced::window::Id::unique();
    s.main_window = Some(main);
    let _ = s.update(Message::OpenMiniPlayer);
    let id = s.mini.unwrap().id;
    // Another window id closing leaves the mini player alone.
    let _ = s.update(Message::WindowClosed(iced::window::Id::unique()));
    assert!(s.mini.is_some());
    let _ = s.update(Message::WindowClosed(id));
    assert!(s.mini.is_none());
    // The bar button opens a fresh one afterwards.
    let _ = s.update(Message::OpenMiniPlayer);
    assert!(s.mini.is_some_and(|m| m.id != id));
}

#[test]
fn mini_player_renders_every_lyrics_state() {
    // Synced: before, on, and past the verses (collapsed and open).
    let mut s = synced_state(6.0);
    let _ = s.update(Message::OpenMiniPlayer);
    let id = s.mini.unwrap().id;
    let _ = s.view_for(id);
    let _ = s.update(Message::MiniHover(true));
    let _ = s.update(Message::MiniToggleLyrics);
    for pos in [0.0, 6.0, 60.0] {
        s.position_secs = pos;
        let _ = s.view_for(id);
    }
    // Instrumental, plain, loading and missing lyrics.
    let mut entry = s.lyrics_cache[&("m1".to_string(), LyricsProvider::Lrclib)].clone();
    entry.instrumental = true;
    s.lyrics_cache
        .insert(("m1".into(), LyricsProvider::Lrclib), entry.clone());
    let _ = s.view_for(id);
    entry.instrumental = false;
    entry.synced.clear();
    entry.plain = "one\n\ntwo\nthree".into();
    s.lyrics_cache
        .insert(("m1".into(), LyricsProvider::Lrclib), entry);
    let _ = s.view_for(id);
    s.lyrics_cache.clear();
    let _ = s.view_for(id);
    // Empty queue still renders (no song, no ring progress).
    s.queue = Queue::default();
    s.paused = true;
    let _ = s.view_for(id);
    // Any other window id is the app itself.
    let _ = s.view_for(iced::window::Id::unique());
}

#[test]
fn mini_window_is_transparent_and_named() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "Song", "Band"));
    let _ = s.update(Message::OpenMiniPlayer);
    let id = s.mini.unwrap().id;
    let other = iced::window::Id::unique();
    // Only the capsule paints: the window background is clear.
    assert_eq!(
        s.theme_for(id).palette().background,
        iced::Color::TRANSPARENT
    );
    assert_ne!(
        s.theme_for(other).palette().background,
        iced::Color::TRANSPARENT
    );
    assert_eq!(s.title_for(other), "Song · Band");
    assert_eq!(s.title_for(id), "Mini player · Song · Band");
    s.language = Language::Spanish;
    assert_eq!(s.title_for(id), "Minirreproductor · Song · Band");
}

#[test]
fn mini_drag_keeps_the_chips_until_a_real_exit() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "T", "A"));
    let _ = s.update(Message::OpenMiniPlayer);
    let _ = s.update(Message::MiniHover(true));
    let _ = s.update(Message::MiniDrag);
    let mini = s.mini.unwrap();
    assert!(mini.hovered && mini.drag_hold);
    // The compositor's move grab sends a leave: the chips stay.
    let _ = s.update(Message::MiniHover(false));
    assert!(s.mini.unwrap().hovered, "chips survive the grab leave");
    // The pointer seen over the window again ends the hold, so the
    // next leave is a real one.
    let _ = s.update(Message::MiniHover(true));
    assert!(!s.mini.unwrap().drag_hold);
    let _ = s.update(Message::MiniHover(false));
    assert!(!s.mini.unwrap().hovered);
    // A drag that starts without a hover still shows the chips.
    let _ = s.update(Message::MiniDrag);
    assert!(s.mini.unwrap().hovered);
}

#[test]
fn mini_ticker_long_lines_stay_inside_their_slots() {
    use crate::mini_player::{
        ticker_lines, ticker_slot, ticker_text_width, ticker_worst_height, MINI_LYRICS_H,
    };
    // Budget: every slot at its maximum still fits inside the ticker.
    assert!(ticker_worst_height() <= MINI_LYRICS_H - 2.0 * 16.0);
    assert_eq!(ticker_slot(0, false).1, 1);
    assert_eq!(ticker_slot(0, true).1, 2);
    assert!(ticker_slot(0, true).0 < ticker_slot(0, false).0);
    for d in [-2, -1, 1, 2] {
        assert_eq!(ticker_slot(d, false).1, 1, "neighbors are one line");
    }
    let long = "and every word we never said is drifting down the river \
                past the porch light and the lanterns and the morning";
    let visible = vec![long, long, long, "short", long];
    // Current verse long: smaller, at most two lines, cut with "…".
    let lines = ticker_lines(&visible, Some(2));
    assert_eq!(lines.len(), 5);
    let w = ticker_text_width();
    for (cut, size, d) in &lines {
        let (_, max) = ticker_slot(*d, *d == 0);
        if cut == "short" {
            continue;
        }
        assert!(cut.ends_with('…'), "{d}: {cut}");
        assert!(cut.chars().count() < long.chars().count());
        if *d != 0 {
            assert_eq!(fit_lines(cut, w, *size, max), *cut, "{d} fits one line");
        }
    }
    assert_eq!(lines[2].1, ticker_slot(0, true).0);
    // A short current verse keeps the full size, untouched.
    let lines = ticker_lines(&visible, Some(3));
    let cur = lines.iter().find(|l| l.2 == 0).unwrap();
    assert_eq!((cur.0.as_str(), cur.1), ("short", ticker_slot(0, false).0));
    // Rendering long lines never panics (synced and plain).
    let mut s = synced_state(6.0);
    let mut entry = s.lyrics_cache[&("m1".to_string(), LyricsProvider::Lrclib)].clone();
    for l in entry.synced.iter_mut() {
        if !l.text.is_empty() {
            l.text = long.into();
        }
    }
    s.lyrics_cache
        .insert(("m1".into(), LyricsProvider::Lrclib), entry.clone());
    let _ = s.update(Message::OpenMiniPlayer);
    let id = s.mini.unwrap().id;
    if !s.mini.unwrap().lyrics {
        let _ = s.update(Message::MiniToggleLyrics);
    }
    for pos in [0.0, 6.0, 60.0] {
        s.position_secs = pos;
        let _ = s.view_for(id);
    }
    entry.synced.clear();
    entry.plain = format!("{long}\n{long}");
    s.lyrics_cache
        .insert(("m1".into(), LyricsProvider::Lrclib), entry);
    let _ = s.view_for(id);
}

#[test]
fn mini_position_is_remembered_across_opens_and_restarts() {
    use crate::mini_player::{format_mini_pos, mini_window_settings, parse_mini_pos};
    assert_eq!(
        parse_mini_pos(&format_mini_pos(iced::Point::new(12.4, -30.6))),
        Some(iced::Point::new(12.0, -31.0))
    );
    assert_eq!(parse_mini_pos("nope"), None);
    assert_eq!(parse_mini_pos("1,NaN"), None);
    // No saved spot: the first placement (top-right); saved: exactly it.
    assert!(matches!(
        mini_window_settings(false, 1.0, None).position,
        iced::window::Position::SpecificWith(_)
    ));
    assert!(matches!(
        mini_window_settings(false, 1.0, Some(iced::Point::new(5.0, 6.0))).position,
        iced::window::Position::Specific(p) if p == iced::Point::new(5.0, 6.0)
    ));
    let mut s = test_state();
    let _ = s.update(Message::OpenMiniPlayer);
    let _ = s.update(Message::MiniMoved(iced::Point::new(700.0, 40.0)));
    let _ = s.update(Message::MiniMoved(iced::Point::new(800.0, 50.0)));
    assert_eq!(s.mini_pos, Some(iced::Point::new(800.0, 50.0)));
    let _ = s.update(Message::MiniClose);
    // Saved on close, back after a restart.
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(again.mini_pos, Some(iced::Point::new(800.0, 50.0)));
    // A position read back after close (Hyprland) is saved at once.
    let _ = s.update(Message::MiniMoved(iced::Point::new(90.0, 60.0)));
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(again.mini_pos, Some(iced::Point::new(90.0, 60.0)));
    // Quitting (main window closed) with the mini open keeps its spot.
    let main = iced::window::Id::unique();
    s.main_window = Some(main);
    let _ = s.update(Message::OpenMiniPlayer);
    let _ = s.update(Message::MiniMoved(iced::Point::new(300.0, 20.0)));
    let _ = s.update(Message::WindowClosed(main));
    assert!(s.mini.is_none());
    let (again, _) = AppState::with_library_path(s.library_path.clone());
    assert_eq!(again.mini_pos, Some(iced::Point::new(300.0, 20.0)));
}

#[cfg(target_os = "linux")]
#[test]
fn mini_window_has_its_own_app_id() {
    let mini = crate::mini_player::mini_window_settings(false, 1.0, None);
    assert_eq!(mini.platform_specific.application_id, "yutu-mux-mini");
    assert_eq!(
        window_settings().platform_specific.application_id,
        "yutu-mux"
    );
}

#[test]
fn player_bar_shows_the_mini_player_button_in_every_layout() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "T", "A"));
    for w in [400.0, 700.0, 1280.0] {
        let _ = s.update(Message::WindowResized(iced::Size::new(w, 800.0)));
        let _ = s.player_view();
        let _ = s.update(Message::OpenMiniPlayer);
        let _ = s.player_view();
        let _ = s.update(Message::MiniClose);
    }
}

/// Headless mini player screenshots (collapsed, hovered, lyrics open)
/// with a fake cover glow, written next to the layout snapshots. The
/// window background is transparent, so the PNGs keep alpha. Run with
/// `cargo test -p yutu-ui --lib mini_player_snapshots -- --ignored`.
#[test]
#[ignore]
fn mini_player_snapshots() {
    let dir = std::env::var("YUTU_SNAPSHOT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/layout-snapshots")
        });
    std::fs::create_dir_all(&dir).unwrap();
    for (name, hovered, lyrics) in [
        ("mini-collapsed", false, false),
        ("mini-hovered", true, false),
        ("mini-lyrics", true, true),
        ("mini-long-lyrics", false, true),
        ("mini-plain-lyrics", false, true),
    ] {
        let mut s = synced_state(6.0);
        let mut t = mk_track("pip1", "Paper Lanterns", "Mara Vell");
        t.thumbnail_url = Some("https://example.invalid/m1.jpg".into());
        s.queue = Queue::default();
        s.queue.push(t.clone());
        let url = s.hero_art_for(&t).unwrap();
        fake_backdrop(&mut s, url);
        let verses = if name == "mini-long-lyrics" || name == "mini-plain-lyrics" {
            [
                "We left the porch light burning for the ones who never came back home",
                "for the ones who find their way across the water in the dark of night",
                "Paper lanterns on the water, every one a little wish we could not keep",
                "carry everything we could not say before the river took the words away",
                "Hold the river, hold it steady, hold it till the morning pulls us in",
                "till the morning pulls us in",
            ]
        } else {
            [
                "We left the porch light burning",
                "for the ones who find their way",
                "Paper lanterns on the water",
                "carry everything we could not say",
                "Hold the river, hold it steady",
                "till the morning pulls us in",
            ]
        };
        s.lyrics_cache.insert(
            ("pip1".into(), LyricsProvider::Lrclib),
            yutu_core::LyricsResult {
                plain: String::new(),
                synced: verses
                    .iter()
                    .enumerate()
                    .map(|(i, v)| yutu_core::LyricLine {
                        start_ms: 50_000 + i as u64 * 8_000,
                        text: (*v).into(),
                    })
                    .collect(),
                instrumental: false,
            },
        );
        // Unsynced: the whole text, wrapped, scrollable.
        if name == "mini-plain-lyrics" {
            let plain = verses.join("\n") + "\n\n" + &verses.join("\n");
            s.lyrics_cache.insert(
                ("pip1".into(), LyricsProvider::Lrclib),
                yutu_core::LyricsResult {
                    plain,
                    synced: Vec::new(),
                    instrumental: false,
                },
            );
        }
        // A third of the way in, on the third verse.
        s.position_secs = 67.0;
        let _ = s.update(Message::OpenMiniPlayer);
        let id = s.mini.unwrap().id;
        if hovered {
            let _ = s.update(Message::MiniHover(true));
        }
        if lyrics != s.mini.unwrap().lyrics {
            let _ = s.update(Message::MiniToggleLyrics);
        }
        let settings = iced::Settings {
            fonts: vec![include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf")
                .as_slice()
                .into()],
            default_font: iced::Font::with_name("Adwaita Sans"),
            ..iced::Settings::default()
        };
        let theme = s.theme_for(id);
        let size = crate::mini_player::mini_content_size(lyrics);
        let mut sim = iced_test::Simulator::with_size(settings, size, s.view_for(id));
        let snap = sim.snapshot(&theme).unwrap();
        for f in std::fs::read_dir(&dir).unwrap().flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            if n.starts_with(&format!("{name}-")) || n == format!("{name}.png") {
                let _ = std::fs::remove_file(f.path());
            }
        }
        snap.matches_image(dir.join(format!("{name}.png"))).unwrap();
    }
}

/// Send one control command through `update`, return its JSON reply.
fn control_call(s: &mut AppState, words: &str) -> Result<serde_json::Value, String> {
    let args: Vec<String> = words.split_whitespace().map(String::from).collect();
    let command = yutu_core::control::parse_command(&args)?;
    let (tx, mut rx) = futures::channel::oneshot::channel();
    let _ = s.update(Message::Control(ControlCall {
        command,
        reply: ReplySlot::new(tx),
    }));
    match rx.try_recv() {
        Ok(Some(ControlReply::Json(r))) => r,
        Ok(Some(ControlReply::Screenshot { .. })) => Err("unexpected screenshot".into()),
        _ => Err("no reply".into()),
    }
}

#[test]
fn control_state_and_navigation_go_through_messages() {
    let mut s = test_state();
    let st = control_call(&mut s, "state").unwrap();
    assert_eq!(st["route"], "home");
    assert_eq!(st["busy"], false);
    let r = control_call(&mut s, "nav settings").unwrap();
    assert_eq!(r["route"], "settings");
    assert_eq!(s.route, Route::Settings);
    control_call(&mut s, "nav back").unwrap();
    assert_eq!(s.route, Route::Home);
    assert!(control_call(&mut s, "nav nowhere").is_err());
}

#[test]
fn control_queue_commands_respect_contracts() {
    let mut s = test_state();
    for i in 0..3 {
        s.queue
            .push(mk_track(&format!("t{i}"), &format!("Song {i}"), "A"));
    }
    s.queue.jump_to(0);
    let q = control_call(&mut s, "queue").unwrap();
    assert_eq!(q["items"].as_array().unwrap().len(), 3);
    assert_eq!(q["index"], 0);
    // Out-of-range jumps are refused, not clamped.
    assert!(control_call(&mut s, "jump 9").is_err());
    // Set-style shuffle/repeat land on the requested value.
    control_call(&mut s, "shuffle on").unwrap();
    assert!(s.queue.shuffle);
    control_call(&mut s, "shuffle on").unwrap();
    assert!(s.queue.shuffle, "set-on twice must not toggle back");
    control_call(&mut s, "repeat one").unwrap();
    assert_eq!(s.queue.repeat, yutu_player::RepeatMode::One);
    control_call(&mut s, "repeat off").unwrap();
    assert_eq!(s.queue.repeat, yutu_player::RepeatMode::Off);
    control_call(&mut s, "volume 0.25").unwrap();
    assert!((s.volume - 0.25).abs() < 1e-6);
}

#[test]
fn control_play_result_needs_a_result() {
    let mut s = test_state();
    assert!(control_call(&mut s, "play-result 0").is_err());
    let p = control_call(&mut s, "playlists").unwrap();
    assert_eq!(p[0]["id"], AppState::FAVORITES_ID);
}

#[test]
fn control_mini_commands_drive_the_mini_player() {
    let mut s = test_state();
    s.queue.push(mk_track("m1", "T", "A"));
    assert_eq!(
        control_call(&mut s, "state").unwrap()["mini"]["open"],
        false
    );
    assert!(control_call(&mut s, "mini lyrics on").is_err(), "closed");
    assert!(control_call(&mut s, "mini screenshot").is_err(), "closed");
    control_call(&mut s, "mini").unwrap();
    assert!(s.mini.is_some());
    control_call(&mut s, "mini lyrics on").unwrap();
    assert!(s.mini.unwrap().lyrics);
    // Already on: no toggle back off.
    control_call(&mut s, "mini lyrics on").unwrap();
    assert!(s.mini.unwrap().lyrics);
    let st = control_call(&mut s, "state").unwrap();
    assert_eq!(st["mini"]["open"], true);
    assert_eq!(st["mini"]["lyrics"], true);
    control_call(&mut s, "mini").unwrap();
    assert!(s.mini.is_none());
    control_call(&mut s, "mini open").unwrap();
    control_call(&mut s, "mini restore").unwrap();
    assert!(s.mini.is_none());
}
