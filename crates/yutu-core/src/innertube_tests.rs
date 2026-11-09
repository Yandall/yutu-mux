//! innertube_tests: moved from innertube.rs (no behavior change).

#[allow(unused_imports)]
use super::innertube::*;
#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use serde_json::Value;
#[allow(unused_imports)]
use std::path::Path;
#[allow(unused_imports)]
use ytmapi_rs::common::YoutubeID;
#[allow(unused_imports)]
use ytmapi_rs::parse::{PlaylistItem, SearchResultAlbum, TableListSong};

#[test]
fn web_context_matches_music_website_shape() {
    // Rolling version tracks today (never a stale pin), and the
    // context carries locale with the user as a sibling, exactly
    // like music.youtube.com sends it.
    let version = web_remix_version();
    assert!(version.starts_with("1.20"), "{version}");
    assert!(version.ends_with(".01.00"), "{version}");
    let ctx = web_context();
    assert_eq!(
        ctx.pointer("/client/clientName").and_then(|v| v.as_str()),
        Some("WEB_REMIX")
    );
    assert_eq!(
        ctx.pointer("/client/hl").and_then(|v| v.as_str()),
        Some("en")
    );
    assert_eq!(
        ctx.pointer("/client/gl").and_then(|v| v.as_str()),
        Some("US")
    );
    assert_eq!(
        ctx.pointer("/client/clientVersion")
            .and_then(|v| v.as_str()),
        Some(version.as_str())
    );
    assert!(ctx.pointer("/client/user").is_none());
    assert_eq!(
        ctx.pointer("/user/lockedSafetyMode")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[test]
fn play_count_filter_keeps_counts_drops_dates() {
    for raw in [
        "213M plays",
        "1B plays",
        "120K views",
        "55K views",
        "1 play",
        "2725 M reproducciones",
        "1,2 M reproduções",
    ] {
        assert!(is_play_count(raw), "{raw}");
    }
    // Upload dates ride the same column (podcast episodes): they
    // are not counts and must never render as one.
    for raw in ["Nov 12, 2025", "2025", "", "  "] {
        assert!(!is_play_count(raw), "{raw}");
    }
}

#[test]
fn artist_songs_fixture_keeps_order_counts_and_artists() {
    let tracks = parse_artist_songs_fixture(include_str!("../fixtures/artist_songs.json"));
    assert_eq!(tracks.len(), 2);
    // Server rank order survives (Popular is this order, not
    // alphabetical); counts survive; artists join like search.
    assert_eq!(tracks[0].title, "Bicycle Race");
    assert_eq!(tracks[0].artist, "Queen");
    assert_eq!(tracks[0].plays.as_deref(), Some("76M plays"));
    assert_eq!(tracks[0].album.as_deref(), Some("The Game"));
    // No duration or thumbs upstream: rows render the bare count
    // with a placeholder cover.
    assert_eq!(tracks[0].duration_secs, 0);
    assert!(tracks[0].thumbnail_url.is_none());
    // The date riding the plays column is dropped (not a count).
    assert_eq!(tracks[1].plays, None);
}

#[test]
fn album_songs_fixture_keeps_track_order_counts_and_context() {
    let tracks = parse_album_songs_fixture(
        include_str!("../fixtures/album_songs.json"),
        "Queen",
        "Test Album",
    );
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0].title, "First Song");
    assert_eq!(tracks[0].artist, "Queen");
    assert_eq!(tracks[0].album.as_deref(), Some("Test Album"));
    assert_eq!(tracks[0].duration_secs, 179);
    assert_eq!(tracks[0].plays.as_deref(), Some("100M plays"));
    assert_eq!(tracks[1].plays.as_deref(), Some("50M plays"));
}

#[test]
fn po_token_mapping_covers_403_and_botguard() {
    for msg in [
        "Http error code 403 recieved in response. Message: <Forbidden>.",
        "web error <403> received",
        "PO token required for playback",
        "botguard challenge missing",
    ] {
        assert_eq!(
            classify_player_error(msg, StreamClient::WebMusic, "vid"),
            StreamError::PoTokenRequired {
                client: "web_music"
            }
        );
    }
}

#[test]
fn unplayable_maps_to_not_found_and_other_to_network() {
    assert_eq!(
        classify_player_error("playability UNPLAYABLE", StreamClient::Ios, "vid"),
        StreamError::NotFound {
            video_id: "vid".into()
        }
    );
    assert_eq!(
        classify_player_error("connection reset", StreamClient::AndroidVr, "vid"),
        StreamError::Network("connection reset".into())
    );
}

#[test]
fn po_token_hint_is_actionable() {
    let err = classify_player_error("403 Forbidden", StreamClient::WebMusic, "vid");
    assert!(err.hint().contains("botguard") || err.hint().contains("android_vr"));
}

#[test]
fn extracts_first_audio_url_from_fixture() {
    let raw = include_str!("../fixtures/player_with_audio.json");
    let value: serde_json::Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        extract_audio_url(&value).as_deref(),
        Some("https://example.invalid/audio.webm")
    );
}

#[test]
fn ciphered_or_missing_streaming_data_yields_none() {
    let ciphered: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/player_ciphered.json")).unwrap();
    assert_eq!(extract_audio_url(&ciphered), None);
    let missing: serde_json::Value = serde_json::json!({"playabilityStatus": {"status": "OK"}});
    assert_eq!(extract_audio_url(&missing), None);
    let unplayable: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/player_unplayable.json")).unwrap();
    assert_eq!(extract_audio_url(&unplayable), None);
    assert_eq!(
        classify_no_url(&unplayable, StreamClient::WebMusic, "vid"),
        StreamError::NotFound {
            video_id: "vid".into()
        }
    );
}

#[test]
fn fallback_chain_records_every_client_before_exhaustion() {
    let mut req = StreamRequest::new("vid");
    let mut seen = Vec::new();
    while let Some(client) = req.next_client() {
        seen.push(client);
    }
    assert_eq!(
        seen,
        vec![
            StreamClient::AndroidVr,
            StreamClient::WebMusic,
            StreamClient::Ios
        ]
    );
    assert_eq!(req.tried.len(), 3);
}

#[test]
fn token_request_body_carries_pot_and_context() {
    let body = player_request_body("vid123", Some("TOKEN"));
    assert_eq!(body["videoId"], "vid123");
    assert_eq!(body["serviceIntegrityDimensions"]["poToken"], "TOKEN");
    assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
    let plain = player_request_body("vid123", None);
    assert!(plain.get("serviceIntegrityDimensions").is_none());
}

#[test]
fn signature_timestamp_is_plausible_days() {
    let days = signature_timestamp_days();
    assert!(days > 20_000 && days < 100_000);
}

#[test]
fn cookie_file_validation_rejects_empty_without_network() {
    let dir = std::env::temp_dir();
    let path = dir.join("yutu-innertube-empty-cookie.txt");
    std::fs::write(&path, "   ").unwrap();
    let err = CookieAuth::from_file(&path).unwrap_err();
    assert!(err.contains("empty"));
    assert!(!err.contains("   "));
}

// ---- Library / recommendations / lyrics mapping (offline) ----

#[test]
fn library_songs_fixture_parses_to_tracks() {
    let tracks = parse_library_songs_fixture(include_str!("../fixtures/library_songs.json"));
    assert_eq!(tracks.len(), 2);
    assert_eq!(
        tracks[0],
        Track {
            id: "isiofhN6_vc".into(),
            title: "A Day In The Life".into(),
            artist: "The Beatles".into(),
            album: Some("The Beatles 1967 - 1970 (Remastered 2009)".into()),
            duration_secs: 308,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
            is_video: false,
        }
    );
    // Multiple artists join the way the search API pre-joins them.
    assert_eq!(tracks[1].artist, "The Beatles, George Martin");
    assert_eq!(tracks[1].duration_secs, 181);
}

#[test]
fn library_songs_fixture_rejects_garbage() {
    assert!(parse_library_songs_fixture("not json").is_empty());
}

#[test]
fn watch_playlist_fixture_parses_to_tracks() {
    let tracks = parse_watch_playlist_fixture(include_str!("../fixtures/watch_playlist.json"));
    assert_eq!(tracks.len(), 2);
    assert_eq!(
        tracks[0],
        Track {
            id: "45cYwDMibGo".into(),
            title: "Come Together (Remastered 2009)".into(),
            artist: "The Beatles".into(),
            album: None,
            duration_secs: 260,
            thumbnail_url: None,
            thumbnail_large_url: None,
            plays: None,
            is_video: false,
        }
    );
    assert_eq!(tracks[1].duration_secs, 186);
}

#[test]
fn playlist_items_map_songs_videos_and_skip_episodes() {
    let song: PlaylistItem = serde_json::from_value(serde_json::json!({
        "Song": {
            "video_id": "vid1",
            "track_no": 1,
            "album": {"name": "Album One", "id": "MPRE1"},
            "duration": "2:30",
            "title": "Song One",
            "artists": [{"name": "Somebody"}],
            "like_status": "LIKE",
            "thumbnails": [],
            "explicit": "NotExplicit",
            "is_available": true,
            "playlist_id": "LM"
        }
    }))
    .unwrap();
    let video: PlaylistItem = serde_json::from_value(serde_json::json!({
        "Video": {
            "video_id": "vid2",
            "track_no": 2,
            "duration": "10:00",
            "title": "Video Two",
            "channel_name": "A Channel",
            "channel_id": "UCxyz",
            "like_status": "INDIFFERENT",
            "thumbnails": [],
            "is_available": true,
            "playlist_id": "LM"
        }
    }))
    .unwrap();
    let episode: PlaylistItem = serde_json::from_value(serde_json::json!({
        "Episode": {
            "episode_id": "ep1",
            "track_no": 3,
            "date": "Live",
            "duration": "Live",
            "title": "Episode Three",
            "podcast_name": "A Podcast",
            "podcast_id": "PLpod",
            "like_status": "INDIFFERENT",
            "thumbnails": [],
            "is_available": true
        }
    }))
    .unwrap();
    let tracks: Vec<Track> = [song, video, episode]
        .iter()
        .filter_map(playlist_item_to_track)
        .collect();
    assert_eq!(
        tracks.len(),
        2,
        "episodes are not music and must be skipped"
    );
    assert_eq!(tracks[0].artist, "Somebody");
    assert_eq!(tracks[0].album.as_deref(), Some("Album One"));
    assert_eq!(tracks[1].artist, "A Channel");
    assert_eq!(tracks[1].album, None);
}

/// Fabricated first-page JSON with the row shapes seen live: a
/// standard row (duration in `fixedColumns`), a flex-duration row
/// (no `fixedColumns`, duration in flex column 3), plus rows that
/// must be skipped (deleted, greyed-out, episode, continuation).
fn fabricated_playlist_page() -> String {
    // One flex column with a single text run.
    let col = |text: &str| {
        format!(
            r#"{{"musicResponsiveListItemFlexColumnRenderer": {{"text": {{"runs": [{{"text": "{text}"}}]}}}}}}"#
        )
    };
    // One row; empty `fixed`/`policy` strings omit those fields.
    let row = |cols: String, fixed: &str, video_id: &str, policy: &str, video_type: &str| {
        let fixed_json = if fixed.is_empty() {
            String::new()
        } else {
            format!(
                r#""fixedColumns": [{{"musicResponsiveListItemFixedColumnRenderer": {{"text": {{"simpleText": "{fixed}"}}}}}}],"#
            )
        };
        let policy_json = if policy.is_empty() {
            String::new()
        } else {
            format!(r#""musicItemRendererDisplayPolicy": "{policy}","#)
        };
        format!(
            r#"{{"musicResponsiveListItemRenderer": {{{fixed_json}{policy_json}"flexColumns": [{cols}],"overlay": {{"musicItemThumbnailOverlayRenderer": {{"content": {{"musicPlayButtonRenderer": {{"playNavigationEndpoint": {{"watchEndpoint": {{"videoId": "{video_id}", "videoType": "{video_type}"}}}}}}}}}}}},"thumbnail": {{"musicThumbnailRenderer": {{"thumbnail": {{"thumbnails": [{{"url": "https://example.invalid/s", "width": 60}}, {{"url": "https://example.invalid/l", "width": 544}}]}}}}}},"menu": {{"menuRenderer": {{"items": []}}}}}}}}"#
        )
    };
    let multi_artist = format!(
        "{},{}",
        col("Song A"),
        r#"{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Alice"}, {"text": ", "}, {"text": "Bob"}]}}}"#
    );
    let rows = [
        row(
            format!("{},{},{}", col("Song A"), col("x"), col("y")),
            "3:20",
            "vid11111111",
            "",
            "MUSIC_VIDEO_TYPE_ATV",
        ),
        row(
            multi_artist,
            "4:05",
            "vid22222222",
            "",
            "MUSIC_VIDEO_TYPE_ATV",
        ),
        row(
            format!(
                "{},{},{},{}",
                col("Flex Song"),
                col("Solo"),
                col("Some Album"),
                col("2:30")
            ),
            "",
            "vid33333333",
            "",
            "MUSIC_VIDEO_TYPE_ATV",
        ),
        row(
            col("Song deleted"),
            "1:00",
            "vid44444444",
            "",
            "MUSIC_VIDEO_TYPE_ATV",
        ),
        row(
            format!("{},{}", col("Grey Song"), col("No One")),
            "1:00",
            "vid55555555",
            "MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT",
            "MUSIC_VIDEO_TYPE_ATV",
        ),
        row(
            format!("{},{}", col("Episode One"), col("A Podcast")),
            "30:00",
            "vid66666666",
            "",
            "MUSIC_VIDEO_TYPE_PODCAST_EPISODE",
        ),
        r#"{"continuationItemRenderer": {}}"#.to_string(),
    ];
    format!(
        r#"{{"contents": {{"twoColumnBrowseResultsRenderer": {{"secondaryContents": {{"sectionListRenderer": {{"contents": [{{"musicPlaylistShelfRenderer": {{"contents": [{}]}}}}]}}}}}}}}}}"#,
        rows.join(",")
    )
}

#[test]
fn tolerant_rows_map_both_shapes_and_skip_junk() {
    let tracks = tolerant_playlist_rows(&fabricated_playlist_page());
    assert_eq!(
        tracks.len(),
        3,
        "standard + multi-artist + flex-duration: {tracks:?}"
    );
    assert_eq!(tracks[0].id, "vid11111111");
    assert_eq!(tracks[0].title, "Song A");
    assert_eq!(tracks[0].duration_secs, 200);
    assert_eq!(
        tracks[0].thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
    assert_eq!(
        tracks[0].thumbnail_large_url.as_deref(),
        Some("https://example.invalid/l")
    );
    assert_eq!(tracks[1].artist, "Alice, Bob");
    assert_eq!(tracks[1].duration_secs, 245);
    // Flex-duration row: no fixedColumns, duration in flex column 3.
    assert_eq!(tracks[2].id, "vid33333333");
    assert_eq!(tracks[2].title, "Flex Song");
    assert_eq!(tracks[2].artist, "Solo");
    assert_eq!(tracks[2].album.as_deref(), Some("Some Album"));
    assert_eq!(tracks[2].duration_secs, 150);
}

#[test]
fn tolerant_rows_parse_continuation_pages_and_reject_garbage() {
    let cont = serde_json::json!({
        "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
            {"musicResponsiveListItemRenderer": {
                "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Late Song"}]}}}, {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Late Artist"}]}}}],
                "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vid99999999", "videoType": "MUSIC_VIDEO_TYPE_ATV"}}}}}},
                "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                "menu": {"menuRenderer": {"items": []}},
            }},
        ]}}]
    });
    let tracks = tolerant_playlist_rows(&cont.to_string());
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id, "vid99999999");
    assert_eq!(tracks[0].duration_secs, 0);
    assert!(tolerant_playlist_rows("not json").is_empty());
    assert!(tolerant_playlist_rows("{}").is_empty());
}

#[test]
fn library_status_split_covers_like_dislike_indifferent() {
    use ytmapi_rs::common::LikeStatus;
    let mut songs: Vec<TableListSong> =
        serde_json::from_str(include_str!("../fixtures/library_songs.json")).unwrap();
    assert!(songs.len() >= 2);
    songs[0].like_status = LikeStatus::Liked;
    songs[1].like_status = LikeStatus::Disliked;
    let rows: Vec<(Track, LikeStatus)> = songs
        .iter()
        .map(|s| (table_song_to_track(s), s.like_status.clone()))
        .collect();
    let (liked, disliked) = split_library_status(&rows);
    assert_eq!(liked, vec![songs[0].video_id.get_raw().to_string()]);
    assert_eq!(disliked, vec![songs[1].video_id.get_raw().to_string()]);
    // Indifferent rows land in neither set.
    songs[1].like_status = LikeStatus::Indifferent;
    let rows: Vec<(Track, LikeStatus)> = songs
        .iter()
        .map(|s| (table_song_to_track(s), s.like_status.clone()))
        .collect();
    let (liked, disliked) = split_library_status(&rows);
    assert_eq!(liked.len(), 1);
    assert!(disliked.is_empty());
}

#[test]
fn leading_count_parses_details_totals() {
    assert_eq!(parse_leading_count("248 songs"), Some(248));
    assert_eq!(parse_leading_count("1,234 videos"), Some(1234));
    assert_eq!(parse_leading_count("  7"), Some(7));
    assert_eq!(parse_leading_count("songs"), None);
    assert_eq!(parse_leading_count(""), None);
}

// ---- Write-operation HTTP shapes (offline: query construction only) ----

#[test]
fn rate_song_queries_hit_like_endpoints_with_video_target() {
    use ytmapi_rs::common::PlaylistID;
    use ytmapi_rs::common::{LikeStatus, VideoID, YoutubeID as _};
    use ytmapi_rs::query::{PostQuery, RatePlaylistQuery, RateSongQuery};

    let like = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Liked);
    assert_eq!(like.path(), "like/like");
    assert_eq!(
        like.header()["target"],
        serde_json::json!({"videoId": "vid123"})
    );
    let dislike = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Disliked);
    assert_eq!(dislike.path(), "like/dislike");
    let clear = RateSongQuery::new(VideoID::from_raw("vid123"), LikeStatus::Indifferent);
    assert_eq!(clear.path(), "like/removelike");

    let pl_like = RatePlaylistQuery::new(PlaylistID::from_raw("PLabc"), LikeStatus::Liked);
    assert_eq!(pl_like.path(), "like/like");
    assert_eq!(
        pl_like.header()["target"],
        serde_json::json!({"playlistId": "PLabc"})
    );
    let pl_dislike = RatePlaylistQuery::new(PlaylistID::from_raw("PLabc"), LikeStatus::Disliked);
    assert_eq!(pl_dislike.path(), "like/dislike");
}

#[test]
fn playlist_write_queries_hit_expected_paths_and_ids() {
    use ytmapi_rs::common::{PlaylistID, SetVideoID, VideoID, YoutubeID as _};
    use ytmapi_rs::query::{playlist::PrivacyStatus, AddPlaylistItemsQuery};
    use ytmapi_rs::query::{
        CreatePlaylistQuery, DeletePlaylistQuery, EditPlaylistQuery, PostQuery,
        RemovePlaylistItemsQuery,
    };

    let create = CreatePlaylistQuery::new("Road Trip", None, PrivacyStatus::Private);
    assert_eq!(create.path(), "playlist/create");
    let header = create.header();
    assert_eq!(header["title"], "Road Trip");
    assert_eq!(header["privacyStatus"], "PRIVATE");

    let delete = DeletePlaylistQuery::new(PlaylistID::from_raw("PLabc123"));
    assert_eq!(delete.path(), "playlist/delete");
    assert_eq!(delete.header()["playlistId"], "PLabc123");

    let add = AddPlaylistItemsQuery::new_from_videos(
        PlaylistID::from_raw("PLabc123"),
        [VideoID::from_raw("vid999")],
        Default::default(),
    );
    assert_eq!(add.path(), "browse/edit_playlist");
    assert_eq!(add.header()["playlistId"], "PLabc123");
    let actions = add.header()["actions"].clone();
    let actions = actions.as_array().expect("actions array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["action"], "ACTION_ADD_VIDEO");
    assert_eq!(actions[0]["addedVideoId"], "vid999");

    let remove = RemovePlaylistItemsQuery::new(
        PlaylistID::from_raw("PLabc123"),
        [SetVideoID::from_raw("set123")],
    );
    assert_eq!(remove.path(), "browse/edit_playlist");
    assert_eq!(remove.header()["playlistId"], "PLabc123");
    let actions = remove.header()["actions"].clone();
    let actions = actions.as_array().expect("remove actions array");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["action"], "ACTION_REMOVE_VIDEO");
    assert_eq!(actions[0]["setVideoId"], "set123");

    let rename = EditPlaylistQuery::new_title(PlaylistID::from_raw("PLabc123"), "New Name");
    assert_eq!(rename.path(), "browse/edit_playlist");
    assert_eq!(rename.header()["playlistId"], "PLabc123");
    let actions = rename.header()["actions"].clone();
    let actions = actions.as_array().expect("rename actions array");
    assert!(
        actions
            .iter()
            .any(|a| a["action"] == "ACTION_SET_PLAYLIST_NAME" && a["playlistName"] == "New Name"),
        "rename must carry ACTION_SET_PLAYLIST_NAME, got {actions:?}"
    );
}

#[test]
fn subscribe_queries_hit_expected_paths() {
    use ytmapi_rs::common::{ArtistChannelID, YoutubeID as _};
    use ytmapi_rs::query::{PostQuery, SubscribeArtistQuery, UnsubscribeArtistsQuery};

    let sub = SubscribeArtistQuery::new(ArtistChannelID::from_raw("UCabc"));
    assert_eq!(sub.path(), "subscription/subscribe");

    let unsub = UnsubscribeArtistsQuery::new([ArtistChannelID::from_raw("UCabc")]);
    assert_eq!(unsub.path(), "subscription/unsubscribe");
}

#[test]
fn set_video_pairs_extract_from_both_page_shapes() {
    let page = serde_json::json!({
        "contents": {"twoColumnBrowseResultsRenderer": {"secondaryContents": {"sectionListRenderer": {"contents": [
            {"musicPlaylistShelfRenderer": {"contents": [
                {"musicResponsiveListItemRenderer": {
                    "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidAAA"}}}}}},
                    "playlistItemData": {"playlistSetVideoId": "setAAA", "videoId": "vidAAA"},
                }},
                {"musicResponsiveListItemRenderer": {
                    "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidBBB"}}}}}},
                    "menu": {"menuRenderer": {"items": [{"menuServiceItemRenderer": {"serviceEndpoint": {"playlistEditEndpoint": {"actions": [{"setVideoId": "setBBB"}]}}}}]}},
                }},
                {"musicResponsiveListItemRenderer": {
                    "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Song deleted"}]}}}],
                }},
            ]}}
        ]}}}}
    });
    let pairs = extract_set_video_pairs(&page.to_string());
    assert_eq!(
        pairs,
        vec![
            ("vidAAA".to_string(), "setAAA".to_string()),
            ("vidBBB".to_string(), "setBBB".to_string()),
        ]
    );
    let cont = serde_json::json!({
        "onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
            {"musicResponsiveListItemRenderer": {
                "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {"playNavigationEndpoint": {"watchEndpoint": {"videoId": "vidCCC"}}}}}},
                "playlistItemData": {"videoId": "vidCCC", "setVideoId": "setCCC"},
            }},
        ]}}]
    });
    assert_eq!(
        extract_set_video_pairs(&cont.to_string()),
        vec![("vidCCC".to_string(), "setCCC".to_string())]
    );
    assert!(extract_set_video_pairs("not json").is_empty());
    assert!(extract_set_video_pairs("{}").is_empty());
}

#[test]
fn playlist_ids_convert_between_browse_and_edit_forms() {
    use crate::library::{browse_playlist_id, canonical_playlist_id, edit_playlist_id};

    // Library browse returns VL-prefixed ids; storage is edit form.
    assert_eq!(canonical_playlist_id("VLPLabc123"), "PLabc123");
    assert_eq!(canonical_playlist_id("VLLM"), "LM");
    assert_eq!(canonical_playlist_id("PLabc123"), "PLabc123");
    assert_eq!(canonical_playlist_id("LM"), "LM");
    // Reads accept either form.
    assert_eq!(browse_playlist_id("PLabc123"), "VLPLabc123");
    assert_eq!(browse_playlist_id("VLPLabc123"), "VLPLabc123");
    assert_eq!(browse_playlist_id("LM"), "VLLM");
    assert_eq!(browse_playlist_id("VLLM"), "VLLM");
    // Writes accept either form (the reported 400 came from sending
    // a VL browse id to browse/edit_playlist).
    assert_eq!(edit_playlist_id("VLPLabc123"), "PLabc123");
    assert_eq!(edit_playlist_id("PLabc123"), "PLabc123");
    assert_eq!(edit_playlist_id("VLLM"), "LM");
}

#[test]
fn library_playlist_rows_store_canonical_ids() {
    let row: ytmapi_rs::parse::LibraryPlaylist = serde_json::from_value(serde_json::json!({
        "playlist_id": "VLPLserver123",
        "title": "Road Trip",
        "thumbnails": [],
        "tracks": "10 songs",
        "author": "Someone",
        "author_id": null,
    }))
    .unwrap();
    assert_eq!(
        server_playlist_of(&row),
        ServerPlaylist {
            id: "PLserver123".into(),
            title: "Road Trip".into(),
        }
    );
}

#[test]
fn lyrics_response_splits_into_plain_lines() {
    // `Lyrics` is `#[non_exhaustive]`, so build it via serde.
    let lyrics: ytmapi_rs::parse::Lyrics = serde_json::from_value(serde_json::json!({
        "lyrics": "line one\nline two\n\nline four",
        "source": "LyricFind",
    }))
    .unwrap();
    assert_eq!(lyrics.source, "LyricFind");
    let lines: Vec<String> = lyrics.lyrics.lines().map(str::to_string).collect();
    assert_eq!(lines, vec!["line one", "line two", "", "line four"]);
}

// ---- OAuth persist (offline: fabricated token JSON, no network) ----

/// Minimal `OAuthToken` JSON mirroring its serde shape (private fields are
/// still deserializable from outside the crate).
fn fabricated_oauth_json() -> String {
    serde_json::json!({
        "token_type": "Bearer",
        "access_token": "ya29.test-access-TOKEN",
        "refresh_token": "1//test-refresh-SECRET",
        "expires_in": 3599,
        "request_time": {"secs_since_epoch": 1790000000, "nanos_since_epoch": 0},
        "client_id": "test-client-id",
        "client_secret": "test-client-SECRET"
    })
    .to_string()
}

#[test]
fn oauth_persist_round_trips_through_file() {
    use crate::auth::OAuthPersist;
    use ytmapi_rs::auth::oauth::OAuthToken;

    let token: OAuthToken =
        serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
    let persist = OAuthPersist::from_token(token);
    let dir = std::env::temp_dir();
    let path = dir.join("yutu-test-oauth-persist.json");
    persist.save_to_file(&path).expect("save oauth persist");
    let loaded = OAuthPersist::load_from_file(&path).expect("load oauth persist");
    // Same token material survives the round trip (compared via re-serialised form).
    let before = serde_json::to_string(persist.token()).unwrap();
    let after = serde_json::to_string(loaded.token()).unwrap();
    assert_eq!(before, after);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn oauth_persist_file_is_owner_only() {
    use crate::auth::OAuthPersist;
    use std::os::unix::fs::PermissionsExt;
    use ytmapi_rs::auth::oauth::OAuthToken;

    let token: OAuthToken =
        serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
    let dir = std::env::temp_dir();
    let path = dir.join("yutu-test-oauth-perms.json");
    // Pre-create with loose perms: save must tighten them.
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    OAuthPersist::from_token(token)
        .save_to_file(&path)
        .expect("save oauth persist");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "oauth file must be owner-only, got {mode:o}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn oauth_persist_never_leaks_in_debug_or_errors() {
    use crate::auth::OAuthPersist;
    use ytmapi_rs::auth::oauth::OAuthToken;

    let token: OAuthToken =
        serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
    let debug = format!("{:?}", OAuthPersist::from_token(token));
    assert!(
        !debug.contains("ya29.test-access-TOKEN"),
        "access token leaked: {debug}"
    );
    assert!(
        !debug.contains("test-refresh-SECRET"),
        "refresh token leaked: {debug}"
    );
    assert!(
        !debug.contains("test-client-SECRET"),
        "client secret leaked: {debug}"
    );

    // Parse errors name the path but never echo file contents.
    let dir = std::env::temp_dir();
    let path = dir.join("yutu-test-oauth-garbage.json");
    std::fs::write(&path, "super-secret-garbage-TOKEN").unwrap();
    let err = OAuthPersist::load_from_file(&path).unwrap_err();
    assert!(
        err.contains("yutu-test-oauth-garbage.json"),
        "error names path: {err}"
    );
    assert!(
        !err.contains("super-secret-garbage-TOKEN"),
        "content leaked: {err}"
    );
    let missing = dir.join("yutu-test-oauth-missing.json");
    let _ = std::fs::remove_file(&missing);
    assert!(OAuthPersist::load_from_file(&missing).is_err());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn oauth_connect_builds_backend_without_network() {
    use crate::auth::OAuthPersist;
    use ytmapi_rs::auth::oauth::OAuthToken;

    let token: OAuthToken =
        serde_json::from_str(&fabricated_oauth_json()).expect("fabricated token parses");
    let backend = InnertubeBackend::connect_with_oauth(&OAuthPersist::from_token(token)).unwrap();
    assert!(matches!(backend.inner, Inner::OAuth(_)));
}

#[test]
fn library_playlist_rows_map_to_server_ids() {
    let row: ytmapi_rs::parse::LibraryPlaylist = serde_json::from_value(serde_json::json!({
        "playlist_id": "PLserver123",
        "title": "Road Trip",
        "thumbnails": [],
        "tracks": "10 songs",
        "author": "Someone",
        "author_id": null,
    }))
    .unwrap();
    assert_eq!(
        server_playlist_of(&row),
        ServerPlaylist {
            id: "PLserver123".into(),
            title: "Road Trip".into(),
        }
    );
}

#[tokio::test]
#[ignore = "live network: oauth refresh rejects non-OAuth backend (needs anonymous connect)"]
async fn oauth_refresh_rejects_non_oauth_backend() {
    let mut backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let err = backend.refresh_oauth().await.unwrap_err();
    assert!(err.contains("OAuth"), "unexpected error: {err}");
}

// ---- Live-network tests: always `#[ignore]`; CI runs offline. ----

#[tokio::test]
#[ignore = "live network: anonymous full top-songs shelf"]
async fn live_artist_top_full() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let artists = backend
        .search_artists("Queen")
        .await
        .expect("search artists");
    let queen = artists
        .iter()
        .find(|a| a.name == "Queen")
        .expect("Queen in artist results");
    let top = backend
        .artist_top_full(&queen.id)
        .await
        .expect("full top songs");
    // Full shelf, not the 5-song typed head.
    assert!(top.len() > 5, "expected full shelf, got {}", top.len());
    assert!(
        top.iter().all(|t| !t.id.is_empty() && !t.title.is_empty()),
        "tracks must carry id and title: {top:?}"
    );
    // Playlist rows carry durations + albums; most rows carry counts.
    assert!(top.iter().any(|t| t.duration_secs > 0));
    assert!(top.iter().any(|t| t.album.is_some()));
    let counted = top.iter().filter(|t| t.plays.is_some()).count();
    assert!(counted * 2 >= top.len(), "expected counts on most rows");
}

#[tokio::test]
#[ignore = "live network: anonymous artist top songs with play counts"]
async fn live_artist_top_songs() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let artists = backend
        .search_artists("Queen")
        .await
        .expect("search artists");
    let queen = artists
        .iter()
        .find(|a| a.name == "Queen")
        .expect("Queen in artist results");
    let top = backend
        .artist_top_songs(&queen.id)
        .await
        .expect("top songs");
    assert!(!top.is_empty(), "expected top songs for Queen");
    assert!(
        top.iter().all(|t| !t.id.is_empty() && !t.title.is_empty()),
        "tracks must carry id and title: {top:?}"
    );
    assert!(
        top.iter().any(|t| t.plays.is_some()),
        "expected some play counts: {top:?}"
    );
}

#[tokio::test]
#[ignore = "live network: anonymous album tracklist with play counts"]
async fn live_album_tracklist() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let albums = backend
        .search_albums("A Night at the Opera Queen")
        .await
        .expect("search albums");
    let album = albums
        .iter()
        .find(|a| a.title == "A Night At The Opera" || a.title == "A Night at the Opera")
        .expect("album in results");
    let tracks = backend.album_tracklist(&album.id).await.expect("tracklist");
    assert!(!tracks.is_empty(), "expected album tracks");
    assert!(
        tracks
            .iter()
            .all(|t| !t.id.is_empty() && !t.title.is_empty()),
        "tracks must carry id and title: {tracks:?}"
    );
    assert!(
        tracks.iter().any(|t| t.plays.is_some()),
        "expected some play counts: {tracks:?}"
    );
}

#[tokio::test]
#[ignore = "live network: anonymous Innertube suggestions"]
async fn live_anonymous_search_suggestions() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let suggestions = backend
        .search_suggestions("Beatles")
        .await
        .expect("suggestions");
    assert!(!suggestions.is_empty());
}

#[tokio::test]
#[ignore = "live network: anonymous song search"]
async fn live_search_tracks() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let tracks = backend
        .search_tracks("Beatles")
        .await
        .expect("search tracks");
    assert!(
        !tracks.is_empty(),
        "expected non-empty track results for 'Beatles', got none"
    );
    assert!(
        tracks
            .iter()
            .all(|t| !t.id.is_empty() && !t.title.is_empty()),
        "tracks must carry id and title: {tracks:?}"
    );
}

#[tokio::test]
#[ignore = "live network: anonymous album search"]
async fn live_search_albums() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let albums = backend
        .search_albums("Beatles")
        .await
        .expect("search albums");
    assert!(
        !albums.is_empty(),
        "expected non-empty album results for 'Beatles', got none"
    );
}

#[tokio::test]
#[ignore = "live network: anonymous artist search"]
async fn live_search_artists() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let artists = backend
        .search_artists("Beatles")
        .await
        .expect("search artists");
    assert!(
        !artists.is_empty(),
        "expected non-empty artist results for 'Beatles', got none"
    );
}

#[tokio::test]
#[ignore = "live network: anonymous raw unfiltered search"]
async fn live_search_all() {
    let all =
        crate::raw_search::raw_search_all("Beatles", &crate::raw_search::SearchAuth::anonymous())
            .await
            .expect("search all");
    assert!(
        !all.tracks.is_empty(),
        "expected non-empty track results for 'Beatles', got none"
    );
    assert!(
        all.tracks
            .iter()
            .all(|t| !t.id.is_empty() && !t.title.is_empty()),
        "tracks must carry id and title: {:?}",
        all.tracks
    );
    assert!(!all.mixed.is_empty(), "expected web mixed order");
}

#[test]
fn duration_parsing_covers_shapes() {
    assert_eq!(parse_duration_secs("3:45"), 225);
    assert_eq!(parse_duration_secs("1:02:03"), 3723);
    assert_eq!(parse_duration_secs("45"), 45);
    assert_eq!(parse_duration_secs(""), 0);
    assert_eq!(parse_duration_secs("3:xx"), 0);
}

#[test]
fn album_year_parsing_rejects_garbage() {
    // `SearchResultAlbum` is `#[non_exhaustive]`; build via serde.
    let album: SearchResultAlbum = serde_json::from_value(serde_json::json!({
        "title": "Abbey Road",
        "artist": "The Beatles",
        "year": "1969",
        "explicit": "NotExplicit",
        "album_id": "MPREabc",
        "album_type": "Album",
        "thumbnails": []
    }))
    .unwrap();
    let mapped = album_to_album(&album);
    assert_eq!(mapped.year, Some(1969));
    assert_eq!(mapped.id, "MPREabc");
    let mut bad = serde_json::to_value(&album).unwrap();
    bad["year"] = serde_json::json!("unknown");
    let bad: SearchResultAlbum = serde_json::from_value(bad).unwrap();
    assert_eq!(album_to_album(&bad).year, None);
}

#[tokio::test]
#[ignore = "live network: player probe fallback chain"]
async fn live_resolve_stream_records_fallback() {
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    // A well-known video id; outcome depends on network/region, so only
    // the chain bookkeeping is asserted.
    let mut req = StreamRequest::new("dQw4w9WgXcQ");
    let _ = backend.resolve_stream(&mut req).await;
    assert_eq!(req.tried.first(), Some(&StreamClient::AndroidVr));
    assert!(!req.tried.is_empty());
}

// NOTE (Oct 2026): the raw sidecar chain below cannot resolve ciphered
// formats without a deobfuscator, so no live test asserts full resolution
// here. Real end-to-end proof lives in
// `crates/yutu-player/tests/e2e_audio.rs` (yt-dlp resolve -> fetch ->
// decode, ignored without binary+network).
#[tokio::test]
#[ignore = "live network: sidecar mint still yields tokens (resolution itself is yt-dlp's job now)"]
async fn live_sidecar_still_mints_tokens() {
    let Some(bin) = crate::pot::find_botguard_bin() else {
        eprintln!("SKIP: rustypipe-botguard not installed");
        return;
    };
    let video = std::env::var("YUTU_TEST_VIDEO_ID").unwrap_or_else(|_| "dQw4w9WgXcQ".into());
    let minted = crate::pot::fetch_po_token(&bin, &video).expect("sidecar mints without crashing");
    assert!(
        !minted.tokens.is_empty(),
        "sidecar returned zero tokens for {video}"
    );
}

#[tokio::test]
#[ignore = "live network: OAuth library songs"]
async fn live_oauth_library_songs() {
    use crate::auth::OAuthPersist;

    let Ok(path) = std::env::var("YUTU_TEST_OAUTH_FILE") else {
        eprintln!("SKIP: set YUTU_TEST_OAUTH_FILE to a saved OAuth token file");
        return;
    };
    let persist = OAuthPersist::load_from_file(Path::new(&path)).expect("load oauth persist");
    let backend = InnertubeBackend::connect_with_oauth(&persist).expect("oauth connect");
    let songs = backend.library_songs().await.expect("library songs");
    assert!(
        !songs.is_empty(),
        "expected non-empty library songs for the OAuth account"
    );
}

#[tokio::test]
#[ignore = "live network: cookie library songs + liked songs"]
async fn live_cookie_library_songs() {
    let Ok(path) = std::env::var("YUTU_TEST_COOKIE_FILE") else {
        eprintln!("SKIP: set YUTU_TEST_COOKIE_FILE to a browser cookie file");
        return;
    };
    let backend = InnertubeBackend::connect_with_cookie_file(Path::new(&path))
        .await
        .expect("cookie connect");
    let songs = backend.library_songs().await.expect("library songs");
    assert!(
        !songs.is_empty(),
        "expected non-empty library songs for the cookie account"
    );
    // Liked playlist may legitimately be empty; only assert it resolves.
    let _ = backend.liked_songs().await.expect("liked songs");
}

#[tokio::test]
#[ignore = "live network: big playlist streams past the 100-item first page"]
async fn live_playlist_tracks_stream_all_pages() {
    let Ok(path) = std::env::var("YUTU_TEST_COOKIE_FILE") else {
        eprintln!("SKIP: set YUTU_TEST_COOKIE_FILE to a browser cookie file");
        return;
    };
    let Ok(playlist_id) = std::env::var("YUTU_TEST_BIG_PLAYLIST_ID") else {
        eprintln!("SKIP: set YUTU_TEST_BIG_PLAYLIST_ID to a 100+ song playlist id");
        return;
    };
    let backend = InnertubeBackend::connect_with_cookie_file(Path::new(&path))
        .await
        .expect("cookie connect");
    let mut calls = Vec::new();
    let tracks = backend
        .playlist_tracks_live(&playlist_id, |n| calls.push(n))
        .await
        .expect("tracks");
    assert!(
        tracks.len() > 100,
        "expected more than one page of tracks, got {}",
        tracks.len()
    );
    // Progress hook: per-page cumulative counts ending at the total.
    assert!(calls.len() >= 2, "expected page ticks, got {calls:?}");
    assert!(
        calls.windows(2).all(|w| w[0] < w[1]),
        "ticks must grow, got {calls:?}"
    );
    assert_eq!(*calls.last().unwrap(), tracks.len());
}

#[tokio::test]
#[ignore = "live network: YTM lyrics for a known video"]
async fn live_song_lyrics() {
    let Ok(video_id) = std::env::var("YUTU_TEST_LYRICS_VIDEO_ID") else {
        eprintln!("SKIP: set YUTU_TEST_LYRICS_VIDEO_ID to a video id with lyrics");
        return;
    };
    let backend = InnertubeBackend::connect_anonymous()
        .await
        .expect("anonymous connect");
    let lines = backend.song_lyrics(&video_id).await.expect("lyrics");
    assert!(!lines.is_empty(), "expected lyric lines for {video_id}");
}
