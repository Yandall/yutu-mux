//! Agent/script control bridge: executes `yutu_core::control` commands
//! against the live app. Every action goes through the same `Message`
//! the UI sends (no side doors), so behavioral contracts (same-song
//! no-op, queue replacement, seek holds) hold for scripted input too.
//!
//! Rules:
//! - Opt-in (`--control` / `YUTU_CONTROL=1`): the listener subscription
//!   only exists when `control_enabled` is set at startup.
//! - The socket task (Unix only) forwards each parsed command as
//!   `Message::Control` and waits for the handler's reply; screenshots
//!   come back as raw RGBA and are PNG-encoded off the UI thread.

use super::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use yutu_core::control::{Command, RepeatArg};
use yutu_core::Track;

/// What the handler sends back to the socket task.
pub enum ControlReply {
    Json(Result<Value, String>),
    /// Raw window capture, encoded by the socket task.
    Screenshot {
        shot: iced::window::Screenshot,
        path: std::path::PathBuf,
    },
}

/// One-shot reply channel carried inside a (cloneable) `Message`.
#[derive(Clone)]
pub struct ReplySlot(Arc<Mutex<Option<futures::channel::oneshot::Sender<ControlReply>>>>);

impl std::fmt::Debug for ReplySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplySlot")
    }
}

impl ReplySlot {
    pub fn new(tx: futures::channel::oneshot::Sender<ControlReply>) -> Self {
        Self(Arc::new(Mutex::new(Some(tx))))
    }

    /// First reply wins; later sends are dropped.
    pub fn send(&self, reply: ControlReply) {
        let tx = self
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(tx) = tx {
            let _ = tx.send(reply);
        }
    }

    fn ok(&self, v: Value) {
        self.send(ControlReply::Json(Ok(v)));
    }

    fn err(&self, e: impl Into<String>) {
        self.send(ControlReply::Json(Err(e.into())));
    }
}

/// A command from the control socket plus where its answer goes.
#[derive(Debug, Clone)]
pub struct ControlCall {
    pub command: Command,
    pub reply: ReplySlot,
}

fn track_json(t: &Track) -> Value {
    json!({
        "id": t.id,
        "title": t.title,
        "artist": t.artist,
        "album": t.album,
        "duration_secs": t.duration_secs,
        "is_video": t.is_video,
    })
}

fn tracks_json(tracks: &[Track]) -> Value {
    Value::Array(
        tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut v = track_json(t);
                v["index"] = json!(i);
                v
            })
            .collect(),
    )
}

/// Route as one word, the same shape `nav` accepts.
pub(crate) fn route_word(r: &Route) -> String {
    match r {
        Route::Home => "home".into(),
        Route::Search => "search".into(),
        Route::History => "history".into(),
        Route::Discovery => "discovery".into(),
        Route::Settings => "settings".into(),
        Route::Artist(id) => format!("artist:{id}"),
        Route::Album(id) => format!("album:{id}"),
        Route::PlaylistDetail(id) => format!("playlist:{id}"),
        Route::RemotePlaylist(id) => format!("remote-playlist:{id}"),
        Route::Lyrics(id) => format!("lyrics:{id}"),
    }
}

impl AppState {
    /// Search-page tracks for the active filter chip.
    fn control_search_tracks(&self) -> &[Track] {
        if self.search_filter == SearchFilter::All && !self.all_results.tracks.is_empty() {
            &self.all_results.tracks
        } else {
            &self.results.tracks
        }
    }

    /// True while something the user would wait on is in flight.
    pub(crate) fn control_busy(&self) -> bool {
        self.searching
            || self.loading_track.is_some()
            || self.artist_loading.is_some()
            || self.album_loading.is_some()
            || self.artist_resolving.is_some()
            || self.album_resolving.is_some()
            || self.remote_playlist_loading.is_some()
    }

    pub(crate) fn control_state(&self) -> Value {
        let now = self.queue.current();
        let layout = self.layout();
        json!({
            "route": route_word(&self.route),
            "busy": self.control_busy(),
            "status": self.status,
            "window": self.window.map(|s| json!({"width": s.width, "height": s.height})),
            "layout": format!("{layout:?}").to_ascii_lowercase(),
            "now_playing": now.map(|t| {
                let mut v = track_json(t);
                v["liked"] = json!(self.liked.contains(&t.id));
                v["duration_secs"] = json!(self.track_secs(t));
                v
            }),
            "paused": self.paused,
            "loading_track": self.loading_track,
            "position_secs": self.position_secs,
            "volume": self.volume,
            "muted": self.muted,
            "shuffle": self.queue.shuffle,
            "repeat": format!("{:?}", self.queue.repeat).to_ascii_lowercase(),
            "queue": {
                "index": self.queue.position(),
                "len": self.queue.len(),
                "autoplay_len": self.autoplay.len(),
            },
            "search": {
                "query": self.search_query,
                "searching": self.searching,
                "filter": format!("{:?}", self.search_filter).to_ascii_lowercase(),
                "tracks": self.control_search_tracks().len(),
            },
            "panels": {
                "now_playing": self.drawer_open,
                "library_pinned": self.sidebar_pinned,
                "library_open": self.sidebar_latched,
                "queue_pinned": self.right_pinned,
                "queue_open": self.right_latched,
            },
            "overlays": {
                "context_menu": self.context_menu,
                "new_playlist_dialog": self.new_playlist.is_some(),
                "delete_confirm": self.pending_playlist_delete,
                "rename_dialog": self.pending_playlist_rename,
            },
        })
    }

    fn control_page(&self) -> Value {
        let tracks: Vec<Track> = match &self.route {
            Route::Search => self.control_search_tracks().to_vec(),
            Route::PlaylistDetail(id) => self
                .find_playlist(id)
                .map(|p| self.playlist_ordered_tracks(&p))
                .unwrap_or_default(),
            Route::RemotePlaylist(id) => self.remote_playlist_tracks(id),
            Route::Artist(id) => self.artist_display_tracks(id),
            Route::Album(id) => self.album_display_tracks(id),
            Route::History => self
                .history_full
                .iter()
                .filter_map(|e| self.find_track(&e.track_id))
                .take(self.history_limit)
                .collect(),
            Route::Home => self.home_pick_tracks(),
            _ => Vec::new(),
        };
        json!({ "route": route_word(&self.route), "tracks": tracks_json(&tracks) })
    }

    fn control_results(&self) -> Value {
        let r = &self.all_results;
        json!({
            "query": self.search_query,
            "searching": self.searching,
            "tracks": tracks_json(self.control_search_tracks()),
            "albums": r.albums.iter().chain(&self.results.albums).map(|a| json!({
                "id": a.id, "title": a.title, "artist": a.artist, "year": a.year,
            })).collect::<Vec<_>>(),
            "artists": r.artists.iter().chain(&self.results.artists).map(|a| json!({
                "id": a.id, "name": a.name,
            })).collect::<Vec<_>>(),
            "playlists": r.playlists.iter().chain(&self.results.playlists).map(|p| json!({
                "id": p.id, "title": p.title, "author": p.author,
            })).collect::<Vec<_>>(),
        })
    }

    fn control_playlists(&self) -> Value {
        let mut out = vec![json!({
            "id": Self::FAVORITES_ID,
            "title": "Favorites",
            "tracks": self.favorite_tracks().len(),
        })];
        out.extend(
            self.user_playlists
                .iter()
                .map(|p| json!({ "id": p.id, "title": p.title, "tracks": p.tracks.len() })),
        );
        Value::Array(out)
    }

    fn control_queue(&self) -> Value {
        json!({
            "index": self.queue.position(),
            "items": tracks_json(self.queue.items()),
            "autoplay": tracks_json(&self.autoplay),
        })
    }

    /// Short post-action summary so callers see the effect at once.
    fn control_summary(&self) -> Value {
        json!({
            "route": route_word(&self.route),
            "busy": self.control_busy(),
            "now_playing": self.queue.current().map(track_json),
            "paused": self.paused,
            "status": self.status,
        })
    }

    /// Run UI messages in order, then reply with the summary.
    fn control_run(&mut self, msgs: Vec<Message>, reply: &ReplySlot) -> iced::Task<Message> {
        let tasks: Vec<_> = msgs.into_iter().map(|m| self.update(m)).collect();
        reply.ok(self.control_summary());
        iced::Task::batch(tasks)
    }

    fn control_nav(&mut self, page: &str) -> Result<Vec<Message>, String> {
        let (kind, id) = match page.split_once(':') {
            Some((k, id)) if !id.is_empty() => (k, Some(id.to_string())),
            _ => (page, None),
        };
        Ok(match (kind, id) {
            ("home", None) => vec![Message::Navigate(Route::Home)],
            ("search", None) => vec![Message::Navigate(Route::Search)],
            ("history", None) => vec![Message::Navigate(Route::History)],
            ("discovery", None) => vec![Message::Navigate(Route::Discovery)],
            ("settings", None) => vec![Message::Navigate(Route::Settings)],
            ("back", None) => vec![Message::NavBack],
            ("forward", None) => vec![Message::NavForward],
            ("artist", Some(id)) => vec![Message::OpenArtist(id)],
            ("album", Some(id)) => vec![Message::OpenAlbum(id)],
            ("playlist", Some(id)) => {
                if self.find_playlist(&id).is_some() {
                    vec![Message::OpenPlaylist(id)]
                } else {
                    vec![Message::OpenSearchPlaylist(id)]
                }
            }
            ("remote-playlist", Some(id)) => vec![Message::OpenSearchPlaylist(id)],
            ("lyrics", Some(id)) => vec![Message::OpenLyrics(id)],
            _ => return Err(format!("unknown page `{page}`")),
        })
    }

    /// Execute one control command (see `yutu_core::control::HELP`).
    pub(crate) fn control(&mut self, call: ControlCall) -> iced::Task<Message> {
        let ControlCall { command, reply } = call;
        let msgs: Vec<Message> = match command {
            Command::Ping => {
                reply.ok(json!({
                    "app": "yutu-mux",
                    "version": env!("CARGO_PKG_VERSION"),
                    "route": route_word(&self.route),
                }));
                return iced::Task::none();
            }
            Command::State => {
                reply.ok(self.control_state());
                return iced::Task::none();
            }
            Command::Queue => {
                reply.ok(self.control_queue());
                return iced::Task::none();
            }
            Command::Results => {
                reply.ok(self.control_results());
                return iced::Task::none();
            }
            Command::Playlists => {
                reply.ok(self.control_playlists());
                return iced::Task::none();
            }
            Command::Page => {
                reply.ok(self.control_page());
                return iced::Task::none();
            }
            // Typing then Enter: lands on the Search page like the header box.
            Command::Search(q) => vec![Message::SearchChanged(q), Message::SearchStay],
            Command::Play(id) => vec![Message::PlayNow(id)],
            Command::PlayResult(n) => match self.control_search_tracks().get(n) {
                Some(t) => vec![Message::PlayNow(t.id.clone())],
                None => {
                    reply.err(format!(
                        "no search result {n} ({} results)",
                        self.control_search_tracks().len()
                    ));
                    return iced::Task::none();
                }
            },
            Command::PlayPlaylist(id) => {
                if self.find_playlist(&id).is_some() {
                    vec![Message::PlayPlaylist(id)]
                } else {
                    vec![Message::PlaySearchPlaylist(id)]
                }
            }
            Command::Radio(id) => vec![Message::StartRadio(id)],
            Command::Pause => {
                if self.paused {
                    vec![]
                } else {
                    vec![Message::Pause]
                }
            }
            Command::Resume => {
                if self.paused || self.restored_pending {
                    vec![Message::TogglePause]
                } else {
                    vec![]
                }
            }
            Command::Toggle => vec![Message::TogglePause],
            Command::Stop => vec![Message::Stop],
            Command::Next => vec![Message::Next],
            Command::Prev => vec![Message::Prev],
            Command::Seek(s) => vec![Message::SeekTo(s as f32)],
            Command::Volume(v) => vec![Message::SetVolume(v)],
            Command::Mute => vec![Message::ToggleMute],
            Command::Shuffle(want) => {
                if want.is_some_and(|w| w == self.queue.shuffle) {
                    vec![]
                } else {
                    vec![Message::ToggleShuffle]
                }
            }
            Command::Repeat(None) => vec![Message::CycleRepeat],
            Command::Repeat(Some(want)) => {
                let target = match want {
                    RepeatArg::Off => yutu_player::RepeatMode::Off,
                    RepeatArg::All => yutu_player::RepeatMode::All,
                    RepeatArg::One => yutu_player::RepeatMode::One,
                };
                // Cycle order is Off -> All -> One -> Off: at most two
                // steps away from any mode.
                let next = |m| match m {
                    yutu_player::RepeatMode::Off => yutu_player::RepeatMode::All,
                    yutu_player::RepeatMode::All => yutu_player::RepeatMode::One,
                    yutu_player::RepeatMode::One => yutu_player::RepeatMode::Off,
                };
                let mut msgs = Vec::new();
                let mut probe = self.queue.repeat;
                while probe != target {
                    probe = next(probe);
                    msgs.push(Message::CycleRepeat);
                }
                msgs
            }
            Command::Enqueue(id) => vec![Message::QueueTrack(id)],
            Command::PlayNext(id) => vec![Message::PlayNext(id)],
            Command::Jump(i) => {
                if i >= self.queue.len() {
                    reply.err(format!("no queue index {i} ({} items)", self.queue.len()));
                    return iced::Task::none();
                }
                vec![Message::QueueJump(i)]
            }
            Command::Remove(id) => vec![Message::RemoveFromQueue(id)],
            Command::ClearQueue => vec![Message::QueueClear],
            Command::Like(id) => match id.or_else(|| self.queue.current().map(|t| t.id.clone())) {
                Some(id) => vec![Message::Like(id)],
                None => {
                    reply.err("nothing is playing; pass a track id");
                    return iced::Task::none();
                }
            },
            Command::Nav(page) => match self.control_nav(&page) {
                Ok(m) => m,
                Err(e) => {
                    reply.err(e);
                    return iced::Task::none();
                }
            },
            Command::NowPlaying(want) => {
                if want.is_some_and(|w| w == self.drawer_open) {
                    vec![]
                } else {
                    vec![Message::ToggleDrawer]
                }
            }
            Command::Library(want) => {
                let narrow = self.layout() == LayoutMode::Narrow;
                let open = if narrow {
                    self.sidebar_latched
                } else {
                    self.sidebar_pinned
                };
                if want.is_some_and(|w| w == open) {
                    vec![]
                } else if narrow {
                    vec![Message::ToggleLibraryDrawer]
                } else {
                    vec![Message::ToggleSidebarPin]
                }
            }
            Command::QueuePanel(want) => {
                let narrow = self.layout() == LayoutMode::Narrow;
                let open = if narrow {
                    self.right_latched
                } else {
                    self.right_pinned
                };
                if want.is_some_and(|w| w == open) {
                    vec![]
                } else if narrow {
                    vec![Message::ToggleQueueDrawer]
                } else {
                    vec![Message::ToggleRightPin]
                }
            }
            Command::Escape => vec![Message::CloseContext],
            Command::Resize(w, h) => {
                let size = iced::Size::new(w.max(MIN_WINDOW_W), h.max(MIN_WINDOW_H));
                // Hyprland owns tiled sizes: resize through the
                // compositor (floating windows only, it says so).
                #[cfg(target_os = "linux")]
                if yutu_desktop::hyprland::available() {
                    let (w, h) = (size.width.round() as u32, size.height.round() as u32);
                    return control_blocking(reply, move || {
                        yutu_desktop::hyprland::resize(w, h).map(|win| {
                            json!({
                                "width": win.size.0,
                                "height": win.size.1,
                                "floating": win.floating,
                            })
                        })
                    });
                }
                reply.ok(json!({ "width": size.width, "height": size.height }));
                return iced::window::latest().then(move |id| match id {
                    Some(id) => iced::window::resize(id, size),
                    None => iced::Task::none(),
                });
            }
            Command::Float(want) => {
                #[cfg(target_os = "linux")]
                if yutu_desktop::hyprland::available() {
                    return control_blocking(reply, move || {
                        yutu_desktop::hyprland::set_floating(want)
                            .map(|floating| json!({ "floating": floating }))
                    });
                }
                let _ = want;
                reply.err("float needs a Hyprland session");
                return iced::Task::none();
            }
            Command::Quit => {
                // Same savers a manual quit relies on; a failed save
                // keeps the app open instead of losing the session.
                if !(self.persist_eq() && self.persist_queue()) {
                    reply.err("session save failed; the app stays open");
                    return iced::Task::none();
                }
                reply.ok(json!({ "quitting": true }));
                #[cfg(unix)]
                let _ = std::fs::remove_file(yutu_core::control::socket_path());
                // A short grace so the reply line reaches the client.
                return iced::Task::future(tokio::time::sleep(std::time::Duration::from_millis(
                    200,
                )))
                .then(|_| iced::exit());
            }
            Command::Screenshot(path) => {
                let path = path.unwrap_or_else(yutu_core::control::default_screenshot_path);
                return iced::window::latest().then(move |id| {
                    let reply = reply.clone();
                    let path = path.clone();
                    match id {
                        Some(id) => iced::window::screenshot(id).map(move |shot| {
                            reply.send(ControlReply::Screenshot {
                                shot,
                                path: path.clone(),
                            });
                            Message::NoOp
                        }),
                        None => {
                            reply.err("no window to capture");
                            iced::Task::none()
                        }
                    }
                });
            }
        };
        self.control_run(msgs, &reply)
    }
}

/// Run a blocking helper (e.g. `hyprctl`) off the UI thread and reply
/// with its result.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn control_blocking(
    reply: ReplySlot,
    f: impl FnOnce() -> Result<Value, String> + Send + 'static,
) -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(f)
                .await
                .unwrap_or_else(|e| Err(format!("worker crashed: {e}")))
        },
        move |r| {
            reply.send(ControlReply::Json(r));
            Message::NoOp
        },
    )
}

/// Socket listener as an Iced subscription (Unix only).
#[cfg(unix)]
pub(crate) fn control_subscription() -> iced::Subscription<Message> {
    iced::Subscription::run(server::stream)
}

#[cfg(not(unix))]
pub(crate) fn control_subscription() -> iced::Subscription<Message> {
    yutu_core::app_log::warn("control", "the control socket is Unix-only");
    iced::Subscription::none()
}

#[cfg(unix)]
mod server {
    use super::{ControlCall, ControlReply, Message, ReplySlot};
    use futures::channel::mpsc::Sender;
    use futures::SinkExt;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use yutu_core::control as proto;

    pub(super) fn stream() -> impl futures::Stream<Item = Message> {
        iced::stream::channel(16, async move |out: Sender<Message>| {
            let path = proto::socket_path();
            if let Err(e) = proto::prepare_socket(&path) {
                yutu_core::app_log::warn("control", format_args!("not listening: {e}"));
                return;
            }
            let listener = match tokio::net::UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    yutu_core::app_log::warn(
                        "control",
                        format_args!("bind {}: {e}", path.display()),
                    );
                    return;
                }
            };
            proto::secure_socket(&path);
            yutu_core::app_log::info("control", format_args!("listening on {}", path.display()));
            loop {
                match listener.accept().await {
                    Ok((conn, _)) => {
                        tokio::spawn(serve(conn, out.clone()));
                    }
                    Err(e) => {
                        yutu_core::app_log::warn("control", format_args!("accept: {e}"));
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                }
            }
        })
    }

    async fn serve(conn: tokio::net::UnixStream, mut out: Sender<Message>) {
        let (r, mut w) = conn.into_split();
        let mut lines = BufReader::new(r).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let result = handle(&line, &mut out).await;
            let mut reply = proto::reply_line(&result);
            reply.push('\n');
            if w.write_all(reply.as_bytes()).await.is_err() {
                break;
            }
        }
    }

    async fn handle(line: &str, out: &mut Sender<Message>) -> Result<Value, String> {
        let args = proto::decode_request(line)?;
        if args.first().map(String::as_str) == Some("help") {
            return Ok(Value::String(proto::HELP.into()));
        }
        let command = proto::parse_command(&args)?;
        let (tx, rx) = futures::channel::oneshot::channel();
        out.send(Message::Control(ControlCall {
            command,
            reply: ReplySlot::new(tx),
        }))
        .await
        .map_err(|_| "app is shutting down".to_string())?;
        let reply = tokio::time::timeout(std::time::Duration::from_secs(25), rx)
            .await
            .map_err(|_| "app did not answer in time".to_string())?
            .map_err(|_| "app dropped the request".to_string())?;
        match reply {
            ControlReply::Json(r) => r,
            ControlReply::Screenshot { shot, path } => {
                tokio::task::spawn_blocking(move || save_png(shot, path))
                    .await
                    .map_err(|e| format!("encoder crashed: {e}"))?
            }
        }
    }

    fn save_png(shot: iced::window::Screenshot, path: std::path::PathBuf) -> Result<Value, String> {
        let (w, h) = (shot.size.width, shot.size.height);
        let img = image::RgbaImage::from_raw(w, h, shot.rgba.to_vec())
            .ok_or("capture size does not match its pixels")?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        }
        img.save_with_format(&path, image::ImageFormat::Png)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        Ok(json!({
            "path": path.display().to_string(),
            "width": w,
            "height": h,
            "scale_factor": shot.scale_factor,
        }))
    }
}
