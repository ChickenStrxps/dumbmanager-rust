# Component update feed

DumbManager's Windows installer uses the [existing Tauri updater](updates.md).
The component feed is a separate contract for the optional DumbIssues,
DumbMCP host connector, and DumbMods packages. A hosted gateway reports service
compatibility over its connection. It is not a desktop package.

The feed envelope is UTF-8 JSON containing two standard Base64 strings,
`payload` and `signature`. Sign the exact bytes of the domain
`DumbManager component feed v1`, a zero byte, and the decoded payload using
Ed25519. The client receives the trusted 32-byte public key from its own
distribution configuration. A feed cannot supply or replace the public key.

The signed payload contains `schema` (currently 1), a positive monotonic
`sequence`, and a `releases` array. Each release contains these fields.

| Field | Meaning |
| --- | --- |
| `component` | `dumbissues`, `dumbmcp-host`, or `dumbmods` |
| `version` | Stable SemVer package version |
| `app` | SemVer range for the desktop app, such as `>=0.1.0, <0.2.0` |
| `contracts` | Object of protocol or format names and supported major versions |
| `target` | `any` or `windows-x86-64` |
| `url` | HTTPS artifact URL without credentials, query, or fragment |
| `size` | Signed artifact length in bytes |
| `sha256` | Lowercase hexadecimal SHA-256 of the complete download |

New feed sequences describe the available supported releases, including older
compatible choices where appropriate. Persist the highest verified sequence
with the consumer's own settings and never authorize a lower sequence. A consumer
selects the newest stable enabled release for its app and platform with an exact
match for its supported contract majors. A platform build wins a same-version
tie with a portable build. A selected release must be newer than the installed
one. Verify the complete artifact against its signed size and hash.

The current core implements signature verification, artifact verification, and
release selection. The desktop will need an authorized feed configuration and
network fetching, durable sequence tracking, staged activation, recovery, and
Settings controls to install components. Keep previous verified builds and
activation records under the Rust app's own data directory.

Run the fixture checks without an installed app, network, or signing secrets.

~~~sh
cargo test --manifest-path src-tauri/core/Cargo.toml --test component_updates
~~~
