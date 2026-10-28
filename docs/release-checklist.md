# Native release completion

Status: development checkpoint, not a replacement release. A component handoff
does not complete the integrated app. Keep these IDs stable until each gate has
evidence, or a concrete access requirement that prevents verification.

- [ ] RUST-01 — Discord workflows are implemented and have external integrated
  service-fixture coverage: actual media/SQLite stash, exact destinations,
  proposal/adoption and related judgments, persisted browser research, frozen
  T3 confirmation/recovery and original ASCII PNG/GIF replies. Changed-itch and
  changed-installation guards, cancelled previews and receipt reuse pass.
  Independent review and the actual companion HTTP route remain open. Live
  Discord registration/delivery is a separate RUST-05 verification requirement.
- [ ] RUST-02 â€” Complete selected-window replay and verify snipping. Replay
  storage is implemented, but no selected-window recording source is wired.
  Snipping compiles and cancellation works. The external synthetic fixture's
  automated drag did not reach the tool-window overlay; no successful selected
  PNG is claimed. The fixture was stopped after that limitation was established.
- [ ] RUST-03 â€” Finish the capability inventory against the original app and
  exercise native capture/edit/review, attachments, reload and persistence in
  disposable state. Check settings and unavailable adapters in the original UI.
- [ ] RUST-04 â€” Verify real FFmpeg/FFprobe conversion and the exact preview GIF's
  Windows clipboard handoff. Protocol fixtures and byte-selection checks do not
  establish actual conversion or paste behavior.
- [ ] RUST-05 â€” Verify configured integration boundaries. Local fixtures cover
  provider, T3 and phone contracts. Live provider credentials, microphone/audio,
  Discord and a real phone/proxy are not verified; do not borrow other installs'
  credentials or call production services as test fixtures.
- [ ] RUST-06 â€” Rebuild and inspect the final installer, app, MCP console and
  source package. Check the accepted DM icon in the actual Windows shell. The
  earlier successful installer predates the latest service changes.
- [ ] RUST-07 â€” Independent review of the integrated final diff; resolve material
  findings and run the documented build/static checks for that head.
- [ ] RUST-08 â€” Show the actual integrated interface for user visual review, then
  complete the private-repository PR workflow when its gates pass. Release or
  publication requires its own authorization.

Recently closed implementation gaps: original public inspiration adapters and
evidence-linked Learning topic guides. Learning passed twelve external fixture
scenario groups; the integrated app passes strict Clippy. These checks do not
close the separate UI and integration gates above.

Development fixtures and logs stay outside this repository. The owner requested
removal of test suites, smoke tooling and QA-only executables; do not add them to
the source package or installer. App artwork, build tooling, end-user diagnostics
and required license notices remain.
