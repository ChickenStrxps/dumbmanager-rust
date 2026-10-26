"""Collect dependency licenses for the executable and renderer into one notice."""
from pathlib import Path
import json
import subprocess

root = Path(__file__).resolve().parents[1]
override_file = root / "tools/license-overrides.json"
overrides = json.loads(override_file.read_text(encoding="utf-8")) if override_file.is_file() else {}
packages = {}
for relative in ("src-tauri/Cargo.toml", "crates/card-renderer/Cargo.toml"):
    manifest = root / relative
    if not manifest.exists():
        continue
    result = subprocess.run(["cargo", "metadata", "--manifest-path", str(manifest), "--format-version", "1"], cwd=root, check=True, capture_output=True, text=True, encoding="utf-8")
    for package in json.loads(result.stdout)["packages"]:
        if package.get("source"):
            packages[package["id"]] = package

sections = ["DumbManager Rust - third-party notices\n\nApplication source is licensed under MIT.\n\n"]
font_notices = sorted((root / "web/fonts").glob("*OFL.txt"))
if not font_notices:
    raise RuntimeError("Bundled font license notices are missing")
for font_notice in font_notices:
    sections.append(f"Bundled font notice: {font_notice.name}\n\n" + font_notice.read_text(encoding="utf-8") + "\n")
sections.append("\nThe following Cargo dependencies may be included across supported targets and build tools.\n")
missing = []
for package in sorted(packages.values(), key=lambda p: (p["name"], p["version"])):
    base = Path(package["manifest_path"]).parent
    license_paths = []
    for path in base.iterdir():
        if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")):
            license_paths.append(path)
    if package.get("license_file"):
        specific = (base / package["license_file"]).resolve()
        if specific.is_relative_to(base.resolve()) and specific.is_file() and specific not in license_paths:
            license_paths.append(specific)
    key = f"{package['name']}@{package['version']}"
    for relative in overrides.get(key, []):
        supplied = (root / relative).resolve()
        if not supplied.is_relative_to((root / "licenses/upstream").resolve()) or not supplied.is_file():
            raise RuntimeError(f"Invalid checked-in license override for {key}")
        if supplied not in license_paths:
            license_paths.append(supplied)
    heading = f"\n{'=' * 72}\n{package['name']} {package['version']}\nLicense: {package.get('license') or 'See below'}\n{package.get('repository') or ''}\n"
    sections.append(heading)
    if not license_paths:
        missing.append(package["name"])
        sections.append("License text is provided in the upstream project referenced above.\n")
    for path in sorted(license_paths):
        if path.stat().st_size > 150_000:
            raise RuntimeError(f"Unexpectedly large license file in {package['name']}")
        sections.append(f"\n--- {path.name} ---\n{path.read_text(encoding='utf-8', errors='replace')}\n")

if missing:
    raise RuntimeError("Missing license text; add a reviewed offline override: " + ", ".join(missing))
destination = root / "THIRD_PARTY_NOTICES.txt"
destination.write_text("".join(sections), encoding="utf-8")
print(f"Collected {len(packages)} dependencies into {destination.name}.")
