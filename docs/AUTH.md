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

## Premium audio (256 kbps)

Signed in with a YouTube Premium account, each song is first resolved
as that account through yt-dlp's `web_music` client, which serves the
256 kbps AAC stream (itag 141). yt-dlp needs a JavaScript runtime for
this client (`deno` is enabled by default) to solve the player's
challenges; the `tv` player variant is pinned first (the solver can lag
behind yt-dlp's default `main` variant), then yt-dlp's default. A song
that is unavailable for the account does not pause Premium for others.
Free accounts, a missing runtime or an expired session simply fall back
to the standard 128 kbps path; the signed-in attempt then pauses for 30 minutes so songs do not
pay for a failing lookup each time. The log line `signed-in audio for
<id>: format 141` confirms Premium quality.

How the session reaches yt-dlp (it only reads cookies from a file):

- Read from the keychain on the resolver thread for each lookup; never
  kept in app state, never on the command line (`ps` shows it to every
  user) and never in environment variables.
- Written to a fresh, randomly named directory created `0700` with a
  `0600` file (`O_EXCL`, so a planted path or symlink is never used),
  in `$XDG_RUNTIME_DIR` (per-user tmpfs, never on disk) when it is
  private, else the OS temp dir.
- Every row is limited to `.youtube.com` and `secure`, with a one-hour
  expiry, so yt-dlp cannot send the session to any other host.
- Deleted as soon as the yt-dlp run ends; jars left by a crash are
  swept after an hour. The stream URL it returns must still be a
  `googlevideo.com` URL, and playback itself sends no cookies.

Warnings:

- Unofficial API use violates YouTube ToS. Personal/open-source use only.
- Never share `cookies.txt`, `rustypipe_cache.json`, or `reports/` payloads.
- If Google forces re-login, re-export. Logout = delete cookie store + cache.
