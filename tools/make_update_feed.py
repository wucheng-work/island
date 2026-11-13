#!/usr/bin/env python3
"""Sign Island's schema-1 update feed from complete local release artifacts.

No upstream or GitHub API fallback is used. The repository comes exclusively
from release/update-source.json; the signing key must match a public key in
release/update-keys.txt. UPDATE_FEED_KEY contains the private P-256 PEM key.

    python3 tools/make_update_feed.py --assets dist --check
    python3 tools/make_update_feed.py dist/latest.json --assets dist
"""

import argparse
import base64
import datetime
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

from release import ROOT, configured_repository, read_version

KEYS = ROOT / "release/update-keys.txt"
SCHEMA = 1
HEADLINE_MAX = 90
P256_SPKI_PREFIX = bytes.fromhex("3059301306072a8648ce3d020106082a8648ce3d030107034200")


def headline(notes, version):
    match = re.search(r"\*\*(.+?)\*\*", notes, re.S)
    text = " ".join(match.group(1).split()).rstrip(".") if match else f"Island {version}"
    if len(text) > HEADLINE_MAX:
        text = text[:HEADLINE_MAX - 1].rstrip() + "…"
    return text


def local_asset(directory, name, repository, version):
    path = directory / name
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"missing local release asset: {name}")
    size = path.stat().st_size
    if size == 0:
        raise ValueError(f"release asset is empty: {name}")
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return {
        "url": f"https://github.com/{repository}/releases/download/v{version}/{name}",
        "size": size,
        "sha256": digest.hexdigest(),
    }


def payload(args):
    version = read_version()
    if args.version and args.version != version:
        raise ValueError(f"--version must match Cargo.toml ({version})")
    repository = configured_repository()
    workflow_repository = os.environ.get("GITHUB_REPOSITORY", "")
    if workflow_repository and workflow_repository.lower() != repository.lower():
        raise ValueError("the configured update repository differs from GITHUB_REPOSITORY")
    notes_path = args.notes or ROOT / f"docs/release-notes/v{version}.md"
    notes = notes_path.read_text(encoding="utf-8").strip()
    if len(notes) < 20 or re.search(r"(?i)\b(TODO|TBD|PLACEHOLDER)\b|待补充|待填写", notes):
        raise ValueError("finished release notes are required")
    day = args.date or datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    if datetime.date.fromisoformat(day).isoformat() != day:
        raise ValueError("--date must be YYYY-MM-DD in UTC")
    windows = local_asset(args.assets, f"Island-For-Windows-Setup-{version}.exe", repository, version)
    mac = local_asset(args.assets, f"Island-For-Mac-{version}.dmg", repository, version)
    # The portable package is part of the complete release, although clients install the NSIS/DMG assets.
    local_asset(args.assets, f"Island-For-Windows-{version}-x64.zip", repository, version)
    return {
        "schema": SCHEMA,
        "version": version,
        "date": day,
        "headline": headline(notes, version),
        "notes": f"https://github.com/{repository}/releases/tag/v{version}",
        "windows": windows,
        "mac": mac,
    }


def trusted_keys():
    keys = {}
    for number, line in enumerate(KEYS.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r"[0-9a-f]{16}", parts[0]) or not re.fullmatch(r"04[0-9a-f]{128}", parts[1]):
            raise ValueError(f"invalid public key at release/update-keys.txt:{number}")
        public = bytes.fromhex(parts[1])
        if hashlib.sha256(public).hexdigest()[:16] != parts[0] or parts[0] in keys:
            raise ValueError(f"invalid or duplicate key id at release/update-keys.txt:{number}")
        keys[parts[0]] = public
    if not keys:
        raise ValueError("release/update-keys.txt has no production signing key")
    return keys


def openssl(*args):
    result = subprocess.run(["openssl", *args], capture_output=True)
    if result.returncode != 0:
        raise ValueError(f"openssl {args[0]} failed: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def der_to_raw(der):
    """Convert a P-256 ECDSA DER sequence to its fixed-size r || s encoding."""
    if len(der) < 8 or der[0] != 0x30 or der[1] != len(der) - 2:
        raise ValueError("signature is not a P-256 DER sequence")
    position = 2
    output = b""
    for _ in range(2):
        if position + 2 > len(der) or der[position] != 0x02:
            raise ValueError("signature does not contain two DER integers")
        size = der[position + 1]
        integer = der[position + 2:position + 2 + size].lstrip(b"\0")
        if not integer or len(integer) > 32:
            raise ValueError("signature integer is not P-256 sized")
        output += integer.rjust(32, b"\0")
        position += 2 + size
    if position != len(der):
        raise ValueError("signature has trailing bytes")
    return output


def sign(text):
    keys = trusted_keys()
    pem = os.environ.get("UPDATE_FEED_KEY", "")
    if "PRIVATE KEY" not in pem:
        raise ValueError("UPDATE_FEED_KEY must be set in the GitHub release environment")
    with tempfile.TemporaryDirectory() as directory:
        directory = Path(directory)
        key = directory / "key.pem"
        descriptor = os.open(str(key), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as file:
            file.write(pem if pem.endswith("\n") else pem + "\n")
        public_der = openssl("ec", "-in", str(key), "-pubout", "-outform", "DER")
        if not public_der.startswith(P256_SPKI_PREFIX) or len(public_der) != len(P256_SPKI_PREFIX) + 65:
            raise ValueError("UPDATE_FEED_KEY is not an ECDSA prime256v1/P-256 key")
        public = public_der[len(P256_SPKI_PREFIX):]
        key_id = hashlib.sha256(public).hexdigest()[:16]
        if keys.get(key_id) != public:
            raise ValueError(f"key {key_id} is not trusted by release/update-keys.txt")
        data = directory / "payload"
        data.write_bytes(text.encode("utf-8"))
        der = openssl("dgst", "-sha256", "-sign", str(key), str(data))
        pub = directory / "pub.pem"
        pub.write_bytes(openssl("ec", "-in", str(key), "-pubout"))
        signature = directory / "sig.der"
        signature.write_bytes(der)
        openssl("dgst", "-sha256", "-verify", str(pub), "-signature", str(signature), str(data))
    return key_id, base64.b64encode(der_to_raw(der)).decode("ascii")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", nargs="?", type=Path)
    parser.add_argument("--assets", type=Path, required=True, help="directory holding all three release packages")
    parser.add_argument("--version", help="optional assertion of the current Cargo version")
    parser.add_argument("--notes", type=Path, help="defaults to docs/release-notes/v<version>.md")
    parser.add_argument("--date", help="release date in UTC, YYYY-MM-DD")
    parser.add_argument("--check", action="store_true", help="print the unsigned payload; never signs or writes")
    args = parser.parse_args()
    if args.check == bool(args.output):
        parser.error("choose either OUTPUT.json or --check")
    try:
        current = payload(args)
        text = json.dumps(current, ensure_ascii=False, separators=(",", ":"))
        if args.check:
            print(json.dumps(current, ensure_ascii=False, indent=2))
            return
        key_id, signature = sign(text)
        feed = {
            "about": "Island update feed. The exact UTF-8 payload is signed with ECDSA P-256/SHA-256; see docs/releases.md.",
            "payload": text,
            "signature": signature,
            "key": key_id,
        }
        args.output.write_text(json.dumps(feed, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"latest.json: v{current['version']}, complete Windows and Mac assets, key {key_id}")
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")


if __name__ == "__main__":
    main()
