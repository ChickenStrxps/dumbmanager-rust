# Performance measurements

Rust is a way to build and maintain the native application, not a promised speed
multiplier. The web UI still runs in Windows WebView2. Rendering too many glyphs,
blocking the UI thread or making slow network calls remains expensive.

## Local renderer experiment

Measured on the same Windows x64 machine using synthetic text and temporary state:

| Operation | Samples | Median |
| --- | ---: | ---: |
| Current uncached GIF renderer | 3 | 1908.55 ms |
| Native Rust GIF prototype, release build | 5 | 224.5 ms |
| Native Rust PNG prototype, release build | 5 | 7.5 ms |

Both GIFs are 1200x675, 72 frames and a 3.6-second loop. The Rust PNG is 2400x1350.
The current renderer starts a dedicated disposable browser, draws and encodes the
card, and closes that browser. The prototype draws and encodes directly in Rust.
The fonts, scene and drawing algorithms differ. These results describe uncached
export latency in this experiment; they do not isolate the language or establish
a whole-app speedup. Cached exports in the current app can already be much faster.

The first 40-frame prototype took about 123 ms; it was then increased to 72 frames
before the measurements above. Do not compare that early figure with the current
renderer without mentioning the frame-count difference.

Reproduce native measurements with `py tools/benchmark-render.py`. The tool builds
the optimized renderer, produces five real GIF/PNG pairs under ignored
`artifacts/benchmark/`, and records measurements in `artifacts/render-benchmark.json`.
Compilation is excluded. Rust 1.95.0 was used for these measurements.

## Desktop comparison still needed

There is no measured overall startup, idle memory, CPU or interaction-latency
advantage yet. A fair desktop comparison should include the full process tree,
the same content, window size, motion setting and warm/cold state. Measure time
to usable controls, not just time to spawn a process. Include WebView2 processes
and the original app's browser/native helpers in memory totals.

The current backend's warm-import time to its first local health response was
78.58 ms median in three disposable runs. That excludes Python import, browser,
window and native helpers and must not be compared with desktop window-ready time.
