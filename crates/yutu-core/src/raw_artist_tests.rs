//! Artist page shelves: fixture parse (trimmed real Queen response,
//! 2 cards per shelf) plus an ignored live check.

use crate::models::{ShelfItemKind, ShelfMore};
use crate::raw_artist::{parse_artist_grid_json, parse_artist_page_json, year_of};

const PAGE: &str = include_str!("../fixtures/artist_page.json");
const GRID: &str = include_str!("../fixtures/artist_grid.json");

#[test]
fn page_keeps_every_openable_shelf_in_server_order() {
    let page = parse_artist_page_json(PAGE);
    assert_eq!(page.name, "Queen");
    assert!(page.thumbnail_url.is_some());
    let titles: Vec<&str> = page.shelves.iter().map(|s| s.title.as_str()).collect();
    // Top songs (list shelf) has its own section; Latest episodes and
    // Podcasts hold nothing the app can open, so they drop out.
    assert_eq!(
        titles,
        [
            "Albums",
            "Singles & EPs",
            "Videos",
            "Live performances",
            "Featured on",
            "Playlists by Queen",
            "Fans might also like",
        ]
    );
}

#[test]
fn cards_map_kind_id_subtitle_and_year() {
    let page = parse_artist_page_json(PAGE);
    let shelf = |t: &str| page.shelves.iter().find(|s| s.title == t).unwrap();

    let album = &shelf("Albums").items[0];
    assert_eq!(album.kind, ShelfItemKind::Album);
    assert!(album.id.starts_with("MPRE"));
    assert_eq!(album.year, Some(2026));
    assert!(album.thumbnail_url.is_some());

    let single = &shelf("Singles & EPs").items[0];
    assert_eq!(single.subtitle, "EP \u{b7} 2026");
    assert_eq!(single.year, Some(2026));

    let video = &shelf("Videos").items[0];
    assert_eq!(video.kind, ShelfItemKind::Video);
    assert_eq!(video.id, "eyGWZs5ZME0");
    assert_eq!(video.year, None);

    // Playlist ids lose the `VL` browse prefix.
    let featured = &shelf("Featured on").items[0];
    assert_eq!(featured.kind, ShelfItemKind::Playlist);
    assert!(featured.id.starts_with("RDCLAK"), "{}", featured.id);

    let fan = &shelf("Fans might also like").items[0];
    assert_eq!(fan.kind, ShelfItemKind::Artist);
    assert!(fan.id.starts_with("UC"));
}

#[test]
fn more_targets_only_where_they_open_anonymously() {
    let page = parse_artist_page_json(PAGE);
    let more = |t: &str| {
        page.shelves
            .iter()
            .find(|s| s.title == t)
            .unwrap()
            .more
            .clone()
    };
    match more("Albums") {
        Some(ShelfMore::Grid { browse_id, params }) => {
            assert!(browse_id.starts_with("MPAD"));
            assert!(!params.contains("%3D"), "params decoded: {params}");
        }
        other => panic!("albums more: {other:?}"),
    }
    assert!(matches!(more("Videos"), Some(ShelfMore::Playlist(id)) if id.starts_with("OLAK")));
    // Channel tabs need a session; shelves without a button stay None.
    assert_eq!(more("Live performances"), None);
    assert_eq!(more("Playlists by Queen"), None);
    assert_eq!(more("Featured on"), None);
}

#[test]
fn grid_parses_discography_cards() {
    let items = parse_artist_grid_json(GRID);
    assert_eq!(items.len(), 3);
    assert!(items.iter().all(|i| i.kind == ShelfItemKind::Album));
    assert_eq!(items[0].title, "Queen Budapest");
}

#[test]
fn junk_input_is_empty_never_fatal() {
    assert!(parse_artist_page_json("{}").shelves.is_empty());
    assert!(parse_artist_page_json("not json").shelves.is_empty());
    assert!(parse_artist_grid_json("{\"contents\":1}").is_empty());
}

#[test]
fn year_reads_last_plausible_four_digits() {
    assert_eq!(year_of("EP \u{b7} 2026"), Some(2026));
    assert_eq!(year_of("2025"), Some(2025));
    assert_eq!(year_of("1.2M views"), None);
    assert_eq!(year_of("Single \u{b7} 9999"), None);
}

/// Live: Queen's page has the full discography behind Albums "More".
#[tokio::test]
#[ignore = "network"]
async fn live_queen_page_and_discography() {
    let page = crate::raw_artist::raw_artist_page("UCiMhD4jzUqG-IgPzUmmytRQ", "en")
        .await
        .expect("page");
    assert!(page.shelves.len() >= 4, "{:?}", page.shelves.len());
    let albums = page.shelves.iter().find_map(|s| match &s.more {
        Some(ShelfMore::Grid { browse_id, params }) => Some((browse_id.clone(), params.clone())),
        _ => None,
    });
    let (browse, params) = albums.expect("discography more");
    let grid = crate::raw_artist::raw_artist_grid(&browse, &params, "en")
        .await
        .expect("grid");
    assert!(grid.len() > 10, "{}", grid.len());
}
