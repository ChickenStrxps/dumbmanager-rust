# Native IPC

All calls use `window.__TAURI__.core.invoke(command, args)` from bundled local
content only. No public HTTP server or Python process is required.

The restored original frontend sends its same-origin JSON requests through
`legacy_api` with `{path,payload}`. It returns the actual original-contract JSON
value, including original snake_case field names. Errors reject with
`{error,code,status}`. `web/native-transport.js` turns those into an HTTP-shaped
JSON Response; a domain failure must not become a network/offline-outbox event.
Paths and capability names are explicitly allowlisted in the native core.
See [the compatibility contract](original-ui-backend.md) for supported calls and
honest unavailable states. Asset requests keep the browser's normal fetch.

The smaller native command surface remains available for service checks and
local exports. These arguments and results use camelCase; its errors reject
with a readable message:

- `app_snapshot` -> `{name, version, dataDir, counts:{open,stashed,ready,resolved,total}, integrations:[{id,name,state,detail}], startedAt}`
- `itch_list` `{query:"",status:"all",project:"all"}` -> array of Itch.
- `itch_save` `{input:{id?,title,description,project,kind}}` -> Itch. New itches are open. kind = bug|improvement|idea. Title required.
- `itch_transition` `{id,status}` -> Itch. status = open|stashed|ready|resolved. Resolving requires the user's explicit acceptance button. Never resolve as an implementation side effect.
- `settings_get` -> `{motion,theme,defaultProject,integrationStatus:[...],...}`. No returned secrets.
- `settings_save` `{input:{motion,theme,defaultProject}}` -> same result as settings_get. motion = full|reduced|off. theme = midnight initially.
- `card_export` `{id,format}` -> `{path,width,height,frames,bytes}`. format = gif|png. This writes a real local file using the Rust renderer; it never posts or accepts an itch.
- `open_export_folder` -> unit. Open only the Rust app's export directory.

Itch = `{id,title,description,project,kind,status,createdAt,updatedAt}`.
Dates are ISO 8601 UTC strings. Lists default newest update first.

The UI must work with genuine empty state; no seeded examples. An ordinary web
preview can show artwork/layout but actions must explicitly say the desktop app
is required, not silently save simulated data. Optional integrations remain
unconfigured until actually implemented and tested. Preserve chosen tab/search/
filters/open detail locally. Unfinished legacy features belong in the migration
inventory, not fake enabled buttons.
