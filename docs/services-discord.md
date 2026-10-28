# Discord DumbBridge

The companion listener is opt-in. Forward only `/interactions` through your own
HTTPS proxy. Native Ed25519 verifies the exact timestamp/body, five-minute
freshness, application identity, allowed users, channels and servers. Phone
routes retain separate authentication and origin checks. Settings show listener
state and the last signed check for the current application/public key; this
does not establish command registration or delivery. OpenSSL is unnecessary.
Explicit registration upserts two named commands without removing others, using
your encrypted bot token. Nothing registers or listens at startup.

Use the message menu's **DumbBridge**, or `/dumbbridge message:` with an optional
attachment. Replies are private and suppress mentions. Buttons bind to the exact
draft version, application, actor, channel and server. Choose registered projects
and exact T3 thread IDs; thread choices include installation identity. Stash
defaults to **unsorted**, never the desktop's active context. A stashed draft
keeps its project. Durable capture/update keys preserve source message URL/ID,
author/channel metadata, edited fields and app-owned evidence IDs. Stash neither
sends to T3 nor accepts the itch. Serialized evidence is checked before capture.
An interrupted stash can be edited and recovered with its original capture
identity. Recovery appends provenance using the current itch version and
preserves intervening native goals, titles and evidence. Full source and edited
fields remain in provenance even when the short itch title/original differs.

Four attachments totaling 20 MiB are supported, with 10 MiB per image. Types are
PNG/JPEG/WebP/GIF and video. Only HTTPS Discord CDN attachment URLs are downloaded,
without redirects and with exact declared-size checks. Import, stash, provider
work, research, sends and rendering use deferred private replies. At most four
jobs run concurrently; card rendering is serialized.

**Structure with Mercury** sends only the five editable text fields through a
configured, tested provider. The full proposal is returned as a text attachment.
**Adopt proposal** is separate; nothing is automatically stashed. **Related with
Jev** judges up to eight candidates from the newest 100 local itches in the
selected project. It never merges them.

**Research with Jev** needs a stashed itch, tested Jev connection and configured
Chromium/Edge executable. Rust fetches at most four HTML documents over HTTPS,
validating all resolved addresses and pinning them for each request. Redirects
are followed only after the same validation; proxies, credentials and cookies
are not used. A fresh owned browser reads static HTML with scripts and all
network requests disabled. Jev selects among bounded observed links; it cannot
type, submit forms, sign in or authorize actions. Pages requiring JavaScript may
be unreadable. Excerpts and exact citations are retained with the itch; they are
source observations, not independent verification or an implementation plan.
**Read research** returns the report as a text attachment. Completed reports and
pending saved results recover without another provider pass. New page fetches
stop after 90 seconds; an in-flight request can finish later.

**ASCII card** uses the original artwork and 17 themes in an owned browser
session. GIF, still PNG and flash preferences are honored. Stash also attaches
a card when a browser is configured; failed artwork does not undo the stash.
Cards contain draft text only, never evidence images. These short-lived sessions
do not replace desktop Browse and are closed and removed after each operation.

**Preview T3 send** verifies the exact destination, freezes itch version, text
and evidence, and attaches complete `send-preview.txt`, including the full bounded
original Discord message as untrusted source text. An oversized preview fails
before sending rather than silently truncating the source. **Confirm T3 send** is
required to dispatch. A changed itch rejects a first send; cancel and prepare a
new preview. Each fresh preview has a new durable key. An uncertain send keeps
its original installation, words, evidence and retry identity and cannot be
redirected or discarded. **Recover same send** retrieves/retries that same
native handoff. Receipts persist; Refresh never dispatches.

Use `/dumbbridge draft:<draft ID>` in the same channel after a lost callback or
restart. Interrupted jobs unlock on a fresh interaction. Reopen the original
source message to resume an interrupted import; recorded files are reused.
Tokens are memory-only and zeroized; signed CDN URLs are never persisted. A
crash between saving a file and recording its ID can leave an unused local file.
Discarding an old draft does not delete its stashed itch.

External disposable fixtures exercise signed requests, actual native media and
SQLite, edits and stale actors/versions, project choices without T3, destination
and changed-itch guards, cancellation, exact frozen preview/send bytes and
receipts over loopback HTTP/WebSocket, synthetic Mercury/Jev responses, real
browser fixture pages, persisted exact citations, and original PNG/multiframe
GIF generation in Edge. Only the fixture substitutes local provider/CDN/callback
endpoints and permits its research loopback pages. Production restrictions stay
enabled in the app.

These checks do not establish real Discord registration/delivery, live provider
quality, public search availability or compatibility with a running T3 version.
The companion HTTP handler passed signed PING/rejection fixtures with a state
shim; the real Tauri AppHandle/listener and delivery still need verification. No live
Discord/provider/T3 service was contacted or itch accepted. Development fixtures
and evidence remain outside the repository.

Research preparation recovery looks up its stable request key across all saved
runs, not just the most recent run. A committed run is returned before checking
a later itch version. When the exact key has no committed run, the bridge retains
its frozen question, reconciles the current itch version and persists that
preparation before retrying. The external fixture checked native edits, a newer
unrelated run, stale-version retries and an interrupted missing preparation.
