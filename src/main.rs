use yutu_core::{MockBackend, MusicBackend};
use yutu_player::{AudioBackend, Queue, StubBackend};

#[tokio::main]
async fn main() {
    println!("yutu-mux 0.1.0 — native YT Music (Iced coming in Phase 3)");
    // Phase 0 smoke: core + player wire up without network/audio.
    let backend = MockBackend::new();
    let results = backend.search("beatles");
    println!("mock search hits: {}", results.tracks.len());
    let mut q = Queue::default();
    for t in results.tracks.iter().take(3) {
        q.push(t.clone());
    }
    let stub = StubBackend;
    println!("queue len={} stub backend ok={}", q.len(), stub.is_ok());
    let ui_state = yutu_ui::AppState::new();
    println!("ui state ok: {:?}", ui_state.route);
}
