# Original frontend restoration

## Native contract handoff

The frontend is restored from the original DumbManager `web/` source, with
the server-served smoke source from DumbAPI. The original UI, navigation, dialogs,
CSS, fonts, and aquarium are the reference. No original runtime or account data is
used. Only `web/` and this document belong to this frontend correction.

`native-transport.js` must run before every application script. It intercepts only
same-origin `/api/*` POST requests and invokes
`window.__TAURI__.core.invoke('legacy_api', {path, payload})`. Success must be the
actual legacy JSON value (no envelope). Rejections may be a readable string or
`{error, code, status, retry_after_ms}`; the bridge turns these into a real JSON
`Response` with a failing HTTP status. Ordinary bundled-asset fetches retain the
browser's native fetch. In a browser without Tauri, API requests return an honest
503 `native_unavailable`; they never contact a Python API. Binary uploads and
media responses need a separate native contract and must remain unavailable.

Required first-party local flows, using original names and snake_case fields:

- `/api/call` receives `{name, args, idempotency_key?, t3_target?}`.
- `manager.state` supplies `{itches, contexts, active_context, preferences}`;
  `preferences.motion` is boolean. The UI also reads optional `typing_words` and
  `typing_generator` capability metadata. Empty local arrays are valid when real.
- Every itch needs its original fields and arrays: `id`, `key`, `title`, `project`,
  `state`, `version`, `created_at`, `updated_at`, `plan`, `outcomes`, `occurrences`,
  `feedback`, `handoffs`, `evidence`, `attachments`, plus supported original detail
  fields. Do not return only
  the newer camelCase `Itch` object from the first migration milestone.
- `manager.capture` receives `title`, `moment`, `project`, `context_id`,
  `context_snapshot` (JSON string), `attachments` (JSON string), `use_active`,
  `event_at`, `request_key`; it returns a complete original itch.
- `manager.update` receives `{id, changes: <JSON string>, expected_version,
  request_key}` and returns the updated original itch. Original edit, plan,
  archive, and ready-for-review actions depend on it.
- `manager.complete` receives `{id, summary, evidence, accepted:true,
  request_key}` after the user's explicit review dialog; it returns the original
  updated itch including a recorded outcome. Completion must never be inferred.
- `manager.get` receives `{id}` and returns the same full legacy itch for popup
  refresh. `manager.repeat`, `manager.settings` (`{motion:<boolean>}`), `manager.export`,
  `manager.context.select`, and `projects.list` should be implemented locally or
  return an explicit unavailable error. A real empty project list uses `{items:[]}`.
- Startup also asks `t3.distributions`, `t3.threads`, `/api/replay` `{action:'status'}`. These may be unavailable; do not fabricate a
  connection, installed account, helper, window, replay, thread, source, or job.
- Opening an itch uses `/api/cook` focus/status/prepare/clear/unfocus and
  `/api/native` status. Missing integrations must not prevent local detail/edit/
  review from opening. Preserve their errors as unavailable.
- Original Side quests, Browse, Git, connection settings, voice, research,
  learning, social and posting remain visible. Their original operations must
  fail truthfully until implemented. No external service is connected by this
  restoration.

Parent-owned checks: Tauri CSP must permit these bundled scripts, local fonts,
blob artwork, and the original dynamically styled controls; no inline executable
script is required. Package syntax checks must use the restored JS filenames,
since `web/art/` from the redesign is removed. Native WebView capture/list/edit/
review and persistence verification belong to the parent/native worker.

## Exact source deltas

1. `index.html`: change the document title to DumbManager Rust and initial storage
   label to Rust; add external deferred `native-transport.js` first, explicit
   `bridge.js` before `app.js`, and external `startup.js` last. Replace the server
   injection marker with an explanatory comment. Preserve all original visual
   markup, navigation, dialogs, and stylesheets.
2. `app.js`: replace dynamic module reexecution/retry with one bundled-module
   validation; catch startup failure in the original status/toast. Bound the two
   layout-frame wait to 100 ms, cancelling the outstanding frame and timer, so a
   nonpainting native window cannot prevent startup from completing. Retain the
   original navigation, capture, editing, review, and renderer logic. Change only
   storage/outbox explanatory strings and the loading source label from DumbAPI
   to local Rust wording. Show a true state-read error in the original empty box
   instead of claiming an empty successful list. Preserve shortcut/replay errors
   in their original settings/status areas. Remove the inherited extra blank
   line at the end of this modified source file.
3. `aquarium.js`: track and release its grain blob URL on replacement and final
   page exit (retain it for persisted page transitions). The original glyph art,
   frames, timings, CSS, DPR cap, bounded smoke/squid atlases, reduced-motion and
   visibility guards are unchanged.
4. `itches.js`: handle the background focus promise rejection; the original
   popup's existing publish/error flow still displays unavailable helper errors.
   Local popup/edit/review is not gated on successful external helper work.
5. `startup.js`: use an external, bounded missing-entry diagnostic. It never
   reinserts or executes modules again, avoiding duplicate clocks/animation loops.
6. New `native-transport.js`: same-origin POST-only IPC adapter, real JSON
   `Response` objects, structured native error status/code/message preservation,
   abort behavior, JSON object validation, and explicit missing-native/upload
   errors. It contains no test data, service connection, Python call, or fallback
   HTTP API. Aborting an invoked command stops waiting; cancellation of a native
   write is not claimed.

All four original CSS files, all original font binaries/notices, and every other
original UI module are byte-identical. The old redesigned `web/art/` scripts,
`web/img/mark-*` images, and renamed font set are removed; old `app.js`,
`index.html`, and `style.css` are replaced. No redesign preview is bundled.

## Observed verification (2026-10-26)

- `node --check` passed for all 24 bundled JavaScript files. HTML validation found
  all 27 external script/stylesheet assets and all CSS font URLs. Every entry
  script is deferred and ordered; there is no inline executable script.
- Live T3 preview loaded all original modules and all three original font faces.
  `appReady` became true, no startup-recovery dialog or retry script was present,
  and the original main handlers opened Now, Itch pile, W's, Side quests, Browse,
  Git, and Settings. Observed error/unhandled-rejection collection stayed empty.
- At 1440 x 900 the original rail measured 236 px and the Silkscreen heading 54 px;
  the original dotted dark background and dense capture/quick-action layout were
  present. At 390 x 844 the original responsive rail/header measured 123 px high
  and the heading 30 px. Neither size had horizontal document overflow. The
  original context editor opened as a 560 px dialog on desktop.
- The T3 headless renderer produced a visually inspected 1440 x 900 capture with
  the original aquarium, roaming glyph creatures, smoke, rail, fonts, controls,
  and original unavailable-state box. Its console reported no errors. For this
  tool-only QA page the real copied CSS/JS/fonts were embedded self-contained and
  viewport-height units resolved to the equivalent 900 px viewport. No native
  API success, app records, or external service data was mocked. No QA page or
  screenshot was added to product assets.
- Live `preview_snapshot` capture timed out twice with
  `Preview automation snapshot timed out after 15000ms.` A focused click also
  timed out; original handlers were exercised through T3 `preview_evaluate` DOM
  clicks. T3 `html_preview` supplied the visual capture after direct localhost
  assets were blocked by that renderer's proxy (`ERR_SOCKS_CONNECTION_FAILED`).
- In the real browser without Tauri, a valid API POST and a Request-object POST
  returned genuine failing Responses (503 `native_unavailable`). Malformed JSON
  and array payloads returned 400; binary upload returned 415; an already-aborted
  request rejected with `AbortError`. A failed capture preserved the typed text,
  restored its button, and created zero itches and zero outbox entries.
- Bundled CSS still fetched normally with status 200 and the exact original
  SHA-256 shown below. Observed API network resource entries remained zero through
  navigation, unavailable capture, and transport checks. The original apps were
  not launched or contacted. No original state, keys, configuration, database,
  installed files, or runtime assets were copied.

## Remaining native checks and unsupported contracts

The Rust worker's source now exposes `legacy_api` with serialized
`{error, code, status}` rejections, matching the adapter. That source-level match
is not a native runtime test. The parent must run the genuine isolated WebView to
verify capture -> list -> popup -> edit/plan -> explicit review/acceptance ->
reload persistence, motion settings, reduced motion, hidden-window animation
pause, and long-lived native memory behavior. This worker did not simulate
successful native records or claim native feature parity.

Parent-owned Tauri CSP and packaging must permit the original programmatic
styles, local fonts/blob art, and the ordered external scripts at tauri.localhost.
The parent must update package UI syntax checks that still name deleted
`web/art/*.js` files. Native command permissions must include `legacy_api`.

Binary uploads, replay/media blobs, and external workflows require distinct
implemented contracts. The original controls/pages remain recognizable and
return unavailable errors. Original social cards call `manager.social.*`; the
new Rust renderer's optional `manager.card_export` command is not substituted
into that workflow or represented as a connected social service. No successful
external status, replay, helper, job, thread, attachment, or populated feed is
fabricated by the frontend.

## Asset provenance SHA-256

Sources are the explicitly authorized original working-tree source assets,
identified below with repository-relative paths. These are raw-byte hashes, not
normalized text hashes. There are 28 unchanged DumbManager assets plus the
unchanged shared smoke source; five original files have the deltas listed above.
Only `native-transport.js` is new.

| Bundled path | Source path | Original SHA-256 | Bundled SHA-256 |
| --- | --- | --- | --- |
| `web/ai.js` | `dumbmanager/web/ai.js` | `6e46ffdc6c25d69e616fd6efa1180375ca38baa580916cf169416388f2d2b5b2` | `6e46ffdc6c25d69e616fd6efa1180375ca38baa580916cf169416388f2d2b5b2` |
| `web/app.js` | `dumbmanager/web/app.js` | `8e4eb34949cb58f3fd9da79f9a12b966db6a68783e5ec2c22101e7bb0b284a02` | `4bc543f87c98b12a968caf080a35dcd8629381796d9b3073a42625b107383a30` |
| `web/aquarium.js` | `dumbmanager/web/aquarium.js` | `e11c3a596a75715ab4d91facda9bc2ca2d40015116d6cce3beae3c8e1fef6f14` | `bf0312c3193fad8becc1b1c3a177bba3a5c2490c70bb84226a773a89e75c263f` |
| `web/attachments.js` | `dumbmanager/web/attachments.js` | `53b2a5f3b1fb66c8931325f1a1d1fdc12801a97d93477cbd60b1ac32e255cb4d` | `53b2a5f3b1fb66c8931325f1a1d1fdc12801a97d93477cbd60b1ac32e255cb4d` |
| `web/bridge.js` | `dumbmanager/web/bridge.js` | `86ec8ba1f51608300c05657bd481ca26329fe04103bd46e178f11f609a351f03` | `86ec8ba1f51608300c05657bd481ca26329fe04103bd46e178f11f609a351f03` |
| `web/browse.js` | `dumbmanager/web/browse.js` | `ba11c78922e76d175f46fced7ceeb39df1265e3ab08426fddc6f73e1ff02104b` | `ba11c78922e76d175f46fced7ceeb39df1265e3ab08426fddc6f73e1ff02104b` |
| `web/chat.js` | `dumbmanager/web/chat.js` | `2eee68201950debeaf58cee33c32ca6e49ab197e083ee4e721bda1de5d6525a0` | `2eee68201950debeaf58cee33c32ca6e49ab197e083ee4e721bda1de5d6525a0` |
| `web/fonts/IBMPlexMono-OFL.txt` | `dumbmanager/web/fonts/IBMPlexMono-OFL.txt` | `7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da` | `7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da` |
| `web/fonts/IBMPlexMono.ttf` | `dumbmanager/web/fonts/IBMPlexMono.ttf` | `6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46` | `6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46` |
| `web/fonts/Silkscreen-OFL.txt` | `dumbmanager/web/fonts/Silkscreen-OFL.txt` | `3b8d2bb95a9b6e8ac3e06858372b6db156068fe80e9bece56b67244bae4d02b6` | `3b8d2bb95a9b6e8ac3e06858372b6db156068fe80e9bece56b67244bae4d02b6` |
| `web/fonts/Silkscreen.ttf` | `dumbmanager/web/fonts/Silkscreen.ttf` | `c845473330b94c2079ce9af01c51ac8ba2d99c24f4d14c039843bbb8e642ebd8` | `c845473330b94c2079ce9af01c51ac8ba2d99c24f4d14c039843bbb8e642ebd8` |
| `web/fonts/SpaceGrotesk-OFL.txt` | `dumbmanager/web/fonts/SpaceGrotesk-OFL.txt` | `564ce565c371c5e5bbf286006565a7c9aa55a9f56e7ca58d56e05d649dd61a72` | `564ce565c371c5e5bbf286006565a7c9aa55a9f56e7ca58d56e05d649dd61a72` |
| `web/fonts/SpaceGrotesk.ttf` | `dumbmanager/web/fonts/SpaceGrotesk.ttf` | `acad6de1fc93436f5c0f1f4137751ef04f1aea3063e7036535970ffcfbd79f72` | `acad6de1fc93436f5c0f1f4137751ef04f1aea3063e7036535970ffcfbd79f72` |
| `web/git.js` | `dumbmanager/web/git.js` | `6ad6160038bea64323b87a449608ecfe748ca35d5c2dd5567fbb137d809da982` | `6ad6160038bea64323b87a449608ecfe748ca35d5c2dd5567fbb137d809da982` |
| `web/index.html` | `dumbmanager/web/index.html` | `7453bbfb594d59a5851b21943d6dfc2730e2d79b310f37135e2ac8d69cef6d9c` | `9505c5c1d13592018262f67910b85b77c012cd7770b42afaa96341708e7db8c0` |
| `web/inspiration.css` | `dumbmanager/web/inspiration.css` | `2bec6e4e8b46d2b79383abe0d6506973a9022267d84869741ed4df3570ebeaee` | `2bec6e4e8b46d2b79383abe0d6506973a9022267d84869741ed4df3570ebeaee` |
| `web/inspiration.js` | `dumbmanager/web/inspiration.js` | `3ccd6b62587ba1056384c1b2ba758bd735aa7f3446ac96533b6a252febaef167` | `3ccd6b62587ba1056384c1b2ba758bd735aa7f3446ac96533b6a252febaef167` |
| `web/intake.js` | `dumbmanager/web/intake.js` | `7904a6f1c673c4865d39f66f949e4fc2f42680a8666f42d68c3160ce0ea596e8` | `7904a6f1c673c4865d39f66f949e4fc2f42680a8666f42d68c3160ce0ea596e8` |
| `web/itches.js` | `dumbmanager/web/itches.js` | `044e80ce4ff6de8692e8de8b4b81c2147a435ec0c7dd4cb7e3671748c2271cea` | `25e2d6f7400c652602557c31ae994d70333fc6406debb4a67e7498ad8bfbcc31` |
| `web/learning.css` | `dumbmanager/web/learning.css` | `98d4e1ef098031f40d8eb8446aa6b58e64e62493d42d337f70397ac8d3a7ad30` | `98d4e1ef098031f40d8eb8446aa6b58e64e62493d42d337f70397ac8d3a7ad30` |
| `web/learning.js` | `dumbmanager/web/learning.js` | `2695b8bc0f17a855682f5c2174b9fe2496213cf93728bfb2972b39e4e587cbd7` | `2695b8bc0f17a855682f5c2174b9fe2496213cf93728bfb2972b39e4e587cbd7` |
| `web/pcm-worklet.js` | `dumbmanager/web/pcm-worklet.js` | `4daa98f2cac63ae04609385480676b5fe5defe12f32c7e80ab1cdda5d1139935` | `4daa98f2cac63ae04609385480676b5fe5defe12f32c7e80ab1cdda5d1139935` |
| `web/previews.js` | `dumbmanager/web/previews.js` | `70a16a8c74d4772bc767a3c88657873bb0d6d64a5542fa4ba27d50b98a6b54b2` | `70a16a8c74d4772bc767a3c88657873bb0d6d64a5542fa4ba27d50b98a6b54b2` |
| `web/research.css` | `dumbmanager/web/research.css` | `38d54731e3677008a0c832cba01589ef5991c49fedb2cb134e98d53bab899163` | `38d54731e3677008a0c832cba01589ef5991c49fedb2cb134e98d53bab899163` |
| `web/research.js` | `dumbmanager/web/research.js` | `7e8b016dbcd850d657d0d7ebf5612e8cfa63c1741d5dfc3f9bec94c171448126` | `7e8b016dbcd850d657d0d7ebf5612e8cfa63c1741d5dfc3f9bec94c171448126` |
| `web/richtext.js` | `dumbmanager/web/richtext.js` | `b08b865f9816d31d15b1c30a582a2a553f11a1d1c5af5eea8fa2b2b9bcd0d5ca` | `b08b865f9816d31d15b1c30a582a2a553f11a1d1c5af5eea8fa2b2b9bcd0d5ca` |
| `web/social.js` | `dumbmanager/web/social.js` | `007072b7e2ee06e9c59df5cc8daa2ca53ec06b8568c8f92c67f874fa3890fd8a` | `007072b7e2ee06e9c59df5cc8daa2ca53ec06b8568c8f92c67f874fa3890fd8a` |
| `web/startup.js` | `dumbmanager/web/startup.js` | `0e32b664edc3dfdb8c79d0fc18fb0925126a0567ddf82444c0afbcfe0989d08f` | `1a5e2e276f58a040be93cffd1f3301405effcd96ee0e86e26ccc5db1b6a3fe18` |
| `web/style.css` | `dumbmanager/web/style.css` | `c6f918b151b0717e8364f0b425f0dfc474cdf2cf81e09bb38161a0d6fe8387f4` | `c6f918b151b0717e8364f0b425f0dfc474cdf2cf81e09bb38161a0d6fe8387f4` |
| `web/t3-settings.js` | `dumbmanager/web/t3-settings.js` | `29f3766679b5a584965f6204b99c894c6dfa974ad3487d8fe371e0ed15647458` | `29f3766679b5a584965f6204b99c894c6dfa974ad3487d8fe371e0ed15647458` |
| `web/ui.js` | `dumbmanager/web/ui.js` | `e26f37bb5b225d616198148b5dcd525ad8086370b7e0f1c6af9ad5c2505995f4` | `e26f37bb5b225d616198148b5dcd525ad8086370b7e0f1c6af9ad5c2505995f4` |
| `web/voice.js` | `dumbmanager/web/voice.js` | `5cc75dfae791ff751af0ac83d280ac8cae8049d3d29d9f493c2e8cc884a3300b` | `5cc75dfae791ff751af0ac83d280ac8cae8049d3d29d9f493c2e8cc884a3300b` |
| `web/webhook.js` | `dumbmanager/web/webhook.js` | `06bef7e8c85fc9c06f3e25c53ac21de08d4b28a8c188b9ab796f492fad55c160` | `06bef7e8c85fc9c06f3e25c53ac21de08d4b28a8c188b9ab796f492fad55c160` |
| `web/ascii-smoke.js` | `DumbAPI/dumbapi_core/ascii_smoke.js` | `67e32a4d6ddf4b90743abf6e9625ed65702076da5e136a66deee90fd80f9bffc` | `67e32a4d6ddf4b90743abf6e9625ed65702076da5e136a66deee90fd80f9bffc` |
| `web/native-transport.js` | New native transport | n/a | `7e9f5c6a17312d5ba1d87920c2fd72ebecff1588488d79daf95916b2a8468e5c` |
