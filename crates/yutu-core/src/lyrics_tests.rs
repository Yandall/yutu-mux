//! lyrics_tests: moved from lyrics_providers.rs (no behavior change).

#[allow(unused_imports)]
use super::lyrics_providers::*;
#[allow(unused_imports)]
use super::*;
#[test]
fn parse_lrc_keeps_stanza_breaks_as_markers() {
    let lines = parse_lrc("[00:01.00]one\n[00:05.00]two\n\n[00:09.00]three\n");
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[2].text, "");
    assert_eq!(lines[2].start_ms, lines[3].start_ms);
    // Markers never steal the highlight.
    assert_eq!(line_at(&lines, 9_000), Some(3));
    assert_eq!(line_at(&lines, 5_000), Some(1));
}

fn tagged(start_ms: u64, text: &str) -> LyricLine {
    LyricLine {
        start_ms,
        text: text.to_owned(),
    }
}

#[test]
fn lrc_single_line_basic() {
    let lines = parse_lrc("[00:12.34]Hello\n");
    assert_eq!(lines, vec![tagged(12_340, "Hello")]);
}

#[test]
fn lrc_multi_tag_line_emits_one_per_timestamp() {
    let lines = parse_lrc("[00:01.00][00:02.00]Hello");
    assert_eq!(lines, vec![tagged(1_000, "Hello"), tagged(2_000, "Hello")]);
}

#[test]
fn lrc_metadata_tags_skipped() {
    let input =
        "[ar:Artist]\n[ti:Title]\n[al:Album]\n[length: 03:45]\n[by:someone]\n[00:10.00]Real line\n";
    let lines = parse_lrc(input);
    assert_eq!(lines, vec![tagged(10_000, "Real line")]);
}

#[test]
fn lrc_two_vs_three_digit_fractions() {
    let lines = parse_lrc("[00:01.23]two\n[00:02.456]three\n");
    assert_eq!(lines, vec![tagged(1_230, "two"), tagged(2_456, "three")]);
}

#[test]
fn lrc_bare_mm_ss_without_fraction() {
    let lines = parse_lrc("[01:23]No fraction\n");
    assert_eq!(lines, vec![tagged(83_000, "No fraction")]);
}

#[test]
fn lrc_unsorted_input_gets_sorted() {
    let lines = parse_lrc("[00:30.00]third\n[00:10.00]first\n[00:20.00]second\n");
    assert_eq!(
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );
    assert!(lines.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
}

#[test]
fn lrc_garbage_lines_skipped() {
    let input = "no tags here\n[bad]nope\n[00:xx.yy]bad time\n[00:10.0]one digit frac\n[00:70.00]bad seconds\n[00:10.00]\n[00:11.00]   \n[00:12.00]Good\n";
    let lines = parse_lrc(input);
    assert_eq!(lines, vec![tagged(12_000, "Good")]);
}

#[test]
fn lrc_empty_input_gives_empty_vec() {
    assert!(parse_lrc("").is_empty());
    assert!(parse_lrc("\n   \n").is_empty());
}

#[test]
fn lrc_colon_fraction_separator_accepted() {
    let lines = parse_lrc("[00:01:23]colon frac\n");
    assert_eq!(lines, vec![tagged(1_230, "colon frac")]);
}

#[test]
fn line_at_before_first_is_none() {
    let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
    assert_eq!(line_at(&lines, 0), None);
    assert_eq!(line_at(&lines, 999), None);
}

#[test]
fn line_at_exact_hit() {
    let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
    assert_eq!(line_at(&lines, 1_000), Some(0));
    assert_eq!(line_at(&lines, 2_000), Some(1));
}

#[test]
fn line_at_between_lines() {
    let lines = vec![tagged(1_000, "a"), tagged(2_000, "b"), tagged(3_000, "c")];
    assert_eq!(line_at(&lines, 1_500), Some(0));
    assert_eq!(line_at(&lines, 2_999), Some(1));
}

#[test]
fn line_at_past_end_gives_last() {
    let lines = vec![tagged(1_000, "a"), tagged(2_000, "b")];
    assert_eq!(line_at(&lines, 2_001), Some(1));
    assert_eq!(line_at(&lines, u64::MAX), Some(1));
}

#[test]
fn line_at_empty_slice_is_none() {
    assert_eq!(line_at(&[], 10_000), None);
}

#[test]
fn lyrics_result_offline_shape() {
    let r = LyricsResult {
        plain: "line one\nline two".to_owned(),
        synced: vec![tagged(1_000, "line one"), tagged(2_000, "line two")],
        instrumental: false,
    };
    assert!(!r.plain.is_empty());
    assert_eq!(r.synced.len(), 2);
    assert!(!r.instrumental);

    let inst = LyricsResult {
        plain: "[Instrumental]".to_owned(),
        synced: Vec::new(),
        instrumental: true,
    };
    assert_eq!(inst.plain, "[Instrumental]");
    assert!(inst.synced.is_empty());
    assert!(inst.instrumental);
}

#[cfg(feature = "lyrics")]
#[test]
fn lrclib_url_omits_empty_album() {
    let with_album = lrclib_url("The Beatles", "Yesterday", Some("Help!"), 125);
    assert!(with_album.contains("album_name=Help%21"));
    assert!(with_album.contains("artist_name=The%20Beatles"));
    let no_album = lrclib_url("A", "T", None, 200);
    assert!(!no_album.contains("album_name"));
    let empty_album = lrclib_url("A", "T", Some(""), 200);
    assert!(!empty_album.contains("album_name"));
}

#[test]
fn lyrics_provider_labels_and_cycle() {
    assert_eq!(LyricsProvider::Lrclib.label(), "LRCLIB");
    assert_eq!(LyricsProvider::YouTubeMusic.label(), "YouTube Music");
    assert_eq!(LyricsProvider::parse("lrclib"), LyricsProvider::Lrclib);
    assert_eq!(LyricsProvider::parse("YTM"), LyricsProvider::YouTubeMusic);
    assert_eq!(
        LyricsProvider::parse("youtube music"),
        LyricsProvider::YouTubeMusic
    );
    assert_eq!(LyricsProvider::parse("bogus"), LyricsProvider::Lrclib);
    assert_eq!(LyricsProvider::Lrclib.next(), LyricsProvider::YouTubeMusic);
    assert_eq!(LyricsProvider::YouTubeMusic.next(), LyricsProvider::Lrclib);
    assert_eq!(LyricsProvider::Lrclib.prev(), LyricsProvider::YouTubeMusic);
    assert_eq!(LyricsProvider::Lrclib.as_pref(), "lrclib");
    assert_eq!(LyricsProvider::YouTubeMusic.as_pref(), "youtubemusic");
}

#[test]
fn normalize_strips_versions_and_credits() {
    assert_eq!(
        normalize_for_match("Bohemian Rhapsody (Remastered 2011)"),
        "bohemian rhapsody"
    );
    assert_eq!(
        normalize_for_match("Yesterday - Remastered 2009"),
        "yesterday"
    );
    assert_eq!(
        normalize_for_match("Blinding Lights feat. Rosalia"),
        "blinding lights"
    );
    assert_eq!(
        normalize_for_match("Dance With Somebody"),
        "dance with somebody"
    );
    assert_eq!(normalize_for_match("  Hello,  World! "), "hello world");
    assert_eq!(normalize_for_match(""), "");
}

#[test]
fn score_prefers_exact_then_duration() {
    let query = MatchQuery {
        artist: "The Beatles",
        title: "Yesterday",
        album: Some("Help!"),
        duration: 125,
    };
    let score = |track_name: &str, artist_name: &str, album_name: &str, duration: u32| {
        score_candidate(
            &MatchCandidate {
                track_name,
                artist_name,
                album_name,
                duration,
            },
            &query,
        )
    };
    let exact = score("Yesterday", "The Beatles", "Help!", 125);
    let remaster = score("Yesterday - Remastered 2009", "The Beatles", "Help!", 125);
    let wrong_artist = score("Yesterday", "Some Cover Band", "Covers", 125);
    let wrong_song = score("Yellow Submarine", "The Beatles", "Help!", 125);
    assert!(exact >= remaster);
    assert!(remaster > wrong_artist);
    assert!(wrong_artist > wrong_song);
    // Duration distance still ranks: same title, closer duration wins.
    let near = score("Yesterday", "The Beatles", "Help!", 126);
    let far = score("Yesterday", "The Beatles", "Help!", 200);
    assert!(near > far);
}

#[test]
fn ytm_lines_join_to_plain() {
    let ok = ytm_lyrics_to_result(vec!["  hello ".into(), "".into(), "world".into()])
        .expect("lines join");
    assert_eq!(ok.plain, "hello\nworld");
    assert!(ok.synced.is_empty());
    assert!(!ok.instrumental);
    assert!(ytm_lyrics_to_result(vec!["  ".into()]).is_err());
}

#[test]
fn ytm_timed_lines_keep_sync() {
    let timed = vec![
        tagged(5_000, "second"),
        tagged(1_000, "first"),
        tagged(2_000, "  "),
    ];
    let ok = ytm_timed_to_result(timed).expect("timed join");
    assert_eq!(ok.plain, "first\nsecond");
    assert_eq!(ok.synced.len(), 2);
    assert_eq!(ok.synced[0].start_ms, 1_000);
    assert!(!ok.instrumental);
    assert!(ytm_timed_to_result(vec![]).is_err());
}

#[cfg(feature = "lyrics")]
#[tokio::test]
#[ignore]
async fn live_lrclib_beatles_yesterday() {
    let res = fetch_lrclib("The Beatles", "Yesterday", Some("Help!"), 125).await;
    match res {
        Ok(lyrics) => assert!(
            !lyrics.plain.trim().is_empty(),
            "expected non-empty plain lyrics for LRCLIB lookup"
        ),
        Err(e) => panic!("live LRCLIB fetch failed: {e}"),
    }
}
