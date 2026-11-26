"""Sign a component feed from verified build reports using an Ed25519 PEM key."""

import argparse
from base64 import b64encode
from hashlib import file_digest
from ipaddress import ip_address
import json
from os import link
from pathlib import Path
import re
import subprocess
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit


CONTEXT = b"DumbManager component feed v1\0"
ED25519_PUBLIC_KEY_PREFIX = bytes.fromhex("302a300506032b6570032100")
MAX_FEED_BYTES = 256 * 1024
MAX_ARTIFACT_BYTES = 512 * 1024 * 1024
COMPONENTS = {"dumbissues", "dumbmcp-host", "dumbmods"}
TARGETS = {"any", "windows-x86-64"}
STABLE = re.compile(r"(?:0|[1-9][0-9]{0,19})\.(?:0|[1-9][0-9]{0,19})\.(?:0|[1-9][0-9]{0,19})")
COMPARATOR = re.compile(r"(?:<=|>=|[<>=]) *(\S+)")
DNS_LABEL = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?")


def stable_version(value):
    return (isinstance(value, str) and STABLE.fullmatch(value)
            and all(int(part) < 2**64 for part in value.split(".")))


def app_range(value):
    if value == "*":
        return True
    if not isinstance(value, str):
        return False
    constraints = value.split(",")
    if not 1 <= len(constraints) <= 8:
        return False
    for constraint in constraints:
        match = COMPARATOR.fullmatch(constraint.strip(" "))
        if not match or not stable_version(match[1]):
            return False
    return True


def valid_host(host):
    if "%" in host:
        return False
    try:
        ip_address(host)
        return True
    except ValueError:
        names = host.removesuffix(".").split(".")
        return len(host) <= 253 and names[-1][:1].isalpha() and all(
            DNS_LABEL.fullmatch(name) for name in names
        )


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def release(spec, directory):
    build = load(directory / spec["build"])
    component, version = build["component"], build["version"]
    artifact = build["file"]
    contracts = build["contracts"]
    size, digest = build["size"], build["sha256"]
    if component not in COMPONENTS or not stable_version(version):
        raise ValueError("invalid component or stable package version")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", artifact):
        raise ValueError("artifact file must be a plain filename")
    if not contracts or any(
        not re.fullmatch(r"[a-z0-9-]{1,64}", name) or type(major) is not int or not 0 < major < 2**32
        for name, major in contracts.items()
    ):
        raise ValueError("invalid supported contract majors")
    if type(size) is not int or not 1 <= size <= MAX_ARTIFACT_BYTES:
        raise ValueError("artifact size is outside the component limit")
    build_folder = (directory / spec["build"]).parent
    with (build_folder / artifact).open("rb") as file:
        if size != file.seek(0, 2):
            raise ValueError(f"artifact size does not match the build report for {component}")
        file.seek(0)
        if file_digest(file, "sha256").hexdigest() != digest:
            raise ValueError(f"artifact hash does not match the build report for {component}")
    target, app, url = spec["target"], spec["app"], spec["url"]
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or not parsed.hostname or not valid_host(parsed.hostname)
            or parsed.username is not None or parsed.port == 0
            or parsed.query or parsed.fragment or "\\" in url or any(char.isspace() for char in url)):
        raise ValueError("use an authorized HTTPS artifact URL without credentials, query or fragment")
    if target not in TARGETS or not app_range(app):
        raise ValueError("provide a supported target and an app range using * or up to eight stable version comparators")
    return {"component": component, "version": version, "contracts": contracts,
            "size": size, "sha256": digest, "target": target, "app": app, "url": url}


def sign(specification, key, openssl):
    spec = load(specification)
    seq = spec["sequence"]
    if type(seq) is not int or not 1 <= seq < 2**64:
        raise ValueError("sequence must be a positive unsigned 64-bit integer")
    releases = [release(row, specification.parent) for row in spec["releases"]]
    unique = {(row["component"], row["version"], row["target"]) for row in releases}
    if len(unique) != len(releases):
        raise ValueError("duplicate component, version and target")
    payload = json.dumps({"schema": 1, "sequence": seq, "releases": releases},
                         sort_keys=True, separators=(",", ":")).encode("utf-8")
    if len(payload) * 4 // 3 + 128 > MAX_FEED_BYTES:
        raise ValueError("feed exceeds the desktop size limit")
    public_key = subprocess.run(
        [openssl, "pkey", "-in", str(key), "-pubout", "-outform", "DER"],
        stdin=subprocess.DEVNULL, capture_output=True, check=False,
    )
    if (public_key.returncode or len(public_key.stdout) != len(ED25519_PUBLIC_KEY_PREFIX) + 32
            or not public_key.stdout.startswith(ED25519_PUBLIC_KEY_PREFIX)):
        raise ValueError("provide an Ed25519 PEM key readable by OpenSSL")
    with TemporaryDirectory() as scratch:
        message = Path(scratch) / "message"
        message.write_bytes(CONTEXT + payload)
        proc = subprocess.run(
            [openssl, "pkeyutl", "-sign", "-rawin", "-inkey", str(key), "-in", str(message)],
            stdin=subprocess.DEVNULL, capture_output=True, check=False,
        )
        if proc.returncode or len(proc.stdout) != 64:
            raise ValueError("feed signing failed. Provide an Ed25519 key readable by OpenSSL")
    envelope = json.dumps({"payload": b64encode(payload).decode("ascii"),
                           "signature": b64encode(proc.stdout).decode("ascii")}) + "\n"
    if len(envelope) > MAX_FEED_BYTES:
        raise ValueError("feed exceeds the desktop size limit")
    return envelope


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("specification", type=Path)
    parser.add_argument("--key", type=Path, required=True, help="Ed25519 PEM signing key")
    parser.add_argument("--openssl", default="openssl", help="OpenSSL executable")
    parser.add_argument("--output", type=Path, required=True, help="new feed envelope file")
    args = parser.parse_args()
    try:
        envelope = sign(args.specification, args.key, args.openssl)
        with TemporaryDirectory(dir=args.output.parent, prefix=".component-feed-") as scratch:
            prepared = Path(scratch) / "feed.json"
            prepared.write_text(envelope, encoding="utf-8", newline="\n")
            link(prepared, args.output)
    except (KeyError, TypeError, OSError, ValueError) as exc:
        parser.error(str(exc))


if __name__ == "__main__":
    main()
