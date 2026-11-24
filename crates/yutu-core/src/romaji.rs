//! Japanese lyrics romanization (romaji under each line).
//!
//! Kanji need context to read (`君の名は` is "kimi no na wa"), so lines
//! go through a morphological analyzer (lindera, MIT) over the IPADIC
//! dictionary. The dictionary is not embedded (it would add ~47 MB to
//! the binary): the first time the user turns romaji on, the app
//! downloads the IPADIC source tarball once (~13 MB, pinned SHA-256),
//! builds it locally (~2 s) and keeps the built files in the data dir.
//!
//! Rules:
//! - Only lyrics with kana anywhere count as Japanese ([`is_japanese`]):
//!   kanji alone is ambiguous (Chinese lyrics) and stays untouched.
//! - Lines without Japanese characters get no romaji (English lines in
//!   a J-pop song already read fine).
//! - Readings come from the dictionary; particles read as spoken
//!   (は "wa", へ "e", を "o"); unknown words fall back to their kana.
//! - Hepburn spelling, long vowels as written (`toukyou`), `n'` before
//!   a vowel or `y`, doubled consonants for small `っ`.

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

/// True when `line` has any Japanese character worth a romaji line.
pub fn line_needs_romaji(line: &str) -> bool {
    line.chars().any(kana::is_japanese_char)
}

/// One analyzed token: surface text, part of speech, dictionary reading
/// (katakana, `None` for unknown words). Kept separate from lindera so
/// the joining rules are unit-tested without a dictionary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Morpheme {
    pub surface: String,
    pub pos: String,
    pub pos_sub: String,
    /// Dictionary base form (`だ` for `でしょ`), empty when unknown.
    pub base: String,
    pub reading: Option<String>,
}

impl Morpheme {
    /// Word separator token (original whitespace).
    pub fn blank() -> Self {
        Self {
            surface: " ".into(),
            pos: String::new(),
            pos_sub: String::new(),
            base: String::new(),
            reading: None,
        }
    }
}

/// Join analyzed tokens into a readable romaji line: words separated by
/// spaces, inflections and auxiliaries glued to their word
/// (`待っていた` reads "matteita"), punctuation attached.
pub fn join_morphemes(tokens: &[Morpheme]) -> String {
    // Words collect kana (and literal text) first and convert once, so
    // sound changes across glued tokens survive (`マッ` + `テ` is
    // "matte", not "mate").
    let mut words: Vec<String> = Vec::new();
    let mut spaced = true;
    // Set by a prefix (`ぶっ`) or an opening bracket: the next token
    // joins it (`ブッ` + `コワレ` keeps its doubled consonant).
    let mut glue_next = false;
    for t in tokens {
        if t.surface.trim().is_empty() {
            // Original spacing separates words (English mixed in).
            spaced = true;
            glue_next = false;
            continue;
        }
        let source = morpheme_source(t);
        if source.is_empty() {
            continue;
        }
        let punctuation =
            !line_needs_romaji(&t.surface) && !t.surface.chars().any(char::is_alphanumeric);
        let opening = punctuation && source.ends_with(['(', '[']);
        if std::mem::take(&mut glue_next) {
            if let Some(word) = words.last_mut() {
                word.push_str(&source);
                glue_next = opening || t.pos == "接頭詞";
                spaced = false;
                continue;
            }
        }
        if opening || t.pos == "接頭詞" {
            words.push(source);
            glue_next = true;
            spaced = false;
            continue;
        }
        // Copulas (だ, です and their forms) read as their own word
        // ("yokatta deshou"); other auxiliaries inflect the verb.
        let copula = t.pos == "助動詞" && matches!(t.base.as_str(), "だ" | "です");
        let glue = punctuation
            || (t.pos == "助動詞" && !copula)
            || (t.pos == "助詞" && t.pos_sub == "接続助詞")
            || (t.pos == "動詞" && (t.pos_sub == "非自立" || t.pos_sub == "接尾"))
            || t.pos_sub == "接尾"
            // Contracted の (`壊れてんだ` reads "kowareten da").
            || (t.surface == "ん" && t.pos == "名詞" && t.pos_sub == "非自立");
        match words.last_mut() {
            Some(word) if glue && !(spaced && !punctuation) => word.push_str(&source),
            _ => words.push(source),
        }
        spaced = false;
    }
    words
        .iter()
        .map(|w| kana::to_romaji(w))
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Kana to convert for one token: particles as spoken, dictionary
/// readings, kana surfaces for unknown words; non-Japanese text as is
/// (full-width punctuation to ASCII, quote brackets dropped).
fn morpheme_source(t: &Morpheme) -> String {
    if t.pos == "助詞" {
        match t.surface.as_str() {
            "は" => return "ワ".into(),
            "へ" => return "エ".into(),
            "を" => return "オ".into(),
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

/// Most likely reading of each kanji, derived from two-kanji dictionary
/// words (`心臓` シンゾウ, `内臓` ナイゾウ, `臓器` ゾウキ give 臓 ゾウ).
/// Used for kanji the analyzer cannot read alone (`臓` has no entry of
/// its own), so a rare kanji never leaks into the romaji line.
///
/// Every split of a word's reading votes for both halves; a few rounds
/// then keep only each word's best split under the current votes, so
/// readings shared across many compounds win.
pub fn derive_kanji_readings<'a>(
    words: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> std::collections::HashMap<char, String> {
    use std::collections::{HashMap, HashSet};
    let is_kanji = |c: char| matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}');
    let is_katakana = |r: &str| {
        r.chars()
            .all(|c| matches!(c, '\u{30A1}'..='\u{30FA}' | 'ー'))
    };
    let mut pairs: HashSet<([char; 2], Vec<char>)> = HashSet::new();
    for (surface, reading) in words {
        let s: Vec<char> = surface.chars().collect();
        if s.len() == 2 && s.iter().all(|c| is_kanji(*c)) && is_katakana(reading) {
            let r: Vec<char> = reading.chars().collect();
            if r.len() >= 2 {
                pairs.insert(([s[0], s[1]], r));
            }
        }
    }
    // A reading half never starts on a small kana, ン, ッ or ー.
    let splits = |r: &[char]| -> Vec<usize> {
        (1..r.len())
            .filter(|&i| !"ァィゥェォャュョッンー".contains(r[i]))
            .collect()
    };
    type Votes = HashMap<(char, String), u32>;
    let half = |r: &[char], a: usize, b: usize| r[a..b].iter().collect::<String>();
    let mut votes: Votes = HashMap::new();
    for (k, r) in &pairs {
        for i in splits(r) {
            *votes.entry((k[0], half(r, 0, i))).or_default() += 1;
            *votes.entry((k[1], half(r, i, r.len()))).or_default() += 1;
        }
    }
    for _ in 0..3 {
        let mut next: Votes = HashMap::new();
        for (k, r) in &pairs {
            let score = |i: usize| {
                let v = |key: (char, String)| votes.get(&key).copied().unwrap_or(0);
                let balance = -((2 * i) as i64 - r.len() as i64).abs();
                (
                    v((k[0], half(r, 0, i))) + v((k[1], half(r, i, r.len()))),
                    balance,
                )
            };
            if let Some(i) = splits(r).into_iter().max_by_key(|&i| score(i)) {
                *next.entry((k[0], half(r, 0, i))).or_default() += 1;
                *next.entry((k[1], half(r, i, r.len()))).or_default() += 1;
            }
        }
        votes = next;
    }
    let mut best: HashMap<char, (String, u32)> = HashMap::new();
    for ((k, r), n) in votes {
        let better = best
            .get(&k)
            .is_none_or(|(br, bn)| (n, r.chars().count(), &r) > (*bn, br.chars().count(), br));
        if better {
            best.insert(k, (r, n));
        }
    }
    best.into_iter().map(|(k, (r, _))| (k, r)).collect()
}

/// Reading for a token the dictionary could not read: kana kept, each
/// kanji from `kanji` (`々` repeats the one before), `None` when a
/// kanji has no known reading either.
pub fn fallback_reading(
    surface: &str,
    kanji: &std::collections::HashMap<char, String>,
) -> Option<String> {
    let mut out = String::new();
    let mut last: Option<&str> = None;
    for c in surface.chars() {
        if let Some(r) = kanji.get(&c) {
            out.push_str(r);
            last = Some(r);
        } else if c == '々' {
            out.push_str(last?);
        } else if kana::is_japanese_char(c) && !kana::is_kana(c) {
            return None;
        } else {
            out.push(c);
            last = None;
        }
    }
    Some(out)
}

#[cfg(feature = "romaji")]
pub use analyzer::*;

#[cfg(feature = "romaji")]
mod analyzer {
    use super::{
        derive_kanji_readings, fallback_reading, join_morphemes, line_needs_romaji, Morpheme,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// IPADIC source tarball (the one lindera's own build uses), UTF-8.
    pub const IPADIC_URL: &str = "https://Lindera.dev/mecab-ipadic-2.7.0-20250920.tar.gz";
    /// Pinned SHA-256 of [`IPADIC_URL`]: anything else never builds.
    pub const IPADIC_SHA256: &str =
        "a7ba9f645ffe7094e56ae1c4a81d100df8fbb1e28bbe1792622e9728e162db3d";
    const IPADIC_SRC_DIR: &str = "mecab-ipadic-2.7.0-20250920";
    /// Built dictionary folder: keyed by the lindera version, whose
    /// on-disk format can change between releases, plus our own build
    /// revision (`r2` added the kanji readings): a bump rebuilds, and
    /// the finished build deletes older folders.
    const BUILT_DIR: &str = "ipadic-lindera-6.2.0-r2";
    /// Derived per-kanji readings (`kanji<TAB>reading` lines) beside
    /// the built dictionary, see [`derive_kanji_readings`].
    const KANJI_FILE: &str = "kanji_readings.tsv";
    const METADATA: &str = include_str!("romaji_ipadic_metadata.json");

    /// Built dictionary location under the romaji data dir.
    pub fn dictionary_dir(base: &Path) -> PathBuf {
        base.join(BUILT_DIR)
    }

    /// True when a finished build sits in `base` (the build writes the
    /// folder under a temp name and renames it last, so a crash never
    /// leaves a half dictionary that looks done).
    pub fn dictionary_ready(base: &Path) -> bool {
        dictionary_dir(base).join("metadata.json").is_file()
    }

    /// Loaded analyzer. Cheap to share (`Arc`), segmenting is sync and
    /// takes microseconds per lyric line.
    pub struct Romanizer {
        segmenter: lindera::segmenter::Segmenter,
        kanji: HashMap<char, String>,
    }

    impl std::fmt::Debug for Romanizer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Romanizer")
        }
    }

    impl Romanizer {
        /// Load the built dictionary from `base` (see [`ensure_dictionary`]).
        pub fn load(base: &Path) -> Result<Self, String> {
            let dir = dictionary_dir(base);
            let path = dir.to_str().ok_or("romaji dictionary path is not UTF-8")?;
            let dict = lindera::dictionary::load_dictionary(path)
                .map_err(|e| format!("romaji dictionary load failed: {e}"))?;
            let segmenter =
                lindera::segmenter::Segmenter::new(lindera::mode::Mode::Normal, dict, None);
            let kanji = std::fs::read_to_string(dir.join(KANJI_FILE))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let (k, r) = l.split_once('\t')?;
                    Some((k.chars().next()?, r.to_string()))
                })
                .collect();
            Ok(Self { segmenter, kanji })
        }

        /// Romaji for one lyric line, `None` when it has nothing
        /// Japanese (or the analyzer fails on it).
        pub fn romanize_line(&self, line: &str) -> Option<String> {
            let line = line.trim();
            if !line_needs_romaji(line) {
                return None;
            }
            // Chunks between spaces analyze alone (a Latin word before
            // a verb otherwise shifts the lattice: "Baby 愛してる" read
            // 愛 as "ito"); a blank token keeps the original spacing.
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
                    // Unknown words with kanji read kanji by kanji.
                    let reading = field(7).or_else(|| fallback_reading(&surface, &self.kanji));
                    Morpheme {
                        pos: field(0).unwrap_or_default(),
                        pos_sub: field(1).unwrap_or_default(),
                        base: field(6).unwrap_or_default(),
                        reading,
                        surface,
                    }
                }));
            }
            let out = join_morphemes(&morphemes);
            (!out.is_empty()).then_some(out)
        }
    }

    /// Download, verify and build the dictionary into `base` unless it
    /// is already there. Blocking file work runs on a blocking thread.
    pub async fn ensure_dictionary(base: PathBuf) -> Result<(), String> {
        if dictionary_ready(&base) {
            return Ok(());
        }
        std::fs::create_dir_all(&base).map_err(|e| format!("romaji setup failed: {e}"))?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("yutu-mux/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("romaji download failed: {e}"))?;
        let bytes = client
            .get(IPADIC_URL)
            .send()
            .await
            .map_err(|e| format!("romaji download failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("romaji download failed: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("romaji download failed: {e}"))?;
        tokio::task::spawn_blocking(move || build_from_tarball(&bytes, &base))
            .await
            .map_err(|e| format!("romaji build failed: {e}"))?
    }

    /// Derive per-kanji readings from the source CSVs (surface is the
    /// first field, reading the twelfth) into `out`.
    fn write_kanji_readings(source: &Path, out: &Path) -> Result<(), String> {
        let mut text = String::new();
        let dir = std::fs::read_dir(source).map_err(|e| format!("romaji build failed: {e}"))?;
        for e in dir.flatten() {
            if e.path().extension().is_some_and(|x| x == "csv") {
                text.push_str(&std::fs::read_to_string(e.path()).unwrap_or_default());
                text.push('\n');
            }
        }
        let words = text.lines().filter_map(|l| {
            let mut f = l.split(',');
            let surface = f.next()?;
            Some((surface, f.nth(10)?))
        });
        let mut lines: Vec<String> = derive_kanji_readings(words)
            .into_iter()
            .map(|(k, r)| format!("{k}\t{r}"))
            .collect();
        lines.sort();
        std::fs::write(out, lines.join("\n")).map_err(|e| format!("romaji build failed: {e}"))
    }

    /// Verify `tarball` against the pinned hash, unpack and build it
    /// into `base` (temp folders, renamed into place last).
    pub fn build_from_tarball(tarball: &[u8], base: &Path) -> Result<(), String> {
        use sha2::Digest as _;
        let digest = sha2::Sha256::digest(tarball);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if hex != IPADIC_SHA256 {
            return Err("romaji dictionary checksum mismatch: refusing to build".into());
        }
        let src = base.join("source.tmp");
        let out = base.join(format!("{BUILT_DIR}.tmp"));
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&out);
        let result = (|| {
            tar::Archive::new(flate2::read::GzDecoder::new(tarball))
                .unpack(&src)
                .map_err(|e| format!("romaji unpack failed: {e}"))?;
            let metadata: lindera::dictionary::Metadata = serde_json::from_str(METADATA)
                .map_err(|e| format!("romaji metadata invalid: {e}"))?;
            let source = src.join(IPADIC_SRC_DIR);
            lindera::dictionary::DictionaryBuilder::new(metadata)
                .build_dictionary(&source, &out)
                .map_err(|e| format!("romaji build failed: {e}"))?;
            write_kanji_readings(&source, &out.join(KANJI_FILE))?;
            let done = dictionary_dir(base);
            let _ = std::fs::remove_dir_all(&done);
            std::fs::rename(&out, &done).map_err(|e| format!("romaji build failed: {e}"))?;
            // Older builds (earlier lindera or revision) are dead weight.
            if let Ok(entries) = std::fs::read_dir(base) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with("ipadic-") && name != BUILT_DIR {
                        let _ = std::fs::remove_dir_all(e.path());
                    }
                }
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&src);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&out);
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
            base: String::new(),
            reading: (!reading.is_empty()).then(|| reading.into()),
        }
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
    fn particles_read_as_spoken() {
        let tokens = [
            m("君", "名詞", "代名詞", "キミ"),
            m("の", "助詞", "連体化", "ノ"),
            m("名", "名詞", "一般", "ナ"),
            m("は", "助詞", "係助詞", "ハ"),
            m("。", "記号", "句点", "。"),
        ];
        assert_eq!(join_morphemes(&tokens), "kimi no na wa.");
    }

    #[test]
    fn inflections_glue_to_their_word() {
        let tokens = [
            m("僕", "名詞", "代名詞", "ボク"),
            m("は", "助詞", "係助詞", "ハ"),
            m("君", "名詞", "代名詞", "キミ"),
            m("を", "助詞", "格助詞", "ヲ"),
            m("待っ", "動詞", "自立", "マッ"),
            m("て", "助詞", "接続助詞", "テ"),
            m("い", "動詞", "非自立", "イ"),
            m("た", "助動詞", "*", "タ"),
        ];
        assert_eq!(join_morphemes(&tokens), "boku wa kimi o matteita");
    }

    #[test]
    fn english_and_spaces_survive() {
        let tokens = [
            m("Baby", "名詞", "固有名詞", ""),
            m(" ", "記号", "空白", ""),
            m("愛", "名詞", "一般", "アイ"),
            m("し", "動詞", "自立", "シ"),
            m("てる", "動詞", "非自立", "テル"),
        ];
        assert_eq!(join_morphemes(&tokens), "Baby ai shiteru");
    }

    #[test]
    fn unknown_words_fall_back_to_kana() {
        let tokens = [m("ぴえん", "名詞", "一般", "")];
        assert_eq!(join_morphemes(&tokens), "pien");
    }

    #[test]
    fn prefixes_keep_their_doubled_consonant() {
        // ぶっ壊れてんだ: "bukkowareten da", not "bu kowarete n da".
        let tokens = [
            m("ぶっ", "接頭詞", "動詞接続", "ブッ"),
            m("壊れ", "動詞", "自立", "コワレ"),
            m("て", "助詞", "接続助詞", "テ"),
            m("ん", "名詞", "非自立", "ン"),
            Morpheme {
                base: "だ".into(),
                ..m("だ", "助動詞", "*", "ダ")
            },
        ];
        assert_eq!(join_morphemes(&tokens), "bukkowareten da");
    }

    #[test]
    fn brackets_and_middle_dots_read_cleanly() {
        // Unknown katakana keeps no ・, and ( opens the next word.
        let tokens = [
            m("アイ・アイ・ア", "名詞", "一般", ""),
            Morpheme::blank(),
            m("(", "名詞", "サ変接続", ""),
            m("アイ・アイ・ア", "名詞", "一般", ""),
            m(")", "名詞", "サ変接続", ""),
        ];
        assert_eq!(join_morphemes(&tokens), "ai ai a (ai ai a)");
    }

    #[test]
    fn kanji_readings_come_from_compounds() {
        let words = [
            ("心臓", "シンゾウ"),
            ("内臓", "ナイゾウ"),
            ("臓器", "ゾウキ"),
            ("腎臓", "ジンゾウ"),
            ("内科", "ナイカ"),
            ("心身", "シンシン"),
            ("器官", "キカン"),
            ("食器", "ショッキ"),
            ("一つ", "ヒトツ"), // not two kanji: ignored
        ];
        let map = derive_kanji_readings(words);
        assert_eq!(map.get(&'臓').map(String::as_str), Some("ゾウ"));
        assert_eq!(map.get(&'内').map(String::as_str), Some("ナイ"));
        assert_eq!(map.get(&'器').map(String::as_str), Some("キ"));
        assert!(!map.contains_key(&'つ'));
        // Unknown tokens read kanji by kanji; 々 repeats; no reading
        // for a kanji means no fallback at all.
        assert_eq!(fallback_reading("臓", &map).as_deref(), Some("ゾウ"));
        assert_eq!(fallback_reading("臓々", &map).as_deref(), Some("ゾウゾウ"));
        assert_eq!(
            fallback_reading("内のか", &map).as_deref(),
            Some("ナイのか")
        );
        assert_eq!(fallback_reading("儚", &map), None);
    }

    #[cfg(feature = "romaji")]
    #[test]
    fn tampered_tarball_never_builds() {
        let dir = std::env::temp_dir().join(format!("yutu-romaji-bad-{}", std::process::id()));
        let err = build_from_tarball(b"not the dictionary", &dir).unwrap_err();
        assert!(err.contains("checksum"));
        assert!(!dictionary_ready(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live: downloads (~13 MB) and builds IPADIC, then romanizes real
    /// lines. `cargo test -p yutu-core --features romaji -- --ignored romaji`
    #[cfg(feature = "romaji")]
    #[tokio::test]
    #[ignore]
    async fn live_romaji_dictionary_romanizes() {
        let dir = std::env::temp_dir().join(format!("yutu-romaji-live-{}", std::process::id()));
        ensure_dictionary(dir.clone()).await.unwrap();
        let r = Romanizer::load(&dir).unwrap();
        assert_eq!(
            r.romanize_line("君の名は。").as_deref(),
            Some("kimi no na wa.")
        );
        assert_eq!(
            r.romanize_line("東京へ行きましょう").as_deref(),
            Some("toukyou e ikimashou")
        );
        assert_eq!(r.romanize_line("Oh yeah"), None);
        assert_eq!(
            r.romanize_line("Baby 愛してるよ").as_deref(),
            Some("Baby aishiteru yo")
        );
        assert_eq!(
            r.romanize_line("夢ならばどれほどよかったでしょう")
                .as_deref(),
            Some("yume naraba dorehodo yokatta deshou")
        );
        assert_eq!(
            r.romanize_line("パーティーへ行こうぜ！").as_deref(),
            Some("paatii e ikou ze!")
        );
        // Lines from a real song (Ado, アイ・アイ・ア).
        assert_eq!(
            r.romanize_line("グサグサ刺せよ心の臓まで、huh?").as_deref(),
            Some("gusagusa saseyo kokoro no zou made, huh?")
        );
        assert_eq!(
            r.romanize_line("ぶっ壊れてんだ ガラスのハートなんてさ")
                .as_deref(),
            Some("bukkowareten da garasu no haato nante sa")
        );
        assert_eq!(
            r.romanize_line("アイ・アイ・ア (アイ・アイ・ア)")
                .as_deref(),
            Some("ai ai a (ai ai a)")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
