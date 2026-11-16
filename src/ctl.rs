//! `yutu-mux ctl ...`: thin client for the running app's control socket
//! (`yutu_core::control`). Prints JSON on stdout, errors on stderr,
//! exit code 0 on success, 1 on failure, 2 on usage errors.

use yutu_core::control as proto;

struct Opts {
    wait: bool,
    timeout_secs: f64,
    socket: std::path::PathBuf,
    words: Vec<String>,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut opts = Opts {
        wait: false,
        timeout_secs: 20.0,
        socket: proto::socket_path(),
        words: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--wait" => opts.wait = true,
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs seconds")?;
                opts.timeout_secs = v.parse().map_err(|_| format!("invalid --timeout `{v}`"))?;
            }
            "--socket" => {
                opts.socket = it.next().ok_or("--socket needs a path")?.into();
            }
            _ => opts.words.push(a.clone()),
        }
    }
    Ok(opts)
}

fn print(v: &serde_json::Value) {
    match v {
        serde_json::Value::String(s) => println!("{s}"),
        other => println!(
            "{}",
            serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string())
        ),
    }
}

/// Poll `state` until the app reports `busy: false` (or time runs out).
#[cfg(unix)]
fn wait_idle(socket: &std::path::Path, timeout_secs: f64) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout_secs);
    // A short grace so async work kicked off by the command registers.
    std::thread::sleep(std::time::Duration::from_millis(300));
    loop {
        let state = proto::call(socket, &["state".to_string()])?;
        if state.get("busy").and_then(|b| b.as_bool()) != Some(true) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("still busy after {timeout_secs}s"));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(unix)]
pub fn run(args: &[String]) -> i32 {
    let mut opts = match parse_opts(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("yutu-mux ctl: {e}");
            return 2;
        }
    };
    match opts.words.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => {
            println!("{}", proto::HELP);
            return if opts.words.is_empty() { 2 } else { 0 };
        }
        _ => {}
    }
    // Validate locally first: usage errors never need the app.
    if let Err(e) = proto::parse_command(&opts.words) {
        eprintln!("yutu-mux ctl: {e}");
        return 2;
    }
    // Screenshot paths are relative to the caller, not the app.
    if opts.words[0] == "screenshot" {
        if let Some(p) = opts.words.get_mut(1) {
            let path = std::path::Path::new(p.as_str());
            if path.is_relative() {
                if let Ok(cwd) = std::env::current_dir() {
                    *p = cwd.join(path).display().to_string();
                }
            }
        }
    }
    let result = proto::call(&opts.socket, &opts.words).and_then(|reply| {
        if !opts.wait {
            return Ok(reply);
        }
        wait_idle(&opts.socket, opts.timeout_secs)?;
        // After waiting, show what the wait was for.
        let follow = if opts.words[0] == "search" {
            "results"
        } else {
            "state"
        };
        proto::call(&opts.socket, &[follow.to_string()])
    });
    match result {
        Ok(v) => {
            print(&v);
            0
        }
        Err(e) => {
            eprintln!("yutu-mux ctl: {e}");
            1
        }
    }
}

#[cfg(not(unix))]
pub fn run(_args: &[String]) -> i32 {
    eprintln!("yutu-mux ctl: the control socket is only available on Linux and macOS");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_flags_split_from_words() {
        let args: Vec<String> = ["--wait", "search", "a", "--timeout", "5", "b"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = parse_opts(&args).unwrap();
        assert!(o.wait);
        assert_eq!(o.timeout_secs, 5.0);
        assert_eq!(o.words, vec!["search", "a", "b"]);
        assert!(parse_opts(&["--timeout".to_string()]).is_err());
    }
}
