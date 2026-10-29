# Media and social integration handoff

## Browser card renderer

`web/card-renderer.js` is the byte-for-byte concatenation of the original DumbAPI `ascii_smoke.js` and `social_gif.js` (with one separating newline). It contains the original 17 themes, ASCII cast, motion, events, copy layout and GIF encoder. The original comments are retained. `web/native-card-renderer.js` serializes up to four requests and exposes `DMNativeCards.gif(args, callNative, {signal})`. The supplied `callNative` must bypass this interception. It calls `manager.social.gif_prepare`, renders the frozen payload, and calls `manager.social.gif_store`. Worker cancellation terminates its worker. The main-thread fallback yields between groups of original frames, checks cancellation and has a 120-second deadline.

A worker's canvas/fonts can differ from the main WebView. Before using it, the broker renders and hashes the first frame in both environments; when they differ, it uses the original DOM canvas implementation. A local browser fixture with Unicode and the Greg event produced different worker and DOM hashes; the fallback output matched the original DOM GIF SHA-256 exactly. A separate smoke fixture produced the original 96-frame/5.76-second GIF. This is fixture evidence, not a complete visual audit of every theme and event or a test in the packaged Windows WebView.

The bundled interface loads the broker and routes `manager.social.gif` through it. CSP permits the local worker, fonts and blob previews. `gif_prepare` returns `{nonce,expires_at,payload,vibe,force}`; `gif_store` accepts `{nonce,gif,metadata}`. Only this operation receives the larger JSON allowance needed for a 15 MiB GIF and base64 overhead. The backend verifies the one-shot 10-minute nonce, draft/source versions, requested theme/events, decoded GIF dimensions, loop, frame bounds and duration, and recomputes SHA-256. Valid exact bytes are stored under this app's own service root, bounded to 96 MiB, with oldest eviction. The WebView is the trusted renderer; GIF parsing and the nonce validate structure and bind the operation to a draft snapshot but cannot prove that arbitrary compromised client pixels depict that snapshot.

`manager.social.gif_copy` requires `{id,expected_version,vibe,theme,sha256}`. It finds the exact cached digest, writes and rereads an app-owned GIF, then calls the parent's native `copy_export_file` helper. The caller must send `live.meta.sha256`. Webhook send selects the cached digest too; it does not rerender a different native fallback. Missing cache, stale source/version, or digest mismatch fails explicitly. Actual Windows shell paste and an external webhook were not tested.

The native Rust `manager.social.gif` implementation remains the older water-only fallback when the browser broker is absent. It is not the original 17-theme artwork and its output is not cached for copy/webhook; do not claim it has social-card parity. Bundled fonts now draw Rust fallback copy with fontdue; unsupported code points are rejected rather than replaced. The original static SVG card was separately ported from `manager_social.py`: seeded original colors and six creatures, escaped text, stage labels, panel and complete width-aware copy. The renderer uses `unicode-width` for copy layout.

Windows OCR, source-image selection, crop and mask review, and flattened sanitized PNGs are implemented. Explicitly chosen local evidence and native Browse screenshots can enter this flow. A source/version-bound sanitized image feeds both still and GIF preparation. A real Windows OCR check on synthetic text passed; this is not a guarantee that OCR detects every secret. Review the image and masks before sharing. No original app source, runtime data or connection is consulted at runtime.

## Post links and webhooks

`manager.social.link` requires `{itch_id,url,confirm:true}` and accepts supported HTTPS X, TikTok and YouTube post URLs. It stores a user-confirmed URL with `verified:false` and `source:user_confirmed_url`; it never contacts or inherits an external account database. `check` reports saved links and explicitly says no account check was performed. `unlink` removes local links. The interface asks for confirmation and labels links as unverified. Its refresh button reads saved links, and it offers no account scan in the posting panel.

Webhook delivery keeps URLs in DPAPI slots, requires confirmation, current versions, an exact cached GIF digest and a request key, reserves a receipt before network I/O, and never retries automatically. 429 responses persist a settings-version-specific cooldown. Concurrent reservations of the same key are atomic; network calls occur outside the lock. A request already in flight before a 429 is received cannot be recalled. Current formatting handles common Markdown links, emphasis, headings and ordered/unordered bullets; it has not been proven identical for all Markdown. The core itch/source recheck and receipt reservation are separate transactions, so concurrent edits may race this final check. DNS/HTTPS policy, fixture multipart and retry behavior are covered locally, not against a real provider.

## Attachments and frameboards

`/api/itch-upload` and `/api/itch-media` use app-owned storage; the native transport passes selected file bytes and decodes binary envelopes. Inputs are capped at 50 MiB (10 MiB for images), eight attachments/80 MiB per itch, and 512 MiB of stored media. `references` returns canonical stored metadata. `read_original` and `/api/itch-media` with `original:true` retrieve the retained original video for a frameboard.

Frameboards use only explicitly configured absolute FFmpeg and FFprobe executables. Settings exposes FFmpeg/FFprobe configuration through `manager.media.frameboard_settings` and `manager.media.frameboard_configure`; saving requires `{confirm:true,ffmpeg,ffprobe}` or `{confirm:true,clear:true}`. Processes have bounded execution and a file/pipe protocol whitelist. For 0.1–120 second clips, the original nine-frame timestamped JPEG filter is used, the video original remains local, and output metadata records the duration. The T3 upload/handoff adapter calls `media::video_frameboard(state,bytes)` for frameboard mode and attaches the JPEG with its duration note. A locally compiled fake-process fixture checked the command contract, output handling and original-byte retention. No real FFmpeg executable or actual video was run as QA; actual decoder/filter compatibility remains unverified. Automatic pruning of unclaimed media is not implemented.

## Verification limits

Before the development suites were removed at the owner's request, renderer and
service checks exercised real GIF decoding, Unicode wrapping, nonce/version/digest
validation, clipboard input bytes, link provenance, concurrent receipt reservations,
cooldowns and local multipart delivery. The integrated service suite and strict
compiler checks also passed. These checks do not establish production-provider
compatibility or full packaged WebView parity. Native clipboard paste, all card
themes/events and real FFmpeg decoding still need a final manual check.
