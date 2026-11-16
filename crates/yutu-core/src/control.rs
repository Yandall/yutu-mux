//! Local control protocol: lets scripts and AI agents drive the running
//! app with semantic commands (play, search, navigate, screenshot)
//! instead of synthesizing mouse input.
//!
//! Rules:
//! - Opt-in: the app only listens when started with `--control` or
//!   `YUTU_CONTROL=1`. Nothing listens by default.
//! - Transport: a Unix domain socket in a per-user `0700` directory,
//!   the socket itself `0600`. Local same-user access only, no TCP.
//! - Wire format: one JSON object per line each way. The request is
//!   `{"args":["play","<id>"]}` (the same words the `ctl` CLI takes),
//!   the reply `{"ok":true,"data":...}` or `{"ok":false,"error":"..."}`.
//!   Any client works (`socat`, Python), the CLI is just a thin one.
//! - Parsing lives here (pure, tested); the UI only executes a typed
//!   [`Command`].

use std::path::PathBuf;

/// One typed control command, parsed from CLI-style words.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Liveness probe (replies with the app version).
    Ping,
    /// Full UI/playback snapshot (route, now playing, window, flags).
    State,
    /// Manual queue + autoplay tail.
    Queue,
    /// Current search results (tracks, albums, artists, playlists).
    Results,
    /// The user's playlists (id, title, track count).
    Playlists,
    /// Tracks on the open page (playlist, album, artist, history).
    Page,
    /// Run a search and show the search page.
    Search(String),
    /// Play a track by id (replaces the queue, like a row click).
    Play(String),
    /// Play the nth track (0-based) of the current search results.
    PlayResult(usize),
    /// Play a playlist from its first track.
    PlayPlaylist(String),
    /// Start a radio mix seeded by a track id.
    Radio(String),
    Pause,
    Resume,
    Toggle,
    Stop,
    Next,
    Prev,
    /// Seek to an absolute position in seconds.
    Seek(f64),
    /// Volume 0.0..=1.0.
    Volume(f32),
    Mute,
    /// `None` toggles.
    Shuffle(Option<bool>),
    /// `None` cycles.
    Repeat(Option<RepeatArg>),
    /// Append a track id to the manual queue.
    Enqueue(String),
    /// Insert a track id right after the current one.
    PlayNext(String),
    /// Jump to a manual-queue index (0-based).
    Jump(usize),
    /// Remove a track id from the queue.
    Remove(String),
    ClearQueue,
    /// Toggle like on a track id (the current track when omitted).
    Like(Option<String>),
    /// Go to a page: `home`, `search`, `history`, `discovery`,
    /// `settings`, `back`, `forward`, `artist:<id>`, `album:<id>`,
    /// `playlist:<id>`, `lyrics:<id>`.
    Nav(String),
    /// Now Playing drawer.
    NowPlaying(Option<bool>),
    /// Library drawer/rail (Narrow: drawer, else pin).
    Library(Option<bool>),
    /// Queue drawer/rail (Narrow: drawer, else pin).
    QueuePanel(Option<bool>),
    /// Close menus, dialogs and drawers (like Esc).
    Escape,
    /// Resize the window (logical pixels).
    Resize(f32, f32),
    /// Capture the real window to a PNG at this path.
    Screenshot(Option<PathBuf>),
    /// Hyprland: float (`Some(true)`), tile (`Some(false)`) or toggle
    /// the window, like a Super+T keybind.
    Float(Option<bool>),
    /// Mini player window (picture-in-picture).
    Mini(MiniArg),
    /// Save the session and close the app.
    Quit,
}

/// `mini` subcommands.
#[derive(Debug, Clone, PartialEq)]
pub enum MiniArg {
    /// Open when closed, close when open (bare `mini`).
    Toggle,
    Open,
    Close,
    /// Close it and bring the main window forward.
    Restore,
    /// Lyrics ticker; `None` toggles.
    Lyrics(Option<bool>),
    /// Capture the mini window to a PNG.
    Screenshot(Option<PathBuf>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatArg {
    Off,
    All,
    One,
}

/// Command list for `ctl help` (one line per command).
pub const HELP: &str = "\
yutu-mux ctl <command> [args]   (start the app with --control or YUTU_CONTROL=1)

Inspect:
  ping                     check the app is listening
  state                    route, now playing, position, window, flags
  queue                    manual queue + autoplay tail
  results                  current search results
  playlists                your playlists
  page                     tracks on the open page

Playback:
  search <query...>        search and open the results page
  play <track-id>          play a track (replaces the queue)
  play-result <n>          play the nth search result (0-based)
  play-playlist <id>       play a playlist
  radio <track-id>         start a radio mix
  pause | resume | toggle | stop | next | prev
  seek <seconds>           absolute position
  volume <0..1>            set volume (unmutes)
  mute                     toggle mute
  shuffle [on|off]         toggle or set
  repeat [off|all|one]     cycle or set
  enqueue <track-id>       append to the queue
  play-next <track-id>     insert after the current track
  jump <index>             jump to a queue index (0-based)
  remove <track-id>        remove from the queue
  clear-queue              clear the manual queue
  like [track-id]          toggle like (current track by default)

UI:
  nav <page>               home|search|history|discovery|settings|back|forward
                           artist:<id>|album:<id>|playlist:<id>|lyrics:<id>
  now-playing [on|off]     Now Playing drawer
  library [on|off]         library panel
  queue-panel [on|off]     queue panel
  escape                   close menus, dialogs and drawers
  resize <w> <h>           resize the window (logical px; Hyprland: float first)
  float [on|off]           Hyprland: float, tile or toggle the window
  screenshot [path.png]    capture the window (default: temp dir)
  mini [open|close|restore]          mini player (bare: toggle)
  mini lyrics [on|off]               mini player lyrics ticker
  mini screenshot [path.png]         capture the mini player window
  quit                     save the session and close the app

Client flags:
  --wait                   after the command, poll until the app is idle
  --timeout <secs>         wait limit (default 20)
  --socket <path>          socket path (default: per-user runtime dir)";

fn need<'a>(args: &'a [String], i: usize, what: &str) -> Result<&'a str, String> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| format!("missing {what}"))
}

fn on_off(arg: Option<&String>) -> Result<Option<bool>, String> {
    match arg.map(|s| s.to_ascii_lowercase()).as_deref() {
        None => Ok(None),
        Some("on" | "true" | "1" | "open") => Ok(Some(true)),
        Some("off" | "false" | "0" | "close") => Ok(Some(false)),
        Some("toggle") => Ok(None),
        Some(other) => Err(format!("expected on|off, got `{other}`")),
    }
}

fn num<T: std::str::FromStr>(s: &str, what: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("invalid {what}: `{s}`"))
}

/// Parse CLI-style words (`["seek", "42"]`) into a [`Command`].
pub fn parse_command(args: &[String]) -> Result<Command, String> {
    let Some(cmd) = args.first() else {
        return Err("empty command (try `help`)".into());
    };
    let rest = || args[1..].join(" ");
    let c = match cmd.as_str() {
        "ping" => Command::Ping,
        "state" | "status" => Command::State,
        "queue" => Command::Queue,
        "results" => Command::Results,
        "playlists" => Command::Playlists,
        "page" => Command::Page,
        "search" => {
            let q = rest();
            if q.trim().is_empty() {
                return Err("missing search query".into());
            }
            Command::Search(q)
        }
        "play" => Command::Play(need(args, 1, "track id")?.into()),
        "play-result" => Command::PlayResult(num(need(args, 1, "result index")?, "index")?),
        "play-playlist" => Command::PlayPlaylist(need(args, 1, "playlist id")?.into()),
        "radio" => Command::Radio(need(args, 1, "track id")?.into()),
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        "toggle" => Command::Toggle,
        "stop" => Command::Stop,
        "next" => Command::Next,
        "prev" | "previous" => Command::Prev,
        "seek" => {
            let s: f64 = num(need(args, 1, "seconds")?, "seconds")?;
            if !s.is_finite() || s < 0.0 {
                return Err("seconds must be >= 0".into());
            }
            Command::Seek(s)
        }
        "volume" => {
            let v: f32 = num(need(args, 1, "volume")?, "volume")?;
            if !(0.0..=1.0).contains(&v) {
                return Err("volume must be within 0..1".into());
            }
            Command::Volume(v)
        }
        "mute" => Command::Mute,
        "shuffle" => Command::Shuffle(on_off(args.get(1))?),
        "repeat" => Command::Repeat(match args.get(1).map(String::as_str) {
            None => None,
            Some("off" | "none") => Some(RepeatArg::Off),
            Some("all" | "playlist") => Some(RepeatArg::All),
            Some("one" | "track") => Some(RepeatArg::One),
            Some(o) => return Err(format!("expected off|all|one, got `{o}`")),
        }),
        "enqueue" => Command::Enqueue(need(args, 1, "track id")?.into()),
        "play-next" => Command::PlayNext(need(args, 1, "track id")?.into()),
        "jump" => Command::Jump(num(need(args, 1, "queue index")?, "index")?),
        "remove" => Command::Remove(need(args, 1, "track id")?.into()),
        "clear-queue" => Command::ClearQueue,
        "like" => Command::Like(args.get(1).cloned()),
        "nav" | "go" => Command::Nav(need(args, 1, "page")?.into()),
        "now-playing" => Command::NowPlaying(on_off(args.get(1))?),
        "library" => Command::Library(on_off(args.get(1))?),
        "queue-panel" => Command::QueuePanel(on_off(args.get(1))?),
        "escape" | "esc" => Command::Escape,
        "resize" => {
            let w: f32 = num(need(args, 1, "width")?, "width")?;
            let h: f32 = num(need(args, 2, "height")?, "height")?;
            if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
                return Err("width and height must be > 0".into());
            }
            Command::Resize(w, h)
        }
        "screenshot" => Command::Screenshot(args.get(1).map(PathBuf::from)),
        "float" => Command::Float(on_off(args.get(1))?),
        "tile" => Command::Float(Some(false)),
        "mini" => Command::Mini(
            match args.get(1).map(|s| s.to_ascii_lowercase()).as_deref() {
                None | Some("toggle") => MiniArg::Toggle,
                Some("open" | "on") => MiniArg::Open,
                Some("close" | "off") => MiniArg::Close,
                Some("restore") => MiniArg::Restore,
                Some("lyrics") => MiniArg::Lyrics(on_off(args.get(2))?),
                Some("screenshot") => MiniArg::Screenshot(args.get(2).map(PathBuf::from)),
                Some(other) => {
                    return Err(format!(
                        "unknown mini action `{other}` (open|close|restore|lyrics|screenshot)"
                    ))
                }
            },
        ),
        "quit" | "exit" => Command::Quit,
        other => return Err(format!("unknown command `{other}` (try `help`)")),
    };
    Ok(c)
}

/// Whether the app should listen: `--control` on the command line or
/// `YUTU_CONTROL` set to a truthy value.
pub fn enabled_from(args: &[String], env: Option<&str>) -> bool {
    args.iter().any(|a| a == "--control")
        || env.is_some_and(|v| !matches!(v.trim(), "" | "0" | "false" | "off"))
}

/// Socket path: `YUTU_CONTROL_SOCKET` wins, else a per-user directory
/// under `XDG_RUNTIME_DIR` (Linux) or the per-user temp dir (macOS
/// `TMPDIR` is already private), suffixed with the user name so two
/// accounts sharing `/tmp` never collide.
pub fn socket_path() -> PathBuf {
    if let Some(p) = std::env::var_os("YUTU_CONTROL_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    socket_dir().join("control.sock")
}

fn socket_dir() -> PathBuf {
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(rt).join("yutu-mux");
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    std::env::temp_dir().join(format!("yutu-mux-{user}"))
}

/// Default screenshot file when the caller names none.
pub fn default_screenshot_path() -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("yutu-mux-screenshot-{ts}.png"))
}

/// Encode one request line (no trailing newline).
pub fn request_line(args: &[String]) -> String {
    serde_json::json!({ "args": args }).to_string()
}

/// Decode one request line into its words.
pub fn decode_request(line: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("bad request json: {e}"))?;
    let arr = v
        .get("args")
        .and_then(|a| a.as_array())
        .ok_or("request needs an `args` array")?;
    arr.iter()
        .map(|a| match a {
            serde_json::Value::String(s) => Ok(s.clone()),
            serde_json::Value::Number(n) => Ok(n.to_string()),
            _ => Err("args must be strings".to_string()),
        })
        .collect()
}

/// Encode one reply line (no trailing newline).
pub fn reply_line(result: &Result<serde_json::Value, String>) -> String {
    match result {
        Ok(data) => serde_json::json!({ "ok": true, "data": data }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    }
    .to_string()
}

/// Decode one reply line back into a result.
pub fn decode_reply(line: &str) -> Result<serde_json::Value, String> {
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("bad reply json: {e}"))?;
    if v.get("ok").and_then(|o| o.as_bool()) == Some(true) {
        Ok(v.get("data").cloned().unwrap_or(serde_json::Value::Null))
    } else {
        Err(v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown error")
            .to_string())
    }
}

/// Blocking client: send one request, read one reply.
#[cfg(unix)]
pub fn call(path: &std::path::Path, args: &[String]) -> Result<serde_json::Value, String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(path).map_err(|e| {
        format!(
            "cannot reach yutu-mux at {} ({e}); start it with --control or YUTU_CONTROL=1",
            path.display()
        )
    })?;
    // Screenshots and slow pages can take a while; never hang forever.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let mut line = request_line(args);
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| format!("send failed: {e}"))?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| format!("no reply: {e}"))?;
    if reply.trim().is_empty() {
        return Err("app closed the connection without replying".into());
    }
    decode_reply(reply.trim())
}

/// Prepare the socket location before binding: a private `0700`
/// directory, and a stale socket file removed only when nothing answers
/// on it (a live instance keeps its socket: returns an error instead).
#[cfg(unix)]
pub fn prepare_socket(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        // Only tighten directories we own (the default location);
        // a custom YUTU_CONTROL_SOCKET parent is the caller's choice.
        if std::env::var_os("YUTU_CONTROL_SOCKET").is_none() {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(format!(
                "another instance already listens on {}",
                path.display()
            ));
        }
        std::fs::remove_file(path).map_err(|e| format!("remove stale socket: {e}"))?;
    }
    Ok(())
}

/// Restrict a freshly bound socket to its owner.
#[cfg(unix)]
pub fn secure_socket(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_core_commands() {
        assert_eq!(parse_command(&words("state")), Ok(Command::State));
        assert_eq!(
            parse_command(&words("search daft punk")),
            Ok(Command::Search("daft punk".into()))
        );
        assert_eq!(parse_command(&words("seek 42.5")), Ok(Command::Seek(42.5)));
        assert_eq!(
            parse_command(&words("play-result 3")),
            Ok(Command::PlayResult(3))
        );
        assert_eq!(parse_command(&words("shuffle")), Ok(Command::Shuffle(None)));
        assert_eq!(
            parse_command(&words("shuffle on")),
            Ok(Command::Shuffle(Some(true)))
        );
        assert_eq!(
            parse_command(&words("repeat one")),
            Ok(Command::Repeat(Some(RepeatArg::One)))
        );
        assert_eq!(
            parse_command(&words("resize 800 600")),
            Ok(Command::Resize(800.0, 600.0))
        );
        assert_eq!(parse_command(&words("like")), Ok(Command::Like(None)));
        assert_eq!(parse_command(&words("quit")), Ok(Command::Quit));
        assert_eq!(parse_command(&words("float")), Ok(Command::Float(None)));
        assert_eq!(
            parse_command(&words("float on")),
            Ok(Command::Float(Some(true)))
        );
        assert_eq!(
            parse_command(&words("tile")),
            Ok(Command::Float(Some(false)))
        );
        assert_eq!(
            parse_command(&words("screenshot /tmp/a.png")),
            Ok(Command::Screenshot(Some("/tmp/a.png".into())))
        );
        assert_eq!(
            parse_command(&words("mini")),
            Ok(Command::Mini(MiniArg::Toggle))
        );
        assert_eq!(
            parse_command(&words("mini lyrics on")),
            Ok(Command::Mini(MiniArg::Lyrics(Some(true))))
        );
        assert_eq!(
            parse_command(&words("mini screenshot /tmp/m.png")),
            Ok(Command::Mini(MiniArg::Screenshot(Some(
                "/tmp/m.png".into()
            ))))
        );
        assert!(parse_command(&words("mini fly")).is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_command(&[]).is_err());
        assert!(parse_command(&words("search")).is_err());
        assert!(parse_command(&words("volume 2")).is_err());
        assert!(parse_command(&words("seek -1")).is_err());
        assert!(parse_command(&words("seek nan")).is_err());
        assert!(parse_command(&words("resize 0 10")).is_err());
        assert!(parse_command(&words("shuffle maybe")).is_err());
        assert!(parse_command(&words("play")).is_err());
        assert!(parse_command(&words("frobnicate")).is_err());
    }

    #[test]
    fn wire_round_trip() {
        let args = words("seek 10");
        assert_eq!(decode_request(&request_line(&args)), Ok(args));
        // Numbers are accepted as words for hand-written clients.
        assert_eq!(
            decode_request(r#"{"args":["jump",2]}"#),
            Ok(words("jump 2"))
        );
        assert!(decode_request("{}").is_err());
        let ok = Ok(serde_json::json!({"a": 1}));
        assert_eq!(decode_reply(&reply_line(&ok)), ok);
        let err: Result<serde_json::Value, String> = Err("nope".into());
        assert_eq!(decode_reply(&reply_line(&err)), err);
    }

    #[test]
    fn opt_in_switch() {
        assert!(!enabled_from(&[], None));
        assert!(!enabled_from(&[], Some("0")));
        assert!(!enabled_from(&[], Some("")));
        assert!(enabled_from(&[], Some("1")));
        assert!(enabled_from(&words("yutu-mux --control"), None));
    }

    #[test]
    fn help_lists_every_command() {
        for w in [
            "ping",
            "state",
            "queue",
            "results",
            "playlists",
            "page",
            "search",
            "play-result",
            "play-playlist",
            "radio",
            "seek",
            "volume",
            "shuffle",
            "repeat",
            "enqueue",
            "play-next",
            "jump",
            "remove",
            "clear-queue",
            "like",
            "nav",
            "now-playing",
            "library",
            "queue-panel",
            "escape",
            "resize",
            "screenshot",
            "float",
            "mini",
            "quit",
        ] {
            assert!(HELP.contains(w), "help misses {w}");
        }
    }
}
