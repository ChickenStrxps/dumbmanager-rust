# Bundled interface capability audit

The bundled original interface is the migration surface. A source audit found
85 distinct literal `call(...)` capability names in its JavaScript. Of those,
84 have named native handling; the remaining legacy job cancellation call is
explicitly unavailable. This count is a routing check, not 84 runtime tests.
Computed operation names, API paths and provider-specific behavior are assessed
separately. No original runtime, keys or records were imported for this audit.

| User flow | Native owner / contract | Verification and limits |
| --- | --- | --- |
| Stash, moments, edit, search, review, repeat, merge and export | `core/src/legacy` through `state.rs` | SQLite transaction/version/retry fixtures; actual WebView capture/edit/review/reload checked. Acceptance remains explicit. |
| Saved context and local suggestions | Core compatibility layer, `services/ai.rs` | Saved context persists. Mercury is an explicit action; configuration is separate from a tested connection. |
| Evidence upload/read, image and video handling | `services/media.rs` | Real native PNG upload/read round trip, original-byte fixture and real FFmpeg nine-panel conversion. OCR/crop and renderer checks have their own earlier evidence. |
| Social draft, preview, GIF, clipboard and webhook | `services/social.rs`, `native-card-renderer.js` | Original artwork rendering and exact cached bytes checked; no real webhook contacted. Windows shell paste remains unverified. |
| T3 installation choice, pairing, projects, threads, preview, chat and handoff | `services/t3.rs` | Exact destinations and retry receipts checked against local fixtures; live read-only thread discovery passed. Live sends remain unverified. |
| Browse and research reader | `services/browser.rs` | Isolated Chromium, bounded static-document reader, pinned public HTTPS and offline DOM. JavaScript-required research pages may be unreadable. |
| Git controls | `services/git.rs` | Explicit registered roots; no original workspace is inferred. No new PR interface is required by this migration. |
| Suggestions, Jev judgments, research, Learning, inspiration, scouts | `services/ai.rs` and subtree | Mercury/Jev connection tests passed live; richer workflows use isolated fixtures and exact citation/recovery checks. Captured X needs an explicit source. |
| Voice and MCP | `services/voice.rs`, separate MCP console | Session/approval fixtures; live audio, provider accounts and external MCP clients unverified. |
| Phone and Discord | `services/companion.rs` and subtree | Live Discord bot/application authentication passed. Signed intake, native storage and sends use fixtures. Live delivery and a physical phone/proxy remain unverified. |
| Shortcuts, snip and replay | `services/desktop.rs` and subtree | Real synthetic-window snip/replay checks. Opt in only; no startup recording. Real game/protected-window capture is not established. |

Unsupported private ecosystem controls and account-scanning controls were removed
from the public interface. The retained legacy `jobs.cancel` handler is unavailable;
`/api/jobs` returns an empty unavailable listing. Neither calls the Python host.

Other API owners: `/api/cook`, `/api/preview` and `/api/upload` route to T3;
`/api/itch-upload` and `/api/itch-media` route to native media;
`/api/native` and `/api/replay` route to desktop services. Unknown operations
return errors rather than successful placeholders. The phone has a restricted
subset and does not inherit native desktop permissions.

See [migration inventory](migration-inventory.md) for remaining gaps and
[release checklist](release-checklist.md) for the release gates. This audit does
not establish complete 1:1 behavior or live external-service compatibility.
