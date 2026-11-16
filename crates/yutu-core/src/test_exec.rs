//! Test helper: write an executable fake sidecar without `ETXTBSY`.
//!
//! Writing a script with `std::fs::write` and then running it races
//! with any parallel test that forks: the child inherits the open write
//! handle until its own exec, and Linux refuses to exec a file that is
//! open for writing ("Text file busy"). Writing from a short-lived `sh`
//! child keeps the handle out of the test process entirely, so no fork
//! can ever inherit it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Write `contents` to `path` as an executable (0755) file.
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"cat > "$1" && chmod 755 "$1""#)
        .arg("sh")
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn sh to write the fake sidecar");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(contents.as_bytes())
        .expect("write fake sidecar");
    let status = child.wait().expect("wait for sh");
    assert!(status.success(), "writing {} failed", path.display());
}
