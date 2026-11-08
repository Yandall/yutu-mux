# Auth (free-tier, no Premium assumed)

## Anonymous (default)

Search + browse via unauthenticated Innertube clients. No login, no cookies.
Some streams/playlists will 403 or be unavailable — expected.

## Cookie login (Phase 4)

1. Fresh browser private window → log into YouTube Music → export cookies
   (`Get cookies.txt LOCALLY` addon) → save as `cookies.txt`.
2. `Settings → Choose cookies.txt` opens the system file picker (portal
   on Linux, native panel on macOS), then `Import`. The session itself
   is kept in the OS keychain (`yutu-mux` / `cookie-session`, stable
   across releases), with a 0600 file fallback when the keychain is
   unreachable.
3. Close the browser window after export (prevents rotation).
4. After a successful import the app offers to delete the `cookies.txt`
   export (Delete is the CTA; Esc keeps it). The session itself is kept
   in the OS keychain (`yutu-mux` / `cookie-session`, stable across
   releases), with a 0600 file fallback when the keychain is unreachable.
   Opening the app never unlocks the keychain; the first connect, sync,
   or search after launch does (silent while the login keychain is
   unlocked, so no OS password on every launch). Sign out clears the
   keychain entry and every fallback file.

Warnings:

- Unofficial API use violates YouTube ToS. Personal/open-source use only.
- Never share `cookies.txt`, `rustypipe_cache.json`, or `reports/` payloads.
- If Google forces re-login, re-export. Logout = delete cookie store + cache.
