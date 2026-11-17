//! Hyprland window control for the control socket (`ctl float`,
//! `ctl resize`): tiled windows ignore client resize requests, so the
//! app asks the compositor through `hyprctl` instead, targeting its own
//! main window (found by process id, addressed by window address; never
//! the focused window, never the mini player).
//!
//! Rules:
//! - Active only under Hyprland (`HYPRLAND_INSTANCE_SIGNATURE` set);
//!   every call is a short blocking `hyprctl` run, so callers keep it
//!   off the UI thread.
//! - Lua configs (Hyprland 0.57+) take `hl.dsp.window.float/resize`
//!   with a `window = "address:<addr>"` selector, the same dispatcher a
//!   Super+T bind uses; older hyprlang configs fall back to the legacy
//!   `togglefloating` / `resizewindowpixel` dispatchers by address.
//!   Never `pid:`: with the mini player open the process has two
//!   windows and Hyprland may pick the wrong one.
//! - The mini player is its own window with its own class
//!   ([`MINI_CLASS`]): `ctl float` / `ctl resize` skip it, and the mini
//!   player uses [`class_window`] / [`move_window`] to keep its top edge
//!   and remember where the user left it (Wayland clients cannot read
//!   or set their own position).

/// Wayland app id (Hyprland class) of the mini player window.
pub const MINI_CLASS: &str = "yutu-mux-mini";

/// One Hyprland client (window) as `hyprctl clients -j` reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct HyprWindow {
    pub address: String,
    pub floating: bool,
    /// Top-left corner in global layout coordinates.
    pub at: (i64, i64),
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

/// Pick this process's main window out of `hyprctl clients -j`
/// output (the mini player window is skipped).
pub fn parse_window(clients_json: &str, pid: u32) -> Result<HyprWindow, String> {
    find_window(clients_json, |c| {
        c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(pid))
            && c.get("class").and_then(|k| k.as_str()) != Some(MINI_CLASS)
    })
    .ok_or_else(|| "no Hyprland window for this process".to_string())?
}

/// Pick this process's window with the given class.
pub fn parse_class_window(clients_json: &str, pid: u32, class: &str) -> Result<HyprWindow, String> {
    find_window(clients_json, |c| {
        c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(pid))
            && c.get("class").and_then(|k| k.as_str()) == Some(class)
    })
    .ok_or_else(|| format!("no Hyprland window with class {class}"))?
}

fn find_window(
    clients_json: &str,
    pick: impl Fn(&serde_json::Value) -> bool,
) -> Option<Result<HyprWindow, String>> {
    let v: serde_json::Value = match serde_json::from_str(clients_json) {
        Ok(v) => v,
        Err(e) => return Some(Err(format!("bad hyprctl json: {e}"))),
    };
    let c = v.as_array()?.iter().find(|c| pick(c))?;
    let pair = |key: &str| {
        c.get(key)
            .and_then(|s| s.as_array())
            .map(|s| {
                (
                    s.first().and_then(|x| x.as_i64()).unwrap_or(0),
                    s.get(1).and_then(|x| x.as_i64()).unwrap_or(0),
                )
            })
            .unwrap_or((0, 0))
    };
    Some(Ok(HyprWindow {
        address: match c.get("address").and_then(|a| a.as_str()) {
            Some(a) => a.to_string(),
            None => return Some(Err("window has no address".into())),
        },
        floating: c.get("floating").and_then(|f| f.as_bool()).unwrap_or(false),
        at: pair("at"),
        size: pair("size"),
    }))
}

/// This process's window with the given class (the mini player).
pub fn class_window(class: &str) -> Result<HyprWindow, String> {
    parse_class_window(&hyprctl(&["clients", "-j"])?, std::process::id(), class)
}

/// Wait (bounded) until this process's `class` window is mapped and,
/// with `size`, has that size; returns it as last seen.
pub fn wait_class_window(
    class: &str,
    size: Option<(i64, i64)>,
    timeout: std::time::Duration,
) -> Result<HyprWindow, String> {
    let until = std::time::Instant::now() + timeout;
    loop {
        let found = class_window(class);
        let done = match &found {
            // Fractional scales round the logical size: 1px slack.
            Ok(w) => {
                size.is_none_or(|s| w.size.0.abs_diff(s.0) <= 1 && w.size.1.abs_diff(s.1) <= 1)
            }
            Err(_) => false,
        };
        if done || std::time::Instant::now() >= until {
            return found;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Lua expression for an absolute move of one window by address.
pub fn move_lua(address: &str, x: i64, y: i64) -> String {
    format!("hl.dsp.window.move({{ window = \"address:{address}\", x = {x}, y = {y} }})")
}

/// Lua expression focusing one window by address (switches to its
/// workspace when it sits on another one).
pub fn focus_lua(address: &str) -> String {
    format!("hl.dsp.focus({{ window = \"address:{address}\" }})")
}

/// Focus this process's main window. Hyprland ignores a client's own
/// activation request unless `misc:focus_on_activate` is on, so the
/// mini player's Open asks the compositor directly.
pub fn focus_main() -> Result<(), String> {
    let w = own_window()?;
    let target = format!("address:{}", w.address);
    dispatch(&focus_lua(&w.address), &["focuswindow", &target])
}

/// Move a window (by address) to an absolute layout position.
pub fn move_window(address: &str, x: i64, y: i64) -> Result<(), String> {
    let arg = format!("exact {x} {y},address:{address}");
    dispatch(&move_lua(address, x, y), &["movewindowpixel", &arg])
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
pub fn float_lua(address: &str, action: &str) -> String {
    format!("hl.dsp.window.float({{ window = \"address:{address}\", action = \"{action}\" }})")
}

/// Lua expression for an exact resize of one window.
pub fn resize_lua(address: &str, width: u32, height: u32) -> String {
    format!("hl.dsp.window.resize({{ window = \"address:{address}\", x = {width}, y = {height} }})")
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
    dispatch(&float_lua(&w.address, action), &["togglefloating", &target])?;
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
        &resize_lua(&w.address, width, height),
        &["resizewindowpixel", &arg],
    )?;
    own_window()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = r#"[
        {"address":"0x1","pid":10,"floating":false,"size":[800,600]},
        {"address":"0x3","pid":42,"class":"yutu-mux-mini","floating":true,
         "at":[1500,60],"size":[380,112]},
        {"address":"0x2","pid":42,"class":"yutu-mux","floating":true,
         "at":[0,0],"size":[1280,720]}
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
    fn mini_window_is_found_by_class_and_skipped_for_main() {
        let m = parse_class_window(CLIENTS, 42, MINI_CLASS).unwrap();
        assert_eq!(m.address, "0x3");
        assert_eq!(m.at, (1500, 60));
        assert_eq!(m.size, (380, 112));
        assert!(parse_class_window(CLIENTS, 10, MINI_CLASS).is_err());
        assert_eq!(
            focus_lua("0x2"),
            r#"hl.dsp.focus({ window = "address:0x2" })"#
        );
        assert_eq!(
            move_lua("0x3", 10, 20),
            r#"hl.dsp.window.move({ window = "address:0x3", x = 10, y = 20 })"#
        );
    }

    #[test]
    fn lua_dispatchers_target_one_window_by_address() {
        assert_eq!(
            float_lua("0x2", "on"),
            r#"hl.dsp.window.float({ window = "address:0x2", action = "on" })"#
        );
        assert_eq!(
            resize_lua("0x2", 900, 700),
            r#"hl.dsp.window.resize({ window = "address:0x2", x = 900, y = 700 })"#
        );
    }
}
