"""Measure real release-mode card exports; not a whole-application benchmark."""
import json
from pathlib import Path
import statistics
import subprocess

root = Path(__file__).resolve().parents[1]
manifest = root / "crates/card-renderer/Cargo.toml"
subprocess.run(["cargo", "build", "--release", "--manifest-path", str(manifest), "--example", "render"], cwd=root, check=True)
metadata = subprocess.run(["cargo", "metadata", "--manifest-path", str(manifest), "--no-deps", "--format-version", "1"], cwd=root, check=True, text=True, capture_output=True)
target = Path(json.loads(metadata.stdout)["target_directory"])
exe = target / "release/examples" / ("render.exe" if __import__("os").name == "nt" else "render")
samples = []
for _ in range(5):
    run = subprocess.run([str(exe), str(root / "artifacts/benchmark")], cwd=root, check=True, text=True, capture_output=True)
    sample = {}
    for line in run.stdout.splitlines():
        kind = "gif" if ".gif " in line else "png"
        sample[kind] = float(line.rsplit(" ", 2)[-2])
    samples.append(sample)
result = {
    "resolution": {"gif": [1200, 675], "png": [2400, 1350]},
    "frames": 72,
    "loopSeconds": 3.6,
    "samplesMs": samples,
    "medianMs": {kind: statistics.median(s[kind] for s in samples) for kind in ("gif", "png")},
    "scope": "Renderer timings inside an optimized executable, excluding compilation and process launch. Different artwork cannot establish a language-only speedup.",
}
destination = root / "artifacts/render-benchmark.json"
destination.parent.mkdir(exist_ok=True)
destination.write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result, indent=2))
