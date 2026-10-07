# Auth (free-tier, no Premium assumed)

## Anonymous (default)

Search + browse via unauthenticated Innertube clients. No login, no cookies.
Some streams/playlists will 403 or be unavailable — expected.

## Cookie login (Phase 4)

1. Fresh browser private window → log into YouTube Music → export cookies
   (`Get cookies.txt LOCALLY` addon) → save as `cookies.txt`.
2. `Settings → Import cookies.txt` (wired in Phase 4; stored locally, never logged).
3. Close the browser window after export (prevents rotation).

Warnings:

- Unofficial API use violates YouTube ToS. Personal/open-source use only.
- Never share `cookies.txt`, `rustypipe_cache.json`, or `reports/` payloads.
- If Google forces re-login, re-export. Logout = delete cookie store + cache.
