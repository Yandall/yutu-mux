//! Hyprland window control for the control socket (`ctl float`,
//! `ctl resize`): tiled windows ignore client resize requests, so the
//! app asks the compositor through `hyprctl` instead, targeting its own
//! window by process id (never the focused window).
//!
//! Rules:
//! - Active only under Hyprland (`HYPRLAND_INSTANCE_SIGNATURE` set);
//!   every call is a short blocking `hyprctl` run, so callers keep it
//!   off the UI thread.
//! - Lua configs (Hyprland 0.57+) take `hl.dsp.window.float/resize`
//!   with a `window = "pid:<pid>"` selector, the same dispatcher a
//!   Super+T bind uses; older hyprlang configs fall back to the legacy
//!   `togglefloating` / `resizewindowpixel` dispatchers by address.

/// One Hyprland client (window) as `hyprctl clients -j` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct HyprWindow {
    pub address: String,
    pub floating: bool,
    pub size: (i64, i64),
}

/// True when running inside a Hyprland session.
pub fn available() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty())
}

fn hyprctl(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("hyprctl")
        .args(args)
        .output()
        .map_err(|e| format!("hyprctl unavailable: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // `dispatch` prints `ok` on success and an error text otherwise.
    if !out.status.success() || (args.first() == Some(&"dispatch") && text != "ok") {
        return Err(format!("hyprctl {}: {text}", args.join(" ")));
    }
    Ok(text)
}

/// Pick this process's window out of `hyprctl clients -j` output.
pub fn parse_window(clients_json: &str, pid: u32) -> Result<HyprWindow, String> {
    let v: serde_json::Value =
        serde_json::from_str(clients_json).map_err(|e| format!("bad hyprctl json: {e}"))?;
    let c = v
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|c| c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(pid)))
        })
        .ok_or("no Hyprland window for this process")?;
    let size = c
        .get("size")
        .and_then(|s| s.as_array())
        .map(|s| {
            (
                s.first().and_then(|x| x.as_i64()).unwrap_or(0),
                s.get(1).and_then(|x| x.as_i64()).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    Ok(HyprWindow {
        address: c
            .get("address")
            .and_then(|a| a.as_str())
            .ok_or("window has no address")?
            .to_string(),
        floating: c.get("floating").and_then(|f| f.as_bool()).unwrap_or(false),
        size,
    })
}

/// This process's window.
pub fn own_window() -> Result<HyprWindow, String> {
    parse_window(&hyprctl(&["clients", "-j"])?, std::process::id())
}

/// Lua dispatch (current syntax) first, legacy dispatcher second.
fn dispatch(lua: &str, legacy: &[&str]) -> Result<(), String> {
    match hyprctl(&["dispatch", lua]) {
        Ok(_) => Ok(()),
        Err(lua_err) => {
            let mut args = vec!["dispatch"];
            args.extend_from_slice(legacy);
            hyprctl(&args).map(|_| ()).map_err(|_| lua_err)
        }
    }
}

/// Lua expression for a floating change on one window.
pub fn float_lua(pid: u32, action: &str) -> String {
    format!("hl.dsp.window.float({{ window = \"pid:{pid}\", action = \"{action}\" }})")
}

/// Lua expression for an exact resize of one window.
pub fn resize_lua(pid: u32, width: u32, height: u32) -> String {
    format!("hl.dsp.window.resize({{ window = \"pid:{pid}\", x = {width}, y = {height} }})")
}

/// Set (or with `None` toggle) floating; returns the new floating state.
pub fn set_floating(want: Option<bool>) -> Result<bool, String> {
    let w = own_window()?;
    if want == Some(w.floating) {
        return Ok(w.floating);
    }
    let action = match want {
        Some(true) => "on",
        Some(false) => "off",
        None => "toggle",
    };
    let target = format!("address:{}", w.address);
    dispatch(
        &float_lua(std::process::id(), action),
        &["togglefloating", &target],
    )?;
    Ok(!w.floating)
}

/// Resize this process's window (logical px). Tiled windows refuse:
/// the layout owns their size.
pub fn resize(width: u32, height: u32) -> Result<HyprWindow, String> {
    let w = own_window()?;
    if !w.floating {
        return Err(
            "the window is tiled in Hyprland: run `ctl float on` (or Super+T) first".into(),
        );
    }
    let arg = format!("exact {width} {height},address:{}", w.address);
    dispatch(
        &resize_lua(std::process::id(), width, height),
        &["resizewindowpixel", &arg],
    )?;
    own_window()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = r#"[
        {"address":"0x1","pid":10,"floating":false,"size":[800,600]},
        {"address":"0x2","pid":42,"floating":true,"size":[1280,720]}
    ]"#;

    #[test]
    fn finds_own_window_by_pid() {
        let w = parse_window(CLIENTS, 42).unwrap();
        assert_eq!(w.address, "0x2");
        assert!(w.floating);
        assert_eq!(w.size, (1280, 720));
        assert!(parse_window(CLIENTS, 7).is_err());
        assert!(parse_window("nope", 42).is_err());
    }

    #[test]
    fn lua_dispatchers_target_own_pid() {
        assert_eq!(
            float_lua(42, "on"),
            r#"hl.dsp.window.float({ window = "pid:42", action = "on" })"#
        );
        assert_eq!(
            resize_lua(42, 900, 700),
            r#"hl.dsp.window.resize({ window = "pid:42", x = 900, y = 700 })"#
        );
    }
}
