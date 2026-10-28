# Bundled interface capability audit

The bundled original interface is the migration surface. A source audit found
93 distinct literal `call(...)` capability names in its JavaScript. Of those,
84 have named native handling; the nine legacy ecosystem calls listed below
are explicitly unavailable. This count is a routing check, not 84 runtime tests.
Computed operation names, API paths and provider-specific behavior are assessed
separately. No original runtime, keys or records were imported for this audit.

| User flow | Native owner / contract | Verification and limits |
| --- | --- | --- |
| Stash, moments, edit, search, review, repeat, merge and export | `core/src/legacy` through `state.rs` | SQLite transaction/version/retry fixtures; actual WebView capture/edit/review/reload checked. Acceptance remains explicit. |
| Saved context and local suggestions | Core compatibility layer, `services/ai.rs` | Saved context persists. Mercury is an explicit action; configuration is separate from a tested connection. |
| Evidence upload/read, image and video handling | `services/media.rs` | Real native PNG upload/read round trip, original-byte fixture and real FFmpeg nine-panel conversion. OCR/crop and renderer checks have their own earlier evidence. |
| Social draft, preview, GIF, clipboard and webhook | `services/social.rs`, `native-card-renderer.js` | Original artwork rendering and exact cached bytes checked; no real webhook contacted. Windows shell paste remains unverified. |
| T3 installation choice, pairing, projects, threads, preview, chat and handoff | `services/t3.rs` | Exact destinations and retry receipts checked against local fixtures; live T3 pairing/send not verified. |
| Browse and research reader | `services/browser.rs` | Isolated Chromium, bounded static-document reader, pinned public HTTPS and offline DOM. JavaScript-required research pages may be unreadable. |
| Git controls | `services/git.rs` | Explicit registered roots; no original workspace is inferred. No new PR interface is required by this migration. |
| Suggestions, Jev judgments, research, Learning, inspiration, scouts | `services/ai.rs` and subtree | Isolated provider/source fixtures and exact citation/recovery checks. Captured X needs an explicit source. Live provider compatibility is unverified. |
| Voice and MCP | `services/voice.rs`, separate MCP console | Session/approval fixtures; live audio, provider accounts and external MCP clients unverified. |
| Phone and Discord | `services/companion.rs` and subtree | Signed intake, auth boundaries, native storage and synthetic sends checked. Live Discord and a physical phone/proxy are unverified. |
| Shortcuts, snip and replay | `services/desktop.rs` and subtree | Real synthetic-window snip/replay checks. Opt in only; no startup recording. Real game/protected-window capture is not established. |

Legacy calls without a native ecosystem adapter are `analytics.suggest`,
`analytics.lookup`, `analytics.query`, `analytics.scan`, `anime.batches`,
`anime.batch.create`, `anime.batch.status`, `anime.batch.render` and `jobs.cancel`.
The core rejects them as unavailable; it does not call the original Python host.
`/api/jobs` returns an empty unavailable listing. These legacy UI entries do not
constitute support for AnalyticsOne or AEdits in the native app.

Other API owners: `/api/cook`, `/api/preview` and `/api/upload` route to T3;
`/api/itch-upload` and `/api/itch-media` route to native media;
`/api/native` and `/api/replay` route to desktop services. Unknown operations
return errors rather than successful placeholders. The phone has a restricted
subset and does not inherit native desktop permissions.

See [migration inventory](migration-inventory.md) for remaining gaps and
[release checklist](release-checklist.md) for the release gates. This audit does
not establish complete 1:1 behavior or live external-service compatibility.
