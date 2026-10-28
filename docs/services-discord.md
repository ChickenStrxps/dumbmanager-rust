# Discord bridge implementation status

The native companion listener is opt-in. Route only `/interactions` through a configured HTTPS proxy. Signed requests use Ed25519 over the exact timestamp and body, with a five-minute freshness window, application identity and configured actor, channel and server restrictions. PING is supported. Phone routes retain their existing authentication and origin checks.

Explicit registration upserts the two named global commands individually, retaining unrelated commands. It requires a separately saved encrypted bot token and performs no registration at startup. A partial failure can leave one command updated; retry is safe.

Text-only slash and message-menu intake creates a local draft scoped to the actor and channel. Edit controls carry a version, and explicit stash uses native capture and update request keys to preserve source and draft details. Replies are ephemeral and suppress mentions. Signed interaction IDs and responses are retained for the freshness window to deduplicate retries. Drafts persist locally; interaction tokens are not stored.

This is an incomplete port: attachment intake, project and thread binding, Mercury/Jev help and related ideas, research, T3 send and receipts, and ASCII/GIF rendering are unavailable. Draft replies use text. No live Discord verification or provider calls were made. Settings retain rendering preferences for future implementation; they do not yet affect replies.
