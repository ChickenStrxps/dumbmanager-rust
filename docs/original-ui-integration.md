# Original interface migration

The existing DumbManager frontend is the visual source of truth. Preserve its
layout, fonts, navigation, capture flow, dialogs and full-page ASCII animation.
The native migration must not replace it with a new dashboard.

Only source code, bundled fonts and original artwork may be reused. Runtime
databases, settings, keys, captures, browser profiles and account data remain in
their existing installation. The Rust app keeps its separate Windows identity,
storage, WebView profile and executable.

The original frontend's same-origin JSON API requests use a small native
transport and the allowlisted `legacy_api` IPC command. Its local domain calls
must reach the Rust SQLite implementation. Unknown or unported operations must
return explicit errors. A status response may truthfully describe an unavailable
service; it must not fabricate connected accounts, threads, jobs or findings.

The original source uses an externally served smoke module and a separately
loaded bridge module. Include these as bundled scripts with their original load
order. Preserve the restrictive script policy; do not introduce inline script
execution to reproduce the Python host's startup script injection.

Frontend and backend implementation notes record exact deltas and supported
contracts. Native QA must exercise the original interface before a new build is
called ready for review. Earlier tests of the replacement interface establish
only backend and installer mechanics; they are not evidence of visual parity.
