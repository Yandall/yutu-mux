//! Artist page shelves: arrival, card metadata, "More" routing, video
//! play, and the rendered page.

use super::*;
use yutu_core::{ArtistPage, ArtistShelf, ShelfItem, ShelfItemKind, ShelfMore};

fn test_state() -> AppState {
    let path = std::env::temp_dir().join(format!(
        "yutu-shelves-test-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let (s, _) = AppState::with_library_path(path.to_string_lossy().into_owned());
    s
}

fn item(kind: ShelfItemKind, id: &str, title: &str, subtitle: &str) -> ShelfItem {
    ShelfItem {
        kind,
        id: id.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        year: yutu_core_year(subtitle),
        thumbnail_url: None,
    }
}

fn yutu_core_year(subtitle: &str) -> Option<u16> {
    subtitle
        .split(|c: char| !c.is_ascii_digit())
        .find(|t| t.len() == 4)
        .and_then(|t| t.parse().ok())
}

fn queen_page() -> ArtistPage {
    ArtistPage {
        name: "Queen".into(),
        thumbnail_url: Some("https://example.invalid/queen.jpg".into()),
        shelves: vec![
            ArtistShelf {
                title: "Albums".into(),
                items: vec![item(ShelfItemKind::Album, "MPREa", "Innuendo", "1991")],
                more: Some(ShelfMore::Grid {
                    browse_id: "MPADq".into(),
                    params: "p=".into(),
                }),
            },
            ArtistShelf {
                title: "Videos".into(),
                items: vec![item(
                    ShelfItemKind::Video,
                    "vid1",
                    "We Will Rock You (Live)",
                    "QueenVEVO \u{b7} 1.2M views",
                )],
                more: Some(ShelfMore::Playlist("OLAKv".into())),
            },
            ArtistShelf {
                title: "Featured on".into(),
                items: vec![item(
                    ShelfItemKind::Playlist,
                    "RDCLAKp",
                    "Presenting Queen",
                    "Playlist \u{b7} YouTube Music",
                )],
                more: None,
            },
            ArtistShelf {
                title: "Fans might also like".into(),
                items: vec![item(ShelfItemKind::Artist, "UCbowie", "David Bowie", "")],
                more: None,
            },
        ],
    }
}

/// Artist page open on Queen's channel with the shelves landed.
fn queen_state() -> AppState {
    let mut s = test_state();
    s.artist_alias.insert("Queen".into(), "UCq".into());
    s.route = Route::Artist("Queen".into());
    s.artist_page_loading = Some("UCq".into());
    let _ = s.update(Message::ArtistPageLoaded {
        channel_id: "UCq".into(),
        result: Ok(queen_page()),
    });
    s
}

/// Fetch markers only exist in network builds.
#[cfg(feature = "innertube")]
#[test]
fn opening_an_artist_fetches_its_shelves_once() {
    let mut s = test_state();
    s.results.artists.push(yutu_core::Artist {
        id: "UCq".into(),
        name: "Queen".into(),
        thumbnail_url: None,
    });
    let _ = s.update(Message::OpenArtist("UCq".into()));
    assert_eq!(s.artist_page_loading.as_deref(), Some("UCq"));
    assert!(s.artist_page_pending("UCq"));
    // A cached page never refetches.
    let _ = s.update(Message::ArtistPageLoaded {
        channel_id: "UCq".into(),
        result: Ok(queen_page()),
    });
    assert_eq!(s.artist_page_loading, None);
    let _ = s.update(Message::OpenArtist("UCq".into()));
    assert_eq!(s.artist_page_loading, None);
}

/// Fetch markers only exist in network builds.
#[cfg(feature = "innertube")]
#[test]
fn resolved_names_chain_into_the_shelf_fetch() {
    let mut s = test_state();
    s.artist_resolving = Some("Queen".into());
    let _ = s.update(Message::ArtistChannelResolved {
        route_id: "Queen".into(),
        name: "Queen".into(),
        result: Ok("UCq".into()),
    });
    assert_eq!(s.artist_loading.as_deref(), Some("UCq"));
    assert_eq!(s.artist_page_loading.as_deref(), Some("UCq"));
}

#[test]
fn shelf_cards_resolve_like_search_hits() {
    let s = queen_state();
    // Album cards: title, owner, year and browse id without a search.
    assert_eq!(s.resolve_album_title("MPREa"), "Innuendo");
    assert_eq!(s.album_browse_id("MPREa").as_deref(), Some("MPREa"));
    assert_eq!(s.album_owner("MPREa").as_deref(), Some("Queen"));
    assert_eq!(s.album_menu_info("MPREa").2, Some(1991));
    // Related artists open on their own channel and name.
    assert_eq!(s.artist_channel_id("UCbowie").as_deref(), Some("UCbowie"));
    assert_eq!(s.resolve_artist_name("UCbowie"), "David Bowie");
    // Playlists get their page header and menu.
    assert_eq!(s.search_playlist_label("RDCLAKp"), "Presenting Queen");
    // The page header photo comes from the shelf page.
    assert!(s.artist_photo("Queen").is_some());
}

#[test]
fn empty_or_failed_shelf_pages_stay_uncached() {
    let mut s = test_state();
    s.artist_alias.insert("Queen".into(), "UCq".into());
    s.route = Route::Artist("Queen".into());
    s.artist_page_loading = Some("UCq".into());
    let _ = s.update(Message::ArtistPageLoaded {
        channel_id: "UCq".into(),
        result: Ok(ArtistPage::default()),
    });
    assert!(!s.artist_pages.contains_key("UCq"));
    s.artist_page_loading = Some("UCq".into());
    let _ = s.update(Message::ArtistPageLoaded {
        channel_id: "UCq".into(),
        result: Err("offline".into()),
    });
    assert!(!s.artist_pages.contains_key("UCq"));
    assert!(s.status.contains("failed"), "{}", s.status);
}

/// Fetch markers only exist in network builds.
#[cfg(feature = "innertube")]
#[test]
fn more_opens_the_grid_or_the_videos_playlist() {
    let mut s = queen_state();
    let _ = s.update(Message::OpenArtistShelfMore {
        channel: "UCq".into(),
        shelf: 0,
    });
    assert_eq!(
        s.route,
        Route::ArtistShelf {
            channel: "UCq".into(),
            shelf: 0
        }
    );
    assert_eq!(s.artist_grid_loading.as_deref(), Some("MPADq|p="));
    // The shelf's own cards stand in until the grid lands.
    assert_eq!(s.artist_shelf_cards("UCq", 0).len(), 1);
    let _ = s.update(Message::ArtistGridLoaded {
        key: "MPADq|p=".into(),
        result: Ok(vec![
            item(ShelfItemKind::Album, "MPREa", "Innuendo", "1991"),
            item(ShelfItemKind::Album, "MPREb", "Jazz", "1978"),
        ]),
    });
    assert_eq!(s.artist_grid_loading, None);
    assert_eq!(s.artist_shelf_cards("UCq", 0).len(), 2);
    // Grid-only albums resolve too.
    assert_eq!(s.resolve_album_title("MPREb"), "Jazz");
    assert_eq!(s.album_owner("MPREb").as_deref(), Some("Queen"));

    // Videos "More" is a plain playlist page.
    let _ = s.update(Message::OpenArtistShelfMore {
        channel: "UCq".into(),
        shelf: 1,
    });
    assert_eq!(s.route, Route::RemotePlaylist("OLAKv".into()));
    assert_eq!(s.search_playlist_label("OLAKv"), "Videos");
}

#[test]
fn video_cards_play_as_the_artist() {
    let mut s = queen_state();
    let page = s.artist_pages.get("UCq").cloned().unwrap();
    let track = AppState::shelf_video_track(&page.shelves[1].items[0], &page.name);
    assert!(track.is_video);
    assert_eq!(track.artist, "Queen", "page artist, not the uploader");
    let _ = s.update(Message::ShelfVideo {
        track: track.clone(),
        menu: true,
    });
    assert!(s.find_track("vid1").is_some());
    let _ = s.update(Message::ShelfVideo { track, menu: false });
    assert_eq!(s.queue.current().map(|t| t.id.as_str()), Some("vid1"));
}

#[test]
fn artist_page_renders_every_shelf_with_more_pills() {
    for language in [i18n::Language::English, i18n::Language::Spanish] {
        let mut s = queen_state();
        s.language = language;
        let size = iced::Size::new(1280.0, 1400.0);
        let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
        for title in ["Albums", "Videos", "Featured on", "Fans might also like"] {
            assert!(sim.find(title).is_ok(), "{title}");
        }
        assert!(sim.find("Innuendo").is_ok());
        assert!(sim.find("David Bowie").is_ok());
        assert!(sim.find(language.shelf_more().as_str()).is_ok());
    }
}

#[test]
fn shelf_grid_page_renders_its_cards() {
    let mut s = queen_state();
    let _ = s.update(Message::OpenArtistShelfMore {
        channel: "UCq".into(),
        shelf: 0,
    });
    let _ = s.update(Message::ArtistGridLoaded {
        key: "MPADq|p=".into(),
        result: Ok(vec![
            item(ShelfItemKind::Album, "MPREa", "Innuendo", "1991"),
            item(ShelfItemKind::Album, "MPREb", "Jazz", "1978"),
        ]),
    });
    let size = iced::Size::new(1280.0, 900.0);
    let mut sim = iced_test::Simulator::with_size(iced::Settings::default(), size, s.view());
    assert!(sim.find("Jazz").is_ok());
    assert!(sim.find("Innuendo").is_ok());
}

#[test]
fn ctl_nav_reaches_a_shelf_more() {
    assert_eq!(
        control::route_word(&Route::ArtistShelf {
            channel: "UCq".into(),
            shelf: 2
        }),
        "artist-more:UCq:2"
    );
}
