# Desktop capture

Replay is off until the user chooses a listed window and arms it. The `hwnd`
field in the existing UI contract is an opaque, one-use choice token: listing
creates a Windows Graphics Capture item for each choice, and arming consumes
that item. Frames never come from a subsequent HWND screen grab. A close event
or failed window-liveness check stops the session; disarm discards any unfinished
event. No capture or shortcut registration is started at application startup.

The sampler keeps up to six seconds and 8 MiB of JPEGs in memory. A focused
capture begins with that buffer, records up to 18 seconds during entry, then
collects six seconds after that native entry saves successfully. Each entry is
bound to an opaque event token and its exact capture retry key. Other intake
origins and cached retries cannot start or consume its event. Blur cancels an
unsaved entry; an entry older than 18 seconds is rebased to the current prebuffer
on the next input/save, so abandoned input cannot add an old scene. Only one event can be pending
at once; additional saves during that event do not get another replay. Events
are capped at 300 frames and 20 MiB. Frames preserve aspect ratio within 640 by
360, at up to 10 samples per second, without audio. The last captured frame is
reused when Windows reports no new content. Games or protected content can
produce blank frames. Windows manages the capture frame-pool GPU allocation;
input dimensions are checked before starting and CPU readback is rejected above
4096 pixels per dimension, but a resize may cause Windows to allocate a larger
frame-pool texture before the frame callback can reject it.

Temporary boards expire after 24 hours during storage operations. The total
budget is 256 MiB and kept boards share 128 MiB. Replay data is stored only
under this application's service data directory. Complete boards are published
by renaming a staged directory. Storage operations recover interrupted staging
and legacy incomplete folders only when they contain ordinary app-named files;
unexpected paths or links fail safely. Valid kept boards are retained. An unfinished replay is not
persisted on window close, disarm, or app exit.

The Windows build includes `windows-capture = "=2.0.1"`. Successful native
`manager.capture` results finish only their matching event without changing the
saved itch result. A one-shot native transport field is removed before the core
validates and persists the capture; it is not part of the retry fingerprint. Canonical hyphenated itch UUIDs map to the replay store's compact
UUID form, so the original frontend can retrieve boards with its normal IDs.

Disposable Windows verification captured the intended red fixture window while
a blue topmost window covered it, decoded the saved JPEG pixels, exercised keep
and delete, stopped after the window closed, and rejected a stale selection.
A completed snip produced a 100 by 80 PNG with the expected fixture pixels.
The expanded fixture also checked wrong token/key rejection, old capture retry
isolation, cancellation, expired-entry rebasing and interrupted-folder recovery.
These checks use owned synthetic windows; protected content and actual game
compatibility are not established.

Interrupted keep/unkeep updates are recovered by removing only correctly named
ordinary manifest temp files under the storage lock. The valid index and kept
frames remain intact. A cursor-free storage fixture checked this recovery,
explicit deletion and expiration cleanup; failed temp writes also remove their
partial output.
