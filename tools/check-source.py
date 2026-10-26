"""Bounded pre-push check of this repository's tracked source only."""
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
tracked = subprocess.run(["git", "ls-files", "-z"], cwd=root, capture_output=True, check=True).stdout.split(b"\0")
rules = {
    "credential shape": re.compile(rb"(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{35,}|sk-[A-Za-z0-9_-]{24,}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)"),
    "personal Windows path": re.compile(rb"[A-Za-z]:[\\/]+Users[\\/]+(?!Public(?:[\\/]|$)|Default(?:[\\/]|$))[A-Za-z0-9_.-]+[\\/]", re.I),
}
excluded_extensions = {".png", ".gif", ".ico", ".ttf", ".woff2"}
failures = []
checked = 0
for item in tracked:
    if not item:
        continue
    relative = item.decode("utf-8")
    path = root / relative
    if any(part in {".qa", "artifacts", "node_modules", "target", ".git"} for part in Path(relative).parts):
        failures.append((relative, "runtime/build directory tracked"))
        continue
    if path.suffix in {".db", ".key", ".pem", ".pfx"} or path.name.startswith(".env") and path.name != ".env.example":
        failures.append((relative, "runtime/credential file tracked"))
        continue
    if not path.is_file() or path.suffix in excluded_extensions or path.stat().st_size > 10_000_000:
        continue
    data = path.read_bytes()
    checked += 1
    for label, pattern in rules.items():
        if pattern.search(data):
            failures.append((relative, label))
if failures:
    for path, label in failures:
        print(f"{path}: {label}")
    sys.exit(1)
print(f"Checked {checked} tracked source files; no blocked path, credential shape or runtime file found.")
