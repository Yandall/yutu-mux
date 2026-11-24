//! Japanese lyrics romanization (romaji under each line).
//!
//! Kanji need context to read (`君の名は` is "kimi no na wa"), so lines
//! go through a morphological analyzer (lindera, MIT) over the UniDic
//! dictionary (BSD-3-Clause), which also knows archaic and poetic words
//! common in lyrics (`傅き` "kashizuki", `昏き` "kuraki"). The dictionary
//! is not embedded (~190 MB): the first time the user turns romaji on,
//! the app downloads lindera's prebuilt UniDic once (~46 MB zip, pinned
//! SHA-256) and unpacks it into the data dir.
//!
//! Rules:
//! - Only lyrics with kana anywhere count as Japanese ([`is_japanese`]):
//!   kanji alone is ambiguous (Chinese lyrics) and stays untouched.
//! - Lines without Japanese characters get no romaji (English lines in
//!   a J-pop song already read fine).
//! - Readings are the words as pronounced; particles as lyric sites
//!   write them (は "wa", へ "e", を "wo"); long vowels spelled out as
//!   kana writes them (`toukyou`, `unmei`), not as ー marks.
//! - Hepburn spelling, `n'` before a vowel or `y`, doubled consonants
//!   for small `っ`.
//! - Words are spaced like lyric sites: inflections glue to their verb
//!   (`matteita`, `kanashisugi`), auxiliary verbs after て stand apart
//!   (`toozakatte yuku`), copulas stand apart (`yokatta deshou`) except
//!   な after an adjectival noun (`atatakana`).

/// Pure conversion helpers (no dictionary needed, always compiled so
/// tests and offline builds cover them).
mod kana {
    /// Katakana for one hiragana char (other chars unchanged).
    pub(super) fn to_katakana(c: char) -> char {
        match c {
            '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            _ => c,
        }
    }

    /// Hepburn romaji for one katakana mora (`None` when not a kana).
    #[rustfmt::skip]
    fn mora(s: &str) -> Option<&'static str> {
        Some(match s {
            "ア" => "a", "イ" => "i", "ウ" => "u", "エ" => "e", "オ" => "o",
            "カ" => "ka", "キ" => "ki", "ク" => "ku", "ケ" => "ke", "コ" => "ko",
            "サ" => "sa", "シ" => "shi", "ス" => "su", "セ" => "se", "ソ" => "so",
            "タ" => "ta", "チ" => "chi", "ツ" => "tsu", "テ" => "te", "ト" => "to",
            "ナ" => "na", "ニ" => "ni", "ヌ" => "nu", "ネ" => "ne", "ノ" => "no",
            "ハ" => "ha", "ヒ" => "hi", "フ" => "fu", "ヘ" => "he", "ホ" => "ho",
            "マ" => "ma", "ミ" => "mi", "ム" => "mu", "メ" => "me", "モ" => "mo",
            "ヤ" => "ya", "ユ" => "yu", "ヨ" => "yo",
            "ラ" => "ra", "リ" => "ri", "ル" => "ru", "レ" => "re", "ロ" => "ro",
            "ワ" => "wa", "ヰ" => "i", "ヱ" => "e", "ヲ" => "o", "ン" => "n",
            "ガ" => "ga", "ギ" => "gi", "グ" => "gu", "ゲ" => "ge", "ゴ" => "go",
            "ザ" => "za", "ジ" => "ji", "ズ" => "zu", "ゼ" => "ze", "ゾ" => "zo",
            "ダ" => "da", "ヂ" => "ji", "ヅ" => "zu", "デ" => "de", "ド" => "do",
            "バ" => "ba", "ビ" => "bi", "ブ" => "bu", "ベ" => "be", "ボ" => "bo",
            "パ" => "pa", "ピ" => "pi", "プ" => "pu", "ペ" => "pe", "ポ" => "po",
            "ヴ" => "vu",
            "ァ" => "a", "ィ" => "i", "ゥ" => "u", "ェ" => "e", "ォ" => "o",
            "ャ" => "ya", "ュ" => "yu", "ョ" => "yo", "ヮ" => "wa",
            "キャ" => "kya", "キュ" => "kyu", "キョ" => "kyo",
            "シャ" => "sha", "シュ" => "shu", "ショ" => "sho", "シェ" => "she",
            "チャ" => "cha", "チュ" => "chu", "チョ" => "cho", "チェ" => "che",
            "ニャ" => "nya", "ニュ" => "nyu", "ニョ" => "nyo",
            "ヒャ" => "hya", "ヒュ" => "hyu", "ヒョ" => "hyo",
            "ミャ" => "mya", "ミュ" => "myu", "ミョ" => "myo",
            "リャ" => "rya", "リュ" => "ryu", "リョ" => "ryo",
            "ギャ" => "gya", "ギュ" => "gyu", "ギョ" => "gyo",
            "ジャ" => "ja", "ジュ" => "ju", "ジョ" => "jo", "ジェ" => "je",
            "ヂャ" => "ja", "ヂュ" => "ju", "ヂョ" => "jo",
            "ビャ" => "bya", "ビュ" => "byu", "ビョ" => "byo",
            "ピャ" => "pya", "ピュ" => "pyu", "ピョ" => "pyo",
            "ファ" => "fa", "フィ" => "fi", "フェ" => "fe", "フォ" => "fo",
            "ティ" => "ti", "ディ" => "di", "トゥ" => "tu", "ドゥ" => "du",
            "ウィ" => "wi", "ウェ" => "we", "ウォ" => "wo",
            "ヴァ" => "va", "ヴィ" => "vi", "ヴェ" => "ve", "ヴォ" => "vo",
            "ツァ" => "tsa", "ツィ" => "tsi", "ツェ" => "tse", "ツォ" => "tso",
            "イェ" => "ye",
            _ => return None,
        })
    }

    /// Romaji for a kana string (hiragana or katakana). Non-kana chars
    /// pass through unchanged, so mixed tokens never lose text.
    pub fn to_romaji(input: &str) -> String {
        let chars: Vec<char> = input.chars().map(to_katakana).collect();
        let mut out = String::with_capacity(input.len());
        let mut geminate = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == 'ッ' {
                geminate = true;
                i += 1;
                continue;
            }
            if c == 'ー' {
                // Long vowel mark: repeat the previous vowel.
                if let Some(v) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                    out.push(v);
                }
                i += 1;
                continue;
            }
            // Two-char digraphs first (キャ), then one char.
            let pair: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let (roma, used) = match (chars.len() > i + 1).then(|| mora(&pair)).flatten() {
                Some(r) => (r, 2),
                None => match mora(&c.to_string()) {
                    Some(r) => (r, 1),
                    None => {
                        if geminate {
                            geminate = false;
                        }
                        out.push(c);
                        i += 1;
                        continue;
                    }
                },
            };
            if c == 'ン' {
                // n' before a vowel or y keeps "kan'i" apart from "kani".
                let next = chars.get(i + 1).map(|n| n.to_string());
                let next_roma = next.as_deref().and_then(mora).unwrap_or("");
                out.push('n');
                if next_roma.starts_with(['a', 'e', 'i', 'o', 'u', 'y']) {
                    out.push('\'');
                }
                i += 1;
                continue;
            }
            if geminate {
                geminate = false;
                // っち is "tchi" in Hepburn; vowels cannot double.
                match roma.as_bytes()[0] {
                    b'c' => out.push('t'),
                    b'a' | b'e' | b'i' | b'o' | b'u' | b'n' => {}
                    b => out.push(b as char),
                }
            }
            out.push_str(roma);
            i += used;
        }
        out
    }

    /// True for hiragana or katakana (the prolonged mark included).
    pub(crate) fn is_kana(c: char) -> bool {
        matches!(c, '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | 'ー')
    }

    /// True for kana or CJK ideographs (kanji, `々`).
    pub(crate) fn is_japanese_char(c: char) -> bool {
        is_kana(c) || matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
    }
}

pub use kana::to_romaji as kana_to_romaji;

/// Lyrics count as Japanese when any kana appears (kanji alone could be
/// Chinese, which this romanizer would misread).
pub fn is_japanese(text: &str) -> bool {
    text.chars().any(kana::is_kana)
}

/// True for hiragana or katakana.
fn romaji_kana_char(c: char) -> bool {
    kana::is_kana(c)
}

/// True when `line` has any Japanese character worth a romaji line.
pub fn line_needs_romaji(line: &str) -> bool {
    line.chars().any(kana::is_japanese_char)
}

/// One analyzed token: surface text, part of speech (UniDic tags),
/// lemma, and reading as pronounced (katakana, `None` for unknown
/// words). Kept separate from lindera so the joining rules are
/// unit-tested without a dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Morpheme {
    pub surface: String,
    pub pos: String,
    pub pos_sub: String,
    /// Dictionary lemma (`だ` for `でしょ`, `居る` for `いた`).
    pub lemma: String,
    pub reading: Option<String>,
}

impl Morpheme {
    /// Word separator token (original whitespace).
    pub fn blank() -> Self {
        Self {
            surface: " ".into(),
            ..Self::default()
        }
    }
}

/// Join analyzed tokens into a readable romaji line (spacing rules in
/// the module docs).
pub fn join_morphemes(tokens: &[Morpheme]) -> String {
    // Words collect kana (and literal text) first and convert once, so
    // sound changes across glued tokens survive (`マッ` + `テ` is
    // "matte", not "mate").
    let mut words: Vec<String> = Vec::new();
    let mut spaced = true;
    // Set by a prefix or an opening bracket: the next token joins it.
    let mut glue_next = false;
    let mut prev: Option<&Morpheme> = None;
    for t in tokens {
        if t.surface.trim().is_empty() {
            // Original spacing separates words (English mixed in).
            spaced = true;
            glue_next = false;
            prev = None;
            continue;
        }
        let source = morpheme_source(t);
        if source.is_empty() {
            continue;
        }
        let punctuation =
            !line_needs_romaji(&t.surface) && !t.surface.chars().any(char::is_alphanumeric);
        let opening = punctuation && source.ends_with(['(', '[']);
        let prefix = t.pos == "接頭辞";
        if std::mem::take(&mut glue_next) {
            if let Some(word) = words.last_mut() {
                word.push_str(&source);
                glue_next = opening || prefix;
                spaced = false;
                prev = Some(t);
                continue;
            }
        }
        if opening || prefix {
            words.push(source);
            glue_next = true;
            spaced = false;
            prev = Some(t);
            continue;
        }
        // Latin words split into letters by the analyzer read whole.
        let latin = |m: &Morpheme| m.surface.chars().all(|c| c.is_ascii_alphanumeric());
        let glue =
            punctuation || prev.is_some_and(|p| latin(p) && latin(t)) || glues_to_previous(t, prev);
        match words.last_mut() {
            Some(word) if glue && !(spaced && !punctuation) => word.push_str(&source),
            _ => words.push(source),
        }
        spaced = false;
        prev = Some(t);
    }
    words
        .iter()
        .map(|w| kana::to_romaji(w))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// True when `t` belongs to the word before it (UniDic tags).
fn glues_to_previous(t: &Morpheme, prev: Option<&Morpheme>) -> bool {
    let after_te = prev.is_some_and(|p| p.pos == "助詞" && p.pos_sub == "接続助詞");
    let after_adjectival = prev.is_some_and(|p| p.pos == "形状詞");
    // A stem: an inflecting word, or a kanji word with kana okurigana
    // (`果てし`, which UniDic files as a noun before ない).
    let after_stem = prev.is_some_and(|p| {
        let kana_tail = p.surface.chars().last().is_some_and(romaji_kana_char);
        let has_kanji = p
            .surface
            .chars()
            .any(|c| line_needs_romaji(&c.to_string()) && !romaji_kana_char(c));
        matches!(p.pos.as_str(), "動詞" | "形容詞") || (kana_tail && has_kanji)
    });
    // Counters join their number ("ichido", "futari").
    if prev.is_some_and(|p| p.pos_sub == "数詞") && matches!(t.pos.as_str(), "接尾辞" | "名詞")
    {
        return true;
    }
    match t.pos.as_str() {
        // Copulas stand apart ("yokatta deshou"), except な closing an
        // adjectival noun ("atatakana"); other auxiliaries inflect.
        "助動詞" => match t.lemma.as_str() {
            "だ" | "です" => after_adjectival && t.surface == "な",
            _ => true,
        },
        // て, で, ば inflect ("shinjite", "miwataseba"); が, けど,
        // から stand apart ("miru ga ii"); contracted の joins
        // ("kowareten da").
        "助詞" => {
            (t.pos_sub == "接続助詞"
                && matches!(t.surface.as_str(), "て" | "で" | "ば" | "ちゃ" | "じゃ"))
                || t.surface == "ん"
        }
        "接尾辞" => true,
        // いる after て inflects ("matteita"); ゆく, くる, しまう stand
        // apart ("toozakatte yuku"); すぎる glues ("kanashisugi").
        "動詞" => (after_te && t.lemma == "居る") || t.lemma == "過ぎる",
        // そう after a verb stem ("kiesou").
        "形状詞" => t.pos_sub == "助動詞語幹" && t.surface == "そう",
        // ない after a verb or adjective stem ("hateshinai"); after a
        // noun it stands apart ("koto nai").
        "形容詞" => t.lemma == "無い" && after_stem,
        _ => false,
    }
}

/// Kana to convert for one token: particles as lyric sites write them,
/// readings as pronounced, kana surfaces for unknown words;
/// non-Japanese text as is (full-width punctuation to ASCII, quote
/// brackets dropped).
fn morpheme_source(t: &Morpheme) -> String {
    if t.pos == "助詞" {
        match t.surface.as_str() {
            "は" => return "ワ".into(),
            "へ" => return "エ".into(),
            "を" => return "ウォ".into(),
            _ => {}
        }
    }
    if !line_needs_romaji(&t.surface) {
        return t
            .surface
            .chars()
            .filter_map(|c| match c {
                '、' => Some(','),
                '。' => Some('.'),
                '！' => Some('!'),
                '？' => Some('?'),
                '（' => Some('('),
                '）' => Some(')'),
                '「' | '」' | '『' | '』' | '・' => None,
                '　' => Some(' '),
                c => Some(c),
            })
            .collect::<String>()
            .trim()
            .to_string();
    }
    // Unknown katakana runs keep their middle dots (`アイ・アイ`):
    // they separate words.
    t.reading
        .as_deref()
        .filter(|r| !r.is_empty() && *r != "*")
        .unwrap_or(&t.surface)
        .replace(['・', '　'], " ")
}

/// Reading as pronounced with long vowels spelled out: UniDic's
/// pronunciation marks them with ー (`ウンメー`), its lemma reading
/// spells them (`ウンメイ`). Where everything before a ー matches the
/// lemma reading (whole words, and the stem of inflected ones:
/// `トーザカッ` / `トオザカル`), the lemma's kana fills it; past the
/// stem an o-row ー reads ウ and an e-row ー reads イ (`イオー` "iou").
/// Loanwords keep their ー (both forms have it: `ゲーム` "geemu").
pub fn spell_long_vowels(pron: &str, lemma_reading: &str) -> String {
    let p: Vec<char> = pron.chars().collect();
    let l: Vec<char> = lemma_reading.chars().collect();
    let same = |a: char, b: char| a == b || a == 'ー' || b == 'ー';
    let mut out = String::with_capacity(pron.len());
    for (i, &c) in p.iter().enumerate() {
        if c != 'ー' {
            out.push(c);
            continue;
        }
        if i < l.len() && p[..i].iter().zip(&l[..i]).all(|(a, b)| same(*a, *b)) {
            out.push(l[i]);
            continue;
        }
        let vowel = out
            .chars()
            .last()
            .map(|v| kana::to_romaji(&v.to_string()))
            .and_then(|r| r.chars().last());
        out.push(match vowel {
            Some('o') => 'ウ',
            Some('e') => 'イ',
            _ => 'ー',
        });
    }
    out
}

/// Readings lyrics use where the dictionary prefers another: UniDic
/// reads 私 formally ("watakushi"), songs almost always sing "watashi".
pub fn common_reading(surface: &str) -> Option<&'static str> {
    match surface {
        "私" => Some("ワタシ"),
        _ => None,
    }
}

#[cfg(feature = "romaji")]
pub use analyzer::*;

#[cfg(feature = "romaji")]
mod analyzer {
    use super::{common_reading, join_morphemes, line_needs_romaji, spell_long_vowels, Morpheme};
    use std::path::{Path, PathBuf};

    /// lindera's prebuilt UniDic for this lindera version (the release
    /// asset its own `embed-unidic` build uses), ~46 MB zip.
    pub const UNIDIC_URL: &str =
        "https://github.com/lindera/lindera/releases/download/v7.0.0/lindera-unidic-7.0.0.zip";
    /// Pinned SHA-256 of [`UNIDIC_URL`]: anything else never unpacks.
    pub const UNIDIC_SHA256: &str =
        "e87a6364c78a0531be59db291eb85f125a694cd687a4ca70a5faae6652099c6e";
    /// Folder inside the zip holding the dictionary files.
    const ZIP_DIR: &str = "lindera-unidic";
    /// Unpacked dictionary folder, keyed by the lindera version (the
    /// format follows it). Bump it whenever the download changes; a
    /// finished setup deletes every other dictionary folder.
    const BUILT_DIR: &str = "unidic-lindera-7.0.0";

    /// Unpacked dictionary location under the romaji data dir.
    pub fn dictionary_dir(base: &Path) -> PathBuf {
        base.join(BUILT_DIR)
    }

    /// True when a finished setup sits in `base` (files unpack into a
    /// temp folder renamed last, so a crash never leaves a half
    /// dictionary that looks done).
    pub fn dictionary_ready(base: &Path) -> bool {
        dictionary_dir(base).join("metadata.json").is_file()
    }

    /// Loaded analyzer. Cheap to share (`Arc`), segmenting is sync and
    /// takes microseconds per lyric line.
    pub struct Romanizer {
        segmenter: lindera::segmenter::Segmenter,
    }

    impl std::fmt::Debug for Romanizer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Romanizer")
        }
    }

    impl Romanizer {
        /// Load the unpacked dictionary from `base` (see
        /// [`ensure_dictionary`]).
        pub fn load(base: &Path) -> Result<Self, String> {
            let dir = dictionary_dir(base);
            let path = dir.to_str().ok_or("romaji dictionary path is not UTF-8")?;
            let dict = lindera::dictionary::load_dictionary(path)
                .map_err(|e| format!("romaji dictionary load failed: {e}"))?;
            let segmenter =
                lindera::segmenter::Segmenter::new(lindera::mode::Mode::Normal, dict, None);
            Ok(Self { segmenter })
        }

        /// Romaji for one lyric line, `None` when it has nothing
        /// Japanese (or the analyzer fails on it).
        pub fn romanize_line(&self, line: &str) -> Option<String> {
            let line = line.trim();
            if !line_needs_romaji(line) {
                return None;
            }
            // Chunks between spaces analyze alone (a Latin word before
            // a verb can shift the lattice); a blank token keeps the
            // original spacing.
            let mut morphemes: Vec<Morpheme> = Vec::new();
            for chunk in line.split_whitespace() {
                if !morphemes.is_empty() {
                    morphemes.push(Morpheme::blank());
                }
                let mut tokens = self.segmenter.segment(chunk.into()).ok()?;
                morphemes.extend(tokens.iter_mut().map(|t| {
                    let surface = t.surface.to_string();
                    let details = t.details();
                    let field = |i: usize| {
                        details
                            .get(i)
                            .filter(|s| !s.is_empty() && **s != "*")
                            .map(|s| s.to_string())
                    };
                    // UniDic details: 0 pos, 1 pos sub, 6 lemma reading,
                    // 7 lemma, 9 pronunciation.
                    let reading = common_reading(&surface).map(str::to_string).or_else(|| {
                        field(9).map(|pron| spell_long_vowels(&pron, &field(6).unwrap_or_default()))
                    });
                    Morpheme {
                        pos: field(0).unwrap_or_default(),
                        pos_sub: field(1).unwrap_or_default(),
                        lemma: field(7).unwrap_or_default(),
                        reading,
                        surface,
                    }
                }));
            }
            let out = join_morphemes(&morphemes);
            (!out.is_empty()).then_some(out)
        }
    }

    /// Download, verify and unpack the dictionary into `base` unless it
    /// is already there. Blocking file work runs on a blocking thread.
    pub async fn ensure_dictionary(base: PathBuf) -> Result<(), String> {
        if dictionary_ready(&base) {
            return Ok(());
        }
        std::fs::create_dir_all(&base).map_err(|e| format!("romaji setup failed: {e}"))?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("yutu-mux/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .map_err(|e| format!("romaji download failed: {e}"))?;
        let bytes = client
            .get(UNIDIC_URL)
            .send()
            .await
            .map_err(|e| format!("romaji download failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("romaji download failed: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("romaji download failed: {e}"))?;
        tokio::task::spawn_blocking(move || install_from_zip(&bytes, &base))
            .await
            .map_err(|e| format!("romaji setup failed: {e}"))?
    }

    /// Verify `zip` against the pinned hash and unpack its dictionary
    /// files into `base` (temp folder, renamed into place last). Older
    /// dictionary folders are deleted once the new one is in place.
    pub fn install_from_zip(zip: &[u8], base: &Path) -> Result<(), String> {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(zip);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if hex != UNIDIC_SHA256 {
            return Err("romaji dictionary checksum mismatch: refusing to install".into());
        }
        let tmp = base.join(format!("{BUILT_DIR}.tmp"));
        let _ = std::fs::remove_dir_all(&tmp);
        let result = (|| {
            std::fs::create_dir_all(&tmp).map_err(|e| format!("romaji setup failed: {e}"))?;
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))
                .map_err(|e| format!("romaji unpack failed: {e}"))?;
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| format!("romaji unpack failed: {e}"))?;
                // Only plain files directly in the dictionary folder:
                // no traversal, no nesting.
                let Some(name) = entry.enclosed_name() else {
                    continue;
                };
                let Ok(rel) = name.strip_prefix(ZIP_DIR) else {
                    continue;
                };
                if !entry.is_file() || rel.components().count() != 1 {
                    continue;
                }
                let mut out = std::fs::File::create(tmp.join(rel))
                    .map_err(|e| format!("romaji unpack failed: {e}"))?;
                std::io::copy(&mut entry, &mut out)
                    .map_err(|e| format!("romaji unpack failed: {e}"))?;
            }
            if !tmp.join("metadata.json").is_file() {
                return Err("romaji unpack failed: no dictionary in the download".into());
            }
            let done = dictionary_dir(base);
            let _ = std::fs::remove_dir_all(&done);
            std::fs::rename(&tmp, &done).map_err(|e| format!("romaji setup failed: {e}"))?;
            // Older dictionaries (IPADIC builds, earlier versions) are
            // dead weight.
            if let Ok(entries) = std::fs::read_dir(base) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name != BUILT_DIR && e.path().is_dir() {
                        let _ = std::fs::remove_dir_all(e.path());
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(surface: &str, pos: &str, sub: &str, reading: &str) -> Morpheme {
        Morpheme {
            surface: surface.into(),
            pos: pos.into(),
            pos_sub: sub.into(),
            lemma: String::new(),
            reading: (!reading.is_empty()).then(|| reading.into()),
        }
    }

    fn with_lemma(mut t: Morpheme, lemma: &str) -> Morpheme {
        t.lemma = lemma.into();
        t
    }

    #[test]
    fn kana_converts_to_hepburn() {
        assert_eq!(kana_to_romaji("キミ"), "kimi");
        assert_eq!(kana_to_romaji("しんじつ"), "shinjitsu");
        assert_eq!(kana_to_romaji("キョウ"), "kyou");
        assert_eq!(kana_to_romaji("マッテ"), "matte");
        assert_eq!(kana_to_romaji("マッチ"), "matchi");
        assert_eq!(kana_to_romaji("ラーメン"), "raamen");
        assert_eq!(kana_to_romaji("ゲンイン"), "gen'in");
        assert_eq!(kana_to_romaji("コンヤ"), "kon'ya");
        assert_eq!(kana_to_romaji("パーティー"), "paatii");
        assert_eq!(kana_to_romaji("ファン"), "fan");
        assert_eq!(kana_to_romaji("ウォ"), "wo");
    }

    #[test]
    fn non_kana_passes_through() {
        assert_eq!(kana_to_romaji("LOVEソング"), "LOVEsongu");
        assert_eq!(kana_to_romaji("漢"), "漢");
    }

    #[test]
    fn japanese_detection_needs_kana() {
        assert!(is_japanese("君の名は"));
        assert!(is_japanese("カタカナ only"));
        assert!(!is_japanese("我爱你")); // Chinese: kanji only.
        assert!(!is_japanese("hello world"));
        assert!(line_needs_romaji("夜"));
        assert!(!line_needs_romaji("Oh yeah!"));
    }

    #[test]
    fn long_vowels_are_spelled_out() {
        assert_eq!(spell_long_vowels("ウンメー", "ウンメイ"), "ウンメイ");
        assert_eq!(spell_long_vowels("トーキョー", "トウキョウ"), "トウキョウ");
        // Inflected: forms differ, the vowel row decides.
        assert_eq!(spell_long_vowels("イオー", "イウ"), "イオウ");
        assert_eq!(spell_long_vowels("トーザカッ", "トオザカル"), "トオザカッ");
        assert_eq!(spell_long_vowels("センセー", ""), "センセイ");
        // Loanwords keep the mark (romaji doubles the vowel).
        assert_eq!(spell_long_vowels("ゲーム", "ゲーム"), "ゲーム");
        assert_eq!(
            kana_to_romaji(&spell_long_vowels("ゲーム", "ゲーム")),
            "geemu"
        );
    }

    #[test]
    fn particles_read_like_lyric_sites() {
        let tokens = [
            m("君", "代名詞", "*", "キミ"),
            m("の", "助詞", "格助詞", "ノ"),
            m("名", "名詞", "普通名詞", "ナ"),
            m("は", "助詞", "係助詞", "ワ"),
            m("心", "名詞", "普通名詞", "ココロ"),
            m("を", "助詞", "格助詞", "オ"),
            m("。", "補助記号", "句点", ""),
        ];
        assert_eq!(join_morphemes(&tokens), "kimi no na wa kokoro wo.");
    }

    #[test]
    fn inflections_glue_and_te_auxiliaries_stand_apart() {
        // 待っていた: いる after て inflects the verb.
        let tokens = [
            m("待っ", "動詞", "一般", "マッ"),
            m("て", "助詞", "接続助詞", "テ"),
            with_lemma(m("い", "動詞", "非自立可能", "イ"), "居る"),
            with_lemma(m("た", "助動詞", "*", "タ"), "た"),
        ];
        assert_eq!(join_morphemes(&tokens), "matteita");
        // 遠ざかってゆく: ゆく after て stands apart.
        let tokens = [
            m("遠ざかっ", "動詞", "一般", "トオザカッ"),
            m("て", "助詞", "接続助詞", "テ"),
            with_lemma(m("ゆく", "動詞", "非自立可能", "ユク"), "行く"),
        ];
        assert_eq!(join_morphemes(&tokens), "toozakatte yuku");
        // 悲しすぎ: すぎる glues.
        let tokens = [
            m("悲し", "形容詞", "一般", "カナシ"),
            with_lemma(m("すぎ", "動詞", "非自立可能", "スギ"), "過ぎる"),
        ];
        assert_eq!(join_morphemes(&tokens), "kanashisugi");
    }

    #[test]
    fn counters_nai_and_connectors() {
        let tokens = [
            m("一", "名詞", "数詞", "イチ"),
            m("度", "名詞", "普通名詞", "ド"),
        ];
        assert_eq!(join_morphemes(&tokens), "ichido");
        // ない joins a verb stem, not a noun.
        let tokens = [
            m("果てし", "名詞", "普通名詞", "ハテシ"),
            with_lemma(m("ない", "形容詞", "非自立可能", "ナイ"), "無い"),
            m("事", "名詞", "普通名詞", "コト"),
            with_lemma(m("ない", "形容詞", "非自立可能", "ナイ"), "無い"),
        ];
        assert_eq!(join_morphemes(&tokens), "hateshinai koto nai");
        // が as a connector stands apart.
        let tokens = [
            m("見る", "動詞", "一般", "ミル"),
            m("が", "助詞", "接続助詞", "ガ"),
            m("いい", "形容詞", "非自立可能", "イイ"),
        ];
        assert_eq!(join_morphemes(&tokens), "miru ga ii");
    }

    #[test]
    fn copulas_stand_apart_except_adjectival_na() {
        let tokens = [
            m("よかっ", "形容詞", "非自立可能", "ヨカッ"),
            with_lemma(m("た", "助動詞", "*", "タ"), "た"),
            with_lemma(m("でしょう", "助動詞", "*", "デショウ"), "です"),
        ];
        assert_eq!(join_morphemes(&tokens), "yokatta deshou");
        let tokens = [
            m("暖か", "形状詞", "一般", "アタタカ"),
            with_lemma(m("な", "助動詞", "*", "ナ"), "だ"),
            m("光", "名詞", "普通名詞", "ヒカリ"),
        ];
        assert_eq!(join_morphemes(&tokens), "atatakana hikari");
        // ...but に after it stays apart ("kasuka ni").
        let tokens = [
            m("微か", "形状詞", "一般", "カスカ"),
            with_lemma(m("に", "助動詞", "*", "ニ"), "だ"),
        ];
        assert_eq!(join_morphemes(&tokens), "kasuka ni");
    }

    #[test]
    fn prefixes_and_contractions_keep_their_sounds() {
        // ぶっ壊れてんだ: "bukkowareten da".
        let tokens = [
            m("ぶっ", "接頭辞", "*", "ブッ"),
            m("壊れ", "動詞", "一般", "コワレ"),
            m("て", "助詞", "接続助詞", "テ"),
            m("ん", "助詞", "準体助詞", "ン"),
            with_lemma(m("だ", "助動詞", "*", "ダ"), "だ"),
        ];
        assert_eq!(join_morphemes(&tokens), "bukkowareten da");
    }

    #[test]
    fn english_brackets_and_middle_dots() {
        let tokens = [
            m("Ba", "名詞", "普通名詞", ""),
            m("by", "名詞", "普通名詞", ""),
            Morpheme::blank(),
            m("アイ・アイ・ア", "名詞", "普通名詞", ""),
            Morpheme::blank(),
            m("(", "補助記号", "括弧開", ""),
            m("アイ", "名詞", "普通名詞", "アイ"),
            m(")", "補助記号", "括弧閉", ""),
        ];
        assert_eq!(join_morphemes(&tokens), "Baby ai ai a (ai)");
    }

    #[test]
    fn unknown_words_fall_back_to_kana() {
        let tokens = [m("ぴえん", "名詞", "普通名詞", "")];
        assert_eq!(join_morphemes(&tokens), "pien");
    }

    #[cfg(feature = "romaji")]
    #[test]
    fn tampered_download_never_installs() {
        let dir = std::env::temp_dir().join(format!("yutu-romaji-bad-{}", std::process::id()));
        let err = install_from_zip(b"not the dictionary", &dir).unwrap_err();
        assert!(err.contains("checksum"));
        assert!(!dictionary_ready(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live: downloads (~46 MB) and unpacks UniDic, then romanizes real
    /// lines. `cargo test -p yutu-core --features romaji -- --ignored romaji`
    #[cfg(feature = "romaji")]
    #[tokio::test]
    #[ignore]
    async fn live_romaji_dictionary_romanizes() {
        let dir = std::env::temp_dir().join(format!("yutu-romaji-live-{}", std::process::id()));
        ensure_dictionary(dir.clone()).await.unwrap();
        let r = Romanizer::load(&dir).unwrap();
        let cases = [
            ("君の名は。", "kimi no na wa."),
            ("東京へ行きましょう", "toukyou e ikimashou"),
            ("Baby 愛してるよ", "Baby aishiteru yo"),
            ("僕はずっと君を待っていた", "boku wa zutto kimi wo matteita"),
            (
                "夢ならばどれほどよかったでしょう",
                "yume naraba dore hodo yokatta deshou",
            ),
            // Ado, アイ・アイ・ア.
            ("ぶっ壊れてんだ", "bukkowareten da"),
            // Same sounds as Genius' "sabishisugi sugisu" (wordplay).
            ("さびしすぎすギス", "sabishisugisu gisu"),
            // Unlucky Morpheus, Phantom Blood (archaic words).
            ("虚けた虫の戯れ言", "utsuketa mushi no zaregoto"),
            ("鋭い爪が食い込む雅味を", "surudoi tsume ga kuikomu gami wo"),
            ("傅き 誓え", "kashizuki chikae"),
            ("昏き世界", "kuraki sekai"),
            // GALNERYUS, I BELIEVE.
            (
                "明日はきっと 暖かな光が",
                "asu wa kitto atatakana hikari ga",
            ),
            ("また遠ざかってゆく", "mata toozakatte yuku"),
            ("折れる事ない この心を", "oreru koto nai kono kokoro wo"),
            (
                "果てしない先へと進むだろう",
                "hateshinai saki e to susumu darou",
            ),
            (
                "Broken wings, fly again もう一度だけ",
                "Broken wings, fly again mou ichido dake",
            ),
            (
                "居並ぶ亡霊 私を見るがいい",
                "inarabu bourei watashi wo miru ga ii",
            ),
        ];
        let mut misses = Vec::new();
        for (line, want) in cases {
            let got = r.romanize_line(line).unwrap_or_default();
            if got != want {
                misses.push(format!("{line}: got {got:?}, want {want:?}"));
            }
        }
        assert_eq!(r.romanize_line("Oh yeah"), None);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(misses.is_empty(), "{misses:#?}");
    }
}
