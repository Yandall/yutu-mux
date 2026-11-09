//! raw_search_tests: moved from raw_search.rs (no behavior change).

#[allow(unused_imports)]
use super::raw_search::*;
#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use serde_json::Value;

/// Minimal flat response: card + one of each playable kind plus
/// skippable kinds (episode, podcast, profile) and a broken item.
fn fixture() -> Value {
    serde_json::json!({
        "contents": {
            "tabbedSearchResultsRenderer": {
                "tabs": [{
                    "tabRenderer": {
                        "content": {
                            "sectionListRenderer": {
                                "contents": [
                                    {"musicCardShelfRenderer": {
                                        "title": {"runs": [{"text": "Chop Suey!"}]},
                                        "subtitle": {"runs": [
                                            {"text": "Video"}, {"text": " • "},
                                            {"text": "System Of A Down"}, {"text": " • "},
                                            {"text": "1.6B views"}, {"text": " • "},
                                            {"text": "3:29"},
                                        ]},
                                        "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                                            {"url": "https://example.invalid/s", "width": 60, "height": 60},
                                            {"url": "https://example.invalid/l", "width": 544, "height": 544},
                                        ]}}},
                                        "onTap": {"watchEndpoint": {"videoId": "CSvFpBOe8eY"}},
                                    }},
                                    {"itemSectionRenderer": {"contents": [
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Chop Suey!"}]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                    {"text": "Song"}, {"text": " • "}, {"text": "System Of A Down"},
                                                ]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "2.1B plays"}]}}},
                                            ],
                                            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                                                {"url": "https://example.invalid/t", "width": 60, "height": 60},
                                            ]}}},
                                            "playlistItemData": {"videoId": "-cid1qHuy_U"},
                                        }},
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "System of A Down"}]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                    {"text": "Artist"}, {"text": " • "}, {"text": "29.3M monthly audience"},
                                                ]}}},
                                            ],
                                            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                            "navigationEndpoint": {"browseEndpoint": {"browseId": "UC123"}},
                                        }},
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Chop Suey"}]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                    {"text": "Single"}, {"text": " • "},
                                                    {"text": "Vesanic Rave"}, {"text": " & "}, {"text": "Vesanic"},
                                                    {"text": " • "}, {"text": "2025"},
                                                ]}}},
                                            ],
                                            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                            "navigationEndpoint": {"browseEndpoint": {"browseId": "MPRE456"}},
                                        }},
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Mix"}]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                    {"text": "Playlist"}, {"text": " • "},
                                                    {"text": "YouTube Music"}, {"text": " • "}, {"text": "31 songs"},
                                                ]}}},
                                            ],
                                            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                                            "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPL789"}},
                                        }},
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Show"}]}}},
                                                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                                                    {"text": "Podcast"}, {"text": " • "}, {"text": "Pub"},
                                                ]}}},
                                            ],
                                            "navigationEndpoint": {"browseEndpoint": {"browseId": "MDP123"}},
                                        }},
                                        {"musicResponsiveListItemRenderer": {
                                            "flexColumns": [],
                                        }},
                                    ]}},
                                ],
                            },
                        },
                    },
                }],
            },
        },
    })
}

#[test]
fn flat_response_parses_in_server_order() {
    let out = parse_search_all(&fixture().to_string()).expect("parse");
    let top = out.top.expect("top card");
    // The top card is the server's pick verbatim (song preference
    // applies to rows, never to the ranked top hit: it carries no
    // duration to disambiguate long videos from songs).
    assert_eq!(top.kind, TopKind::Video);
    assert_eq!(top.id, "CSvFpBOe8eY");
    assert_eq!(top.artist, "System Of A Down");
    assert_eq!(top.plays.as_deref(), Some("1.6B views"));
    assert_eq!(
        top.thumbnail_url.as_deref(),
        Some("https://example.invalid/s")
    );
    // The card recording has no shelf row, so it is synthesized
    // first (hero plays the server pick, not the first shelf row);
    // the shelf song keeps its counts behind it.
    assert_eq!(out.tracks.len(), 2);
    assert_eq!(out.tracks[0].id, "CSvFpBOe8eY");
    assert_eq!(out.tracks[0].title, "Chop Suey!");
    assert_eq!(out.tracks[0].artist, "System Of A Down");
    assert!(out.tracks[0].is_video);
    assert_eq!(out.tracks[0].plays.as_deref(), Some("1.6B views"));
    assert_eq!(out.tracks[1].id, "-cid1qHuy_U");
    assert!(!out.tracks[1].is_video);
    assert_eq!(out.tracks[1].plays.as_deref(), Some("2.1B plays"));
    assert_eq!(out.artists.len(), 1);
    assert_eq!(out.artists[0].id, "UC123");
    assert_eq!(out.albums.len(), 1);
    assert_eq!(out.albums[0].id, "MPRE456");
    assert_eq!(out.albums[0].artist, "Vesanic Rave & Vesanic");
    assert_eq!(out.albums[0].year, Some(2025));
    assert_eq!(out.playlists.len(), 1);
    assert_eq!(out.playlists[0].author, "YouTube Music");
    assert_eq!(out.playlists[0].detail, "31 songs");
    // Podcast + broken item skipped; mixed is the web row order.
    assert_eq!(
        out.mixed
            .iter()
            .map(|m| (m.kind, m.id.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "-cid1qHuy_U"),
            (1, "UC123"),
            (2, "MPRE456"),
            (3, "VLPL789"),
        ]
    );
}

/// One flat item with the given subtitle runs + endpoint bits.
fn item(title: &str, sub: Vec<&str>, extra: serde_json::Value) -> Value {
    let runs: Vec<Value> = sub
        .into_iter()
        .map(|t| serde_json::json!({"text": t}))
        .collect();
    let mut item = serde_json::json!({
        "flexColumns": [
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title}]}}},
            {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": runs}}},
        ],
        "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
    });
    for (k, v) in extra.as_object().unwrap() {
        item[k] = v.clone();
    }
    item
}

fn wrap(items: Vec<Value>) -> String {
    let contents: Vec<Value> = items
        .into_iter()
        .map(|r| serde_json::json!({"musicResponsiveListItemRenderer": r}))
        .collect();
    serde_json::json!({
        "contents": {"tabbedSearchResultsRenderer": {"tabs": [{
            "tabRenderer": {"content": {"sectionListRenderer": {"contents": [
                {"itemSectionRenderer": {"contents": contents}},
            ]}}},
        }]}},
    })
    .to_string()
}

#[test]
fn multi_artist_segments_keep_album_position() {
    // "Song • A & B" (one separator): artists join, no album.
    let out = parse_search_all(&wrap(vec![item(
        "Duo",
        vec!["Song", " • ", "A", " & ", "B"],
        serde_json::json!({"playlistItemData": {"videoId": "v1"}}),
    )]))
    .expect("parse");
    assert_eq!(out.tracks.len(), 1);
    assert_eq!(out.tracks[0].artist, "A & B");
    assert!(out.tracks[0].album.is_none());
    // "Song • A • Album" (two separators): last segment is the album.
    let out = parse_search_all(&wrap(vec![item(
        "Solo",
        vec!["Song", " • ", "A", " • ", "Album"],
        serde_json::json!({"playlistItemData": {"videoId": "v2"}}),
    )]))
    .expect("parse");
    assert_eq!(out.tracks[0].artist, "A");
    assert_eq!(out.tracks[0].album.as_deref(), Some("Album"));
}

#[test]
fn radio_mixes_skip_but_bare_playlists_map() {
    let out = parse_search_all(&wrap(vec![
        item(
            "Mix",
            vec!["Playlist", " • ", "YTM", " • ", "31 songs"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "RDCLAK123"}}}),
        ),
        item(
            "Presenting Someone",
            vec!["Playlist", " • ", "YouTube Music", " • ", "67 songs"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "VLRDCLAK123"}}}),
        ),
        item(
            "Fan",
            vec!["Playlist", " • ", "Fan", " • ", "5 views"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "PL456"}}}),
        ),
    ]))
    .expect("parse");
    assert_eq!(out.playlists.len(), 1);
    assert_eq!(out.playlists[0].id, "PL456");
    assert_eq!(out.mixed.len(), 1);
}

#[test]
fn podcast_episodes_and_profiles_never_misfile() {
    // Episode rows (even in a forced Songs shelf) are skipped, not
    // songs; profile channels are skipped, not artists.
    let episode = || {
        let mut extra = with_video("ep1");
        // with_video marks OMV: episodes ride the podcast type.
        extra["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]
            ["musicPlayButtonRenderer"]["playNavigationEndpoint"]["watchEndpoint"]
            ["watchEndpointMusicSupportedConfigs"]["watchEndpointMusicConfig"]["musicVideoType"] =
            serde_json::json!("MUSIC_VIDEO_TYPE_PODCAST_EPISODE");
        extra["playlistItemData"] = serde_json::json!({"videoId": "ep1"});
        extra
    };
    let out = parse_search_all(&wrap(vec![
        item(
            "Show",
            vec!["Episode", " • ", "Oct 30, 2019", " • ", "Pub"],
            episode(),
        ),
        item(
            "Fan",
            vec!["Profile", " • ", "@fan"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "UC999"}}}),
        ),
        item(
            "Band",
            vec!["Artist", " • ", "1M monthly audience"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "UC111"}}}),
        ),
    ]))
    .expect("parse");
    assert!(out.tracks.is_empty());
    assert_eq!(out.artists.len(), 1);
    assert_eq!(out.artists[0].id, "UC111");
    assert_eq!(
        out.mixed
            .iter()
            .map(|m| (m.kind, m.id.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "UC111")]
    );
}

#[test]
fn top_card_without_a_shelf_row_synthesizes_once() {
    // Card video already shelved: no duplicate row.
    let card = serde_json::json!({
        "musicCardShelfRenderer": {
            "title": {"runs": [{"text": "Hey"}]},
            "subtitle": {"runs": [
                {"text": "Song"}, {"text": " • "}, {"text": "Band"},
            ]},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "https://example.invalid/s", "width": 60, "height": 60},
            ]}}},
            "onTap": {"watchEndpoint": {"videoId": "v1"}},
        },
    });
    let mut root: Value = serde_json::from_str(&wrap(vec![item(
        "Hey",
        vec!["Song", " • ", "Band"],
        serde_json::json!({"playlistItemData": {"videoId": "v1"}}),
    )]))
    .expect("wrap parses");
    root["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
        ["sectionListRenderer"]["contents"]
        .as_array_mut()
        .expect("sections")
        .insert(0, card);
    let out = parse_search_all(&root.to_string()).expect("parse");
    assert_eq!(out.tracks.len(), 1);
    assert_eq!(out.tracks[0].id, "v1");
}

#[test]
fn top_artist_card_without_a_shelf_row_synthesizes() {
    // The ranked artist has no shelf row (only the card): it is
    // synthesized so the hero resolves by id instead of falling
    // back to the first shelf row.
    let card = serde_json::json!({
        "musicCardShelfRenderer": {
            "title": {"runs": [{"text": "Héroes del Silencio"}]},
            "subtitle": {"runs": [
                {"text": "Artist"}, {"text": " • "}, {"text": "7.02M monthly audience"},
            ]},
            "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [
                {"url": "https://example.invalid/a", "width": 60, "height": 60},
            ]}}},
            "onTap": {"browseEndpoint": {"browseId": "UC123"}},
        },
    });
    let mut root: Value = serde_json::from_str(&wrap(vec![item(
        "Heroes",
        vec!["Song", " • ", "Band"],
        serde_json::json!({"playlistItemData": {"videoId": "v1"}}),
    )]))
    .expect("wrap parses");
    root["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
        ["sectionListRenderer"]["contents"]
        .as_array_mut()
        .expect("sections")
        .insert(0, card);
    let out = parse_search_all(&root.to_string()).expect("parse");
    assert_eq!(out.artists.len(), 1);
    assert_eq!(out.artists[0].id, "UC123");
    assert_eq!(out.artists[0].name, "Héroes del Silencio");
    assert!(out.mixed.iter().all(|m| m.id != "UC123"));
}

#[test]
fn unknown_tokens_need_a_player_signal() {
    // Unknown token + no player type: skipped, never misfiled.
    let out = parse_search_all(&wrap(vec![item(
        "???",
        vec!["Karaoke", " • ", "Someone"],
        serde_json::json!({"playlistItemData": {"videoId": "v9"}}),
    )]))
    .expect("parse");
    assert!(out.tracks.is_empty() && out.mixed.is_empty());
    // Unknown token + ATV overlay: trusted as a song.
    let out = parse_search_all(&wrap(vec![item(
        "???",
        vec!["Karaoke", " • ", "Someone"],
        serde_json::json!({
            "playlistItemData": {"videoId": "v9"},
            "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
                "playNavigationEndpoint": {"watchEndpoint": {
                    "videoId": "v9",
                    "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}},
                }},
            }}}},
        }),
    )]))
    .expect("parse");
    assert_eq!(out.tracks.len(), 1);
}

#[test]
fn old_shelf_layout_maps_by_title() {
    let shelf = serde_json::json!({
        "musicShelfRenderer": {
            "title": {"runs": [{"text": "Songs"}]},
            "contents": [
                {"musicResponsiveListItemRenderer": {
                    "flexColumns": [
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Hey"}]}}},
                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                            {"text": "Song"}, {"text": " • "}, {"text": "Band"},
                        ]}}},
                    ],
                    "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": []}}},
                    "playlistItemData": {"videoId": "vshelf"},
                }},
            ],
        },
    });
    let text = serde_json::json!({
        "contents": {"tabbedSearchResultsRenderer": {"tabs": [{
            "tabRenderer": {"content": {"sectionListRenderer": {"contents": [shelf]}}},
        }]}},
    })
    .to_string();
    let out = parse_search_all(&text).expect("parse");
    assert_eq!(out.tracks.len(), 1);
    assert_eq!(out.tracks[0].id, "vshelf");
}

#[test]
fn bare_shelf_songs_parse_artist_album_duration() {
    // Filtered songs omit the type token: artist, album, duration.
    // (The overlay player type is the classification signal.)
    let atv = |id: &str| {
        serde_json::json!({"overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
            "playNavigationEndpoint": {"watchEndpoint": {
                "videoId": id,
                "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}},
            }},
        }}}}})
    };
    let with_overlay = |id: &str| {
        let mut extra = serde_json::json!({"playlistItemData": {"videoId": id}});
        extra["overlay"] = atv(id)["overlay"].clone();
        extra
    };
    let out = parse_search_all(&wrap(vec![item(
        "Chop Suey!",
        vec!["System Of A Down", " • ", "Toxicity", " • ", "3:31"],
        with_overlay("v1"),
    )]))
    .expect("parse");
    assert_eq!(out.tracks.len(), 1);
    assert_eq!(out.tracks[0].artist, "System Of A Down");
    assert_eq!(out.tracks[0].album.as_deref(), Some("Toxicity"));
    assert_eq!(out.tracks[0].duration_secs, 211);
    // No-album shape keeps the duration out of the album slot.
    let out = parse_search_all(&wrap(vec![item(
        "Intro",
        vec!["Band", " • ", "0:42"],
        with_overlay("v2"),
    )]))
    .expect("parse");
    assert_eq!(out.tracks[0].artist, "Band");
    assert!(out.tracks[0].album.is_none());
    assert_eq!(out.tracks[0].duration_secs, 42);
    // Bare shelf playlists map author + detail without a token.
    let out = parse_search_all(&wrap(vec![item(
        "Mix",
        vec!["Fan", " • ", "56 views"],
        serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "VL1"}}}),
    )]))
    .expect("parse");
    assert_eq!(out.playlists.len(), 1);
    assert_eq!(out.playlists[0].author, "Fan");
    assert_eq!(out.playlists[0].detail, "56 views");
}

#[test]
fn missing_section_list_errors_but_bad_items_do_not() {
    assert!(parse_search_all("{}").is_err());
    assert!(parse_search_all("not json").is_err());
    let mut broken = fixture();
    broken["contents"]["tabbedSearchResultsRenderer"]["tabs"][0]["tabRenderer"]["content"]
        ["sectionListRenderer"]["contents"][1]["itemSectionRenderer"]["contents"]
        .as_array_mut()
        .unwrap()
        .clear();
    let out = parse_search_all(&broken.to_string()).expect("parse");
    // No shelf rows survive, but the top card still synthesizes its
    // playable row (mixed stays empty: no duplicate row renders).
    assert_eq!(out.tracks.len(), 1);
    assert_eq!(out.tracks[0].id, "CSvFpBOe8eY");
    assert!(out.mixed.is_empty());
    assert!(out.top.is_some());
}

#[test]
fn filter_params_match_web_tabs() {
    // Same `params` the web client sends per tab (mirrors the
    // ytmapi-rs filtered queries with ExactMatch spelling).
    assert_eq!(
        SearchFilterKind::Songs.params(),
        "EgWKAQIIAWoMEA4QChADEAQQCRAF"
    );
    assert_eq!(
        SearchFilterKind::Albums.params(),
        "EgWKAQIYAWoMEA4QChADEAQQCRAF"
    );
    assert_eq!(
        SearchFilterKind::Artists.params(),
        "EgWKAQIgAWoMEA4QChADEAQQCRAF"
    );
    assert_eq!(
        SearchFilterKind::Playlists.params(),
        "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU%3D"
    );
}

#[test]
fn sapisid_hash_shape() {
    let hash = sapisid_hash(1234567890, "abcDEF123");
    assert!(hash.starts_with("1234567890_"));
    assert_eq!(hash.len(), 11 + 40);
    assert!(hash[11..].chars().all(|c| c.is_ascii_hexdigit()));
}

/// Extra payload for video rows: playlist id plus the OMV player
/// type (the mirror of the ATV helper elsewhere).
fn with_video(id: &str) -> Value {
    serde_json::json!({
        "playlistItemData": {"videoId": id},
        "overlay": {"musicItemThumbnailOverlayRenderer": {"content": {"musicPlayButtonRenderer": {
            "playNavigationEndpoint": {"watchEndpoint": {
                "videoId": id,
                "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": "MUSIC_VIDEO_TYPE_OMV"}},
            }},
        }}}},
    })
}

#[test]
fn song_preference_drops_duplicate_videos() {
    let out = parse_search_all(&wrap(vec![
        item(
            "Chop Suey!",
            vec!["Song", " • ", "System Of A Down"],
            serde_json::json!({"playlistItemData": {"videoId": "song1"}}),
        ),
        item(
            "Chop Suey! (Official Video)",
            vec!["Video", " • ", "System Of A Down"],
            with_video("vid1"),
        ),
        // Interleaved kinds hold their mixed positions around drops.
        item(
            "System of A Down",
            vec!["Artist", " • ", "29.3M monthly audience"],
            serde_json::json!({"navigationEndpoint": {"browseEndpoint": {"browseId": "UC123"}}}),
        ),
        item(
            "Lonely Day",
            vec!["Video", " • ", "System Of A Down"],
            with_video("vid2"),
        ),
        item(
            "Toxicity",
            vec!["Song", " • ", "System Of A Down"],
            serde_json::json!({"playlistItemData": {"videoId": "song2"}}),
        ),
    ]))
    .expect("parse");
    // Dup video gone, everything else in walk order (mixed too).
    let ids: Vec<&str> = out.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["song1", "vid2", "song2"]);
    let mixed: Vec<(u8, &str)> = out.mixed.iter().map(|m| (m.kind, m.id.as_str())).collect();
    assert_eq!(
        mixed,
        vec![(0, "song1"), (1, "UC123"), (0, "vid2"), (0, "song2")]
    );
}

#[test]
fn song_preference_keeps_videos_without_a_song_twin() {
    // Different durations: different recordings, both stay.
    let out = parse_search_all(&wrap(vec![
        item(
            "Chop Suey!",
            vec!["Song", " • ", "Band", " • ", "Album", " • ", "3:31"],
            serde_json::json!({"playlistItemData": {"videoId": "song1"}}),
        ),
        item(
            "Chop Suey!",
            vec!["Video", " • ", "Band", " • ", "1M views", " • ", "9:99"],
            with_video("vid1"),
        ),
        // Same duration: the video is the same track, dropped.
        item(
            "Intro",
            vec!["Song", " • ", "Band", " • ", "0:42"],
            serde_json::json!({"playlistItemData": {"videoId": "song2"}}),
        ),
        item(
            "Intro",
            vec!["Video", " • ", "Band", " • ", "9 views", " • ", "0:42"],
            with_video("vid2"),
        ),
        // Remaster markers are not video markers: both stay.
        item(
            "Song",
            vec!["Song", " • ", "Band"],
            serde_json::json!({"playlistItemData": {"videoId": "song3"}}),
        ),
        item(
            "Song (Remastered)",
            vec!["Video", " • ", "Band"],
            with_video("vid3"),
        ),
        // Topic channels match the plain artist name.
        item(
            "Anthem",
            vec!["Song", " • ", "Band"],
            serde_json::json!({"playlistItemData": {"videoId": "song4"}}),
        ),
        item(
            "Anthem [Video]",
            vec!["Video", " • ", "Band - Topic"],
            with_video("vid4"),
        ),
    ]))
    .expect("parse");
    let ids: Vec<&str> = out.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["song1", "vid1", "song2", "song3", "vid3", "song4"]
    );
}

#[test]
fn norm_title_strips_video_markers_only() {
    assert_eq!(norm_title("Chop Suey! (Official Video)"), "chop suey!");
    assert_eq!(norm_title("Anthem [Official Music Video]"), "anthem");
    assert_eq!(norm_title("Song - Music Video"), "song");
    assert_eq!(norm_title("Anthem [Official Lyric Video]"), "anthem");
    assert_eq!(norm_title("Clip (Visualizer)"), "clip");
    // Non-video parentheticals survive (never merge remasters).
    assert_eq!(norm_title("Song (Remastered)"), "song (remastered)");
    assert_eq!(norm_title("Song (Live)"), "song (live)");
    assert_eq!(norm_artist("Band - Topic"), "band");
    assert_eq!(norm_artist("BandVEVO"), "band");
}

#[test]
fn cookie_sapisid_extracts() {
    let auth = SearchAuth::cookie("SID=x; SAPISID=mysecret; HSID=y".into());
    assert_eq!(auth.sapisid(), Some("mysecret"));
    assert!(SearchAuth::anonymous().sapisid().is_none());
}

#[tokio::test]
#[ignore = "live network: filtered artist search resolves Queen"]
async fn live_filtered_artist_search_finds_queen() {
    // Guards the "Go to artist" resolve path for names the page
    // search never showed as a card: the Artists tab must still
    // carry the channel id.
    let bundle = raw_search_filtered("queen", &SearchAuth::anonymous(), SearchFilterKind::Artists)
        .await
        .expect("filtered artist search");
    assert!(
        bundle
            .artists
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case("queen")),
        "Queen missing from filtered artists"
    );
}
