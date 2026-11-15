//! playlist_sources tests: link detection, page/API/CSV parsing on
//! fabricated payloads shaped like the live ones (probed 2026-11).

use super::*;

fn spotify(id: &str) -> Option<ImportSource> {
    Some(ImportSource::Spotify(LinkId::Id(id.into())))
}

#[test]
fn links_are_routed_to_their_platform() {
    let sp = "37i9dQZF1DXcBWIGoYBM5M";
    assert_eq!(
        detect_source(&format!("https://open.spotify.com/playlist/{sp}?si=abc")),
        spotify(sp)
    );
    assert_eq!(
        detect_source(&format!("https://open.spotify.com/intl-es/playlist/{sp}")),
        spotify(sp)
    );
    assert_eq!(
        detect_source(&format!("spotify:playlist:{sp}")),
        spotify(sp)
    );
    assert_eq!(
        detect_source("https://spotify.link/AbCdEf"),
        Some(ImportSource::Spotify(LinkId::Short(
            "https://spotify.link/AbCdEf".into()
        )))
    );
    assert_eq!(
        detect_source("https://www.deezer.com/en/playlist/3155776842?utm_source=x"),
        Some(ImportSource::Deezer(LinkId::Id("3155776842".into())))
    );
    assert_eq!(
        detect_source("link.deezer.com/s/30abc"),
        Some(ImportSource::Deezer(LinkId::Short(
            "https://link.deezer.com/s/30abc".into()
        )))
    );
    let apple =
        "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb";
    assert_eq!(
        detect_source(apple),
        Some(ImportSource::Apple(apple.into()))
    );
    assert_eq!(
        detect_source("https://music.youtube.com/playlist?list=PLabcdefghijkl"),
        Some(ImportSource::YouTube("PLabcdefghijkl".into()))
    );
    assert_eq!(
        detect_source("/home/me/Downloads/Road Trip.csv"),
        Some(ImportSource::File(
            "/home/me/Downloads/Road Trip.csv".into()
        ))
    );
}

#[test]
fn non_playlist_links_are_rejected() {
    for bad in [
        "",
        "https://open.spotify.com/track/3h5T5JypYU7huFiVYhv1dr",
        "https://open.spotify.com/album/1A2B3C",
        "https://www.deezer.com/en/album/12345",
        "https://www.deezer.com/en/playlist/abc",
        "https://music.apple.com/us/album/housewife/6822497770",
        "https://example.com/playlist?list=",
        "hello there",
    ] {
        assert_eq!(detect_source(bad), None, "{bad:?}");
    }
}

fn spotify_page(rows: usize) -> String {
    let row = |i: usize| {
        format!(
            r#"{{"uri":"spotify:track:{i}","title":"Song {i}","subtitle":"KAROL G, Judeline","duration":225834,"entityType":"track"}}"#
        )
    };
    let list: Vec<String> = (0..rows).map(row).collect();
    format!(
        r#"<html><script id="__NEXT_DATA__" type="application/json">{{"props":{{"pageProps":{{"state":{{"data":{{"entity":{{"type":"playlist","name":"Today’s Top Hits","trackList":[{}]}}}}}}}}}}}}</script></html>"#,
        list.join(",")
    )
}

#[test]
fn spotify_embed_rows_parse_and_full_pages_are_capped() {
    let pl = parse_spotify_embed(&spotify_page(2)).unwrap();
    assert_eq!(pl.title, "Today’s Top Hits");
    assert_eq!(pl.tracks.len(), 2);
    assert_eq!(pl.tracks[0].title, "Song 0");
    // No-break spaces in artist lists become plain spaces.
    assert_eq!(pl.tracks[0].artist, "KAROL G, Judeline");
    assert_eq!(pl.tracks[0].duration_secs, 226);
    assert_eq!(pl.limit, None);
    let full = parse_spotify_embed(&spotify_page(SPOTIFY_PAGE_CAP)).unwrap();
    assert_eq!(full.limit, Some(ReadLimit::SpotifyCap(SPOTIFY_PAGE_CAP)));
    assert!(parse_spotify_embed("<html>nothing</html>").is_err());
}

fn apple_page(rows: usize, total: usize) -> String {
    let row = |i: usize| {
        format!(
            r#"{{"id":"track-lockup - pl.x - {i}","title":"Track {i}","artistName":"Tate McRae","duration":208810,"tertiaryLinks":[{{"title":"Housewife - Single"}}]}}"#
        )
    };
    let list: Vec<String> = (0..rows).map(row).collect();
    format!(
        r#"<script type="application/json" id="serialized-server-data">{{"data":[{{"data":{{"sections":[{{"itemKind":"containerDetailHeaderLockup","items":[{{"title":"Today’s Hits","trackCount":{total}}}]}},{{"itemKind":"trackLockup","items":[{}]}},{{"itemKind":"bubbleLockup","items":[{{"title":"Not a song"}}]}}]}}}}]}}</script>"#,
        list.join(",")
    )
}

#[test]
fn apple_page_rows_parse_and_short_reads_are_flagged() {
    let pl = parse_apple_page(&apple_page(3, 3)).unwrap();
    assert_eq!(pl.title, "Today’s Hits");
    assert_eq!(pl.tracks.len(), 3);
    assert_eq!(pl.tracks[1].title, "Track 1");
    assert_eq!(pl.tracks[1].artist, "Tate McRae");
    assert_eq!(pl.tracks[1].album.as_deref(), Some("Housewife - Single"));
    assert_eq!(pl.tracks[1].duration_secs, 209);
    assert_eq!(pl.limit, None);
    let short = parse_apple_page(&apple_page(3, 250)).unwrap();
    assert_eq!(short.limit, Some(ReadLimit::Partial { got: 3, total: 250 }));
    assert!(parse_apple_page("<html></html>").is_err());
}

#[test]
fn deezer_api_pages_parse_and_errors_surface() {
    let head = r#"{"id":3155776842,"title":"Top Worldwide","nb_tracks":100,"tracks":{"data":[]}}"#;
    assert_eq!(
        parse_deezer_header(head).unwrap(),
        ("Top Worldwide".to_string(), 100)
    );
    let page = r#"{"data":[{"title":"Boston","duration":170,"artist":{"name":"STELLA LEFTY"},"album":{"title":"Boston"}}],"total":100,"next":"https://api.deezer.com/playlist/3155776842/tracks?limit=100&index=100"}"#;
    let (rows, next) = parse_deezer_tracks(page).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].artist, "STELLA LEFTY");
    assert_eq!(rows[0].duration_secs, 170);
    assert!(next.unwrap().contains("index=100"));
    // A `next` pointing off Deezer's API is never followed.
    let evil = r#"{"data":[],"next":"https://evil.example/x"}"#;
    assert_eq!(parse_deezer_tracks(evil).unwrap().1, None);
    let private = r#"{"error":{"type":"DataException","message":"no data","code":800}}"#;
    assert!(parse_deezer_header(private)
        .unwrap_err()
        .contains("no data"));
}

#[test]
fn exportify_csv_maps_columns_and_quoted_fields() {
    let csv = "\u{feff}\"Track URI\",\"Track Name\",\"Album Name\",\"Artist Name(s)\",\"Duration (ms)\"\n\
        \"spotify:track:1\",\"Hello, World\",\"Greetings\",\"Ana, Bo\",\"201000\"\n\
        \"spotify:track:2\",\"Say \"\"Hi\"\"\",\"\",\"Cy\",\"59500\"\n\
        \n";
    let pl = parse_csv(csv, "My Export").unwrap();
    assert_eq!(pl.title, "My Export");
    assert_eq!(pl.tracks.len(), 2);
    assert_eq!(pl.tracks[0].title, "Hello, World");
    assert_eq!(pl.tracks[0].artist, "Ana, Bo");
    assert_eq!(pl.tracks[0].album.as_deref(), Some("Greetings"));
    assert_eq!(pl.tracks[0].duration_secs, 201);
    assert_eq!(pl.tracks[1].title, "Say \"Hi\"");
    assert_eq!(pl.tracks[1].album, None);
    assert_eq!(pl.tracks[1].duration_secs, 60);
}

#[test]
fn tunemymusic_and_semicolon_exports_use_the_playlist_name() {
    let csv = "Track name;Artist name;Album;Playlist name;Type;ISRC\n\
        Yellow;Coldplay;Parachutes;Road Trip;Playlist;GBAYE0000351\n\
        Clocks;Coldplay;A Rush of Blood;Road Trip;Playlist;\n";
    let pl = parse_csv(csv, "fallback").unwrap();
    assert_eq!(pl.title, "Road Trip");
    assert_eq!(pl.tracks.len(), 2);
    assert_eq!(pl.tracks[1].title, "Clocks");
    assert_eq!(pl.tracks[1].artist, "Coldplay");
}

#[test]
fn plain_text_lists_read_artist_dash_title() {
    let txt = "Coldplay - Yellow\n\nDaft Punk - One More Time\nJust A Title\n";
    let pl = parse_csv(txt, "list").unwrap();
    assert_eq!(pl.tracks.len(), 3);
    assert_eq!(pl.tracks[0].artist, "Coldplay");
    assert_eq!(pl.tracks[0].title, "Yellow");
    assert_eq!(pl.tracks[2].artist, "");
    assert_eq!(pl.tracks[2].title, "Just A Title");
    assert!(parse_csv("\n\n", "x").is_err());
    assert!(parse_csv("Title,Artist\n", "x").is_err());
}

#[test]
fn durations_read_clock_seconds_and_ms() {
    assert_eq!(parse_duration("3:20", false), 200);
    assert_eq!(parse_duration("1:02:03", false), 3723);
    assert_eq!(parse_duration("200", false), 200);
    assert_eq!(parse_duration("200000", false), 200);
    assert_eq!(parse_duration("1500", true), 2);
    assert_eq!(parse_duration("n/a", false), 0);
}

#[test]
fn export_files_read_from_disk() {
    let dir = std::env::temp_dir().join(format!("yutu-csv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("road_trip-mix.csv");
    std::fs::write(&path, "Title,Artist\nYellow,Coldplay\n").unwrap();
    let pl = read_file(&path).unwrap();
    assert_eq!(pl.title, "road trip mix");
    assert_eq!(pl.tracks[0].artist, "Coldplay");
    assert!(read_file(&dir.join("missing.csv")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Live: read one public playlist per platform and match a sample on
/// YouTube Music. Needs network:
/// `cargo test -p yutu-core --features innertube -- --ignored live_foreign --nocapture`.
#[cfg(feature = "innertube")]
#[test]
#[ignore]
fn live_foreign_sources_read_and_match() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let links = [
        "https://www.deezer.com/en/playlist/3155776842",
        "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
        "https://music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb",
    ];
    for link in links {
        let source = detect_source(link).expect("link detected");
        let pl = rt.block_on(fetch_foreign(&source)).expect("playlist read");
        eprintln!(
            "{}: {:?} rows={} limit={:?}",
            source.platform(),
            pl.title,
            pl.tracks.len(),
            pl.limit
        );
        assert!(!pl.title.is_empty());
        assert!(pl.tracks.len() >= 10);
        let sample: Vec<ForeignTrack> = pl.tracks.into_iter().take(8).collect();
        let (found, missing) = rt.block_on(crate::song_match::resolve_foreign(
            sample,
            &crate::raw_search::SearchAuth::anonymous(),
            |_, _| {},
        ));
        for t in &found {
            eprintln!("  ok   {} - {} (video={})", t.artist, t.title, t.is_video);
        }
        for m in &missing {
            eprintln!("  miss {}", m.label());
        }
        assert!(found.len() >= 6, "too many misses: {missing:?}");
    }
}

fn track(
    id: &str,
    title: &str,
    artist: &str,
    album: Option<&str>,
    secs: u32,
) -> crate::models::Track {
    crate::models::Track {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        album: album.map(str::to_string),
        duration_secs: secs,
        thumbnail_url: None,
        thumbnail_large_url: None,
        plays: None,
        is_video: false,
    }
}

#[test]
fn exported_csv_reads_back_as_the_same_playlist() {
    let tracks = vec![
        track("a1", "Hello, \"World\"", "Ana, Bo", Some("Greetings"), 201),
        track("a2", "Sedúceme", "Viti Ruiz", None, 0),
        track("a3", "=SUM(A1)", "-Hacker", Some("@home"), 60),
    ];
    let csv = to_csv("Salsita", &tracks);
    assert!(csv.starts_with("\"Track Name\",\"Artist Name(s)\""));
    assert!(csv.contains("https://music.youtube.com/watch?v=a1"));
    // Formula-looking cells are defused for spreadsheets...
    assert!(csv.contains("\"'=SUM(A1)\""));
    assert!(csv.contains("\"'-Hacker\""));
    // ...and the importer restores them.
    let back = parse_csv(&csv, "fallback").unwrap();
    assert_eq!(back.title, "Salsita");
    assert_eq!(back.tracks.len(), 3);
    assert_eq!(back.tracks[0].title, "Hello, \"World\"");
    assert_eq!(back.tracks[0].artist, "Ana, Bo");
    assert_eq!(back.tracks[0].album.as_deref(), Some("Greetings"));
    assert_eq!(back.tracks[0].duration_secs, 201);
    assert_eq!(back.tracks[1].duration_secs, 0);
    assert_eq!(back.tracks[1].album, None);
    assert_eq!(back.tracks[2].title, "=SUM(A1)");
    assert_eq!(back.tracks[2].artist, "-Hacker");
    assert_eq!(back.tracks[2].album.as_deref(), Some("@home"));
    // An empty playlist still exports a header.
    assert_eq!(to_csv("Empty", &[]).lines().count(), 1);
}

#[test]
fn export_file_names_are_safe() {
    assert_eq!(export_file_name("Salsita"), "Salsita.csv");
    assert_eq!(export_file_name("AC/DC: Best?"), "AC_DC_ Best_.csv");
    assert_eq!(export_file_name("  ..  "), "playlist.csv");
    assert_eq!(export_file_name(""), "playlist.csv");
}
