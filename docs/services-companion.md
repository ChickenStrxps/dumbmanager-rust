# Phone connection

Connections contains the explicit phone listener controls. Save a specific
loopback or private IPv4 address and an unused port, then start the listener.
Use a private LAN address for a trusted local network. For remote access,
configure your own HTTPS proxy origin and bind the app to loopback. The app does
not install a proxy, open a firewall or expose a tunnel for you.

Create an invitation and open or copy its link to your phone. Its token is in
the URL fragment, works once and expires in five minutes. The phone asks for a
Connect action before exchanging it for an HttpOnly, SameSite=Strict session.
Sessions expire after 24 hours and are invalidated by listener stop or app exit.
Revoke all devices invalidates active sessions and unused invitations immediately.

The server requires the configured Host and same-origin POSTs, plus a separate
CSRF header for paired writes. API inputs and concurrent requests are bounded.
Assets come from the bundled frontend, not a request-controlled filesystem path.
The phone can use local itch/evidence and configured chat flows. Credential
administration, Git, listeners and native system actions require the desktop.

Direct HTTP is intended only for a trusted LAN; HTTPS is needed beyond it.
The app stays awake on the desktop for the listener to remain available. Pairing
does not enable startup persistence. Discord routing is a separate unfinished
service and does not become available merely by starting this listener.

Eight real loopback checks passed against fresh temporary native storage.
A physical phone, reverse proxy and external network have not been verified.
