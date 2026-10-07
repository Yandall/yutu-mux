# PO tokens (why some streams 403)

Since Aug 2024 YouTube requires Proof-of-Origin tokens for `web*` client
streams (BotGuard attestation, per-video, short-lived). Without one, GVS
returns 403.

yutu-mux strategy (Phase 4):

1. Prefer clients that don't need PO tokens (`android_vr`, unauthenticated).
2. Fall back to `web_music` + cached token via `rustypipe-botguard` sidecar.
3. Surface an honest error with install instructions — never silent failure.

Premium subscribers are exempt from the GVS token requirement (per yt-dlp
PO-Token guide, July 2026). yutu-mux assumes free-tier, so the fallback
chain matters.
