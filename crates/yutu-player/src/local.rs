//! Local folder scan for audio files.
//!
//! Recursive, std-only, sorted output. Skips hidden directories
//! (any dir whose name starts with `.`).

use std::path::Path;

/// Audio extensions (lowercase, without the dot).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "m4a", "opus", "wav", "aac"];

/// Returns true if the file extension is a known audio type (case-insensitive).
#[must_use]
pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            AUDIO_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

fn is_hidden_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

fn visit(dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Never follow symlinked directories (avoids cycles); a symlinked
        // *file* with an audio extension is still collected.
        if path.is_symlink() {
            if path.is_file() && is_audio_file(&path) {
                out.push(path.to_string_lossy().into_owned());
            }
            continue;
        }
        if path.is_dir() {
            if is_hidden_dir(&path) {
                continue;
            }
            visit(&path, out);
        } else if path.is_file() && is_audio_file(&path) {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

/// Recursively scan `path` for audio files.
///
/// Returns sorted `Vec<String>` of full paths. Errors (as `String`) when
/// `path` does not exist or is not a directory.
pub fn scan_folder(path: impl AsRef<Path>) -> Result<Vec<String>, String> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(format!("directory does not exist: {}", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("not a directory: {}", path.display()));
    }
    let mut out = Vec::new();
    visit(path, &mut out);
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn unique_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("yutu-scan-test-{}-{tag}", std::process::id()))
    }

    fn setup_tree(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("a.mp3"), b"fake").unwrap();
        std::fs::write(root.join("b.FLAC"), b"fake").unwrap();
        std::fs::write(root.join("notes.txt"), b"nope").unwrap();
        std::fs::write(root.join("sub").join("c.ogg"), b"fake").unwrap();
        std::fs::write(root.join("sub").join("d.wav"), b"fake").unwrap();
        std::fs::write(root.join("sub").join("e.mp4"), b"nope").unwrap();
        std::fs::write(root.join(".hidden").join("secret.mp3"), b"fake").unwrap();
    }

    #[test]
    fn finds_audio_recursively_sorted_and_skips_hidden_and_non_audio() {
        let root = unique_root("basic");
        setup_tree(&root);
        let found = scan_folder(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let mut expected = vec![
            root.join("a.mp3").to_string_lossy().into_owned(),
            root.join("b.FLAC").to_string_lossy().into_owned(),
            root.join("sub")
                .join("c.ogg")
                .to_string_lossy()
                .into_owned(),
            root.join("sub")
                .join("d.wav")
                .to_string_lossy()
                .into_owned(),
        ];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn errors_on_nonexistent_dir() {
        let missing =
            std::env::temp_dir().join(format!("yutu-scan-missing-{}-nodir", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        let err = scan_folder(&missing).unwrap_err();
        assert!(err.contains("does not exist"), "unexpected err: {err}");
    }

    #[test]
    fn errors_on_file_not_dir() {
        let root = unique_root("file-not-dir");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let f = root.join("x.mp3");
        std::fs::write(&f, b"fake").unwrap();
        let err = scan_folder(&f).unwrap_err();
        assert!(err.contains("not a directory"), "unexpected err: {err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_dir_returns_empty_vec() {
        let root = unique_root("empty");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(scan_folder(&root).unwrap(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn matches_all_supported_extensions_case_insensitively() {
        let root = unique_root("exts");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for ext in ["mp3", "FLAC", "Ogg", "m4a", "OPUS", "wav", "AAC"] {
            std::fs::write(root.join(format!("s.{ext}")), b"fake").unwrap();
        }
        std::fs::write(root.join("s.mp4"), b"nope").unwrap();
        let found = scan_folder(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(found.len(), 7);
    }
}
