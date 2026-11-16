#!/usr/bin/env python3
"""Read, check or increment Island's single Cargo workspace version.

This tool never commits, tags, pushes or publishes. bump-patch only changes
Cargo.toml and asks Cargo to update the workspace packages in Cargo.lock.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORKSPACE_PACKAGES = {"poni", "poni-core", "studio-diorama"}
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")
REPOSITORY = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*\Z")


def read_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    table = re.search(r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", text)
    if table is None:
        raise ValueError("Cargo.toml has no [workspace.package] table")
    match = re.search(r'^version\s*=\s*"([^"]+)"\s*$', table.group(1), re.M)
    if match is None or not VERSION.fullmatch(match.group(1)):
        raise ValueError("workspace version must be major.minor.patch without a suffix")
    return match.group(1)


def locked_versions(text):
    result = {}
    for block in re.split(r"(?m)^\[\[package\]\]\s*$", text)[1:]:
        name = re.search(r'^name = "([^"]+)"$', block, re.M)
        version = re.search(r'^version = "([^"]+)"$', block, re.M)
        if name and version and name.group(1) in WORKSPACE_PACKAGES:
            result[name.group(1)] = version.group(1)
    return result


def configured_repository():
    path = ROOT / "release/update-source.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    repository = data.get("repository", "")
    if not isinstance(repository, str) or not REPOSITORY.fullmatch(repository):
        raise ValueError("release/update-source.json must configure your GitHub owner/repo")
    return repository


def check(args):
    version = read_version()
    versions = locked_versions((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    if versions != {name: version for name in WORKSPACE_PACKAGES}:
        raise ValueError("Cargo.lock workspace versions do not match Cargo.toml; let Cargo update the lockfile")
    if args.tag is not None and args.tag != "v" + version:
        raise ValueError(f"tag must be v{version}, got {args.tag!r}")
    if args.require_notes:
        path = ROOT / f"docs/release-notes/v{version}.md"
        notes = path.read_text(encoding="utf-8").strip()
        if len(notes) < 20 or re.search(r"(?i)\b(TODO|TBD|PLACEHOLDER)\b|待补充|待填写", notes):
            raise ValueError(f"{path.relative_to(ROOT)} needs finished release notes")
    if args.require_update_source or args.repository:
        repository = configured_repository()
        if args.repository and repository.lower() != args.repository.lower():
            raise ValueError("configured update repository does not match the workflow repository")
    print(f"version {version}: checks passed")


def bump_patch():
    version = read_version()
    major, minor, patch = map(int, version.split("."))
    new_version = f"{major}.{minor}.{patch + 1}"
    if (ROOT / ".git").exists():
        tag = subprocess.run(
            ["git", "show-ref", "--verify", "--quiet", f"refs/tags/v{new_version}"], cwd=ROOT,
        )
        if tag.returncode == 0:
            raise ValueError(f"v{new_version} already exists; do not reuse a release version")
        if tag.returncode != 1:
            raise ValueError("could not check existing Git tags")
    cargo = ROOT / "Cargo.toml"
    lock = ROOT / "Cargo.lock"
    old_cargo = cargo.read_bytes()
    old_lock = lock.read_bytes()
    text = old_cargo.decode("utf-8")
    table = re.search(r"(?ms)^\[workspace\.package\]\s*\n(.*?)(?=^\[|\Z)", text)
    updated = re.sub(r'^version\s*=\s*"[^"]+"\s*$', f'version = "{new_version}"', table.group(1), count=1, flags=re.M)
    cargo.write_text(text[:table.start(1)] + updated + text[table.end(1):], encoding="utf-8")
    try:
        subprocess.run(["cargo", "update", "--workspace", "--offline"], cwd=ROOT, check=True)
        current = lock.read_text(encoding="utf-8")
        if locked_versions(current) != {name: new_version for name in WORKSPACE_PACKAGES}:
            raise ValueError("Cargo did not update all three workspace packages")
        # Cargo may format the changed workspace blocks; dependency resolution must stay identical.
        def dependencies_only(value):
            blocks = re.split(r"(?m)^\[\[package\]\]\s*$", value)
            return [
                block
                for block in blocks[1:]
                if not any(
                    re.search(r'^name = "' + name + r'"$', block, re.M)
                    for name in WORKSPACE_PACKAGES
                )
            ]
        if dependencies_only(current) != dependencies_only(old_lock.decode("utf-8")):
            raise ValueError("Cargo changed dependency locks; restored version files for review")
    except (OSError, subprocess.CalledProcessError, ValueError):
        cargo.write_bytes(old_cargo)
        lock.write_bytes(old_lock)
        raise
    print(f"{version} -> {new_version}")
    print(
        f"Next: write docs/release-notes/v{new_version}.md, run both platform checks, "
        "then explicitly authorize packaging/tagging."
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("version", help="print the Cargo workspace version")
    check_parser = commands.add_parser("check", help="check lockfile and optional release requirements")
    check_parser.add_argument("--tag", help="require this exact v<version> tag")
    check_parser.add_argument("--require-notes", action="store_true")
    check_parser.add_argument("--require-update-source", action="store_true")
    check_parser.add_argument("--repository", help="require update source to match this GitHub owner/repo")
    commands.add_parser("bump-patch", help="increment patch once and let Cargo update workspace locks")
    args = parser.parse_args()
    try:
        if args.command == "version":
            print(read_version())
        elif args.command == "check":
            check(args)
        else:
            bump_patch()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"error: {error}\n")


if __name__ == "__main__":
    main()
