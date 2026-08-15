#!/usr/bin/env python3
"""Compare migration identities and contents across the two fixed trees."""

from __future__ import annotations

import csv
import hashlib
import re
import subprocess
from collections import Counter
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
REPO_DIR = AUDIT_DIR.parents[2]
GENERATED_DIR = AUDIT_DIR / "generated"
TIPS = {
    "niffler": "908443291a2826b57286f56f1555fd10e922c0b3",
    "aether": "654c4f69789f02d08e926a77338f1b94f34f8658",
}
DRIVERS = ("postgres", "mysql", "sqlite")


def git_bytes(*args: str) -> bytes:
    return subprocess.check_output(["git", *args], cwd=REPO_DIR)


def migration_rows(branch: str, revision: str) -> list[dict[str, str]]:
    paths = git_bytes("ls-tree", "-r", "--name-only", "-z", revision).split(b"\0")
    rows = []
    for raw_path in paths:
        if not raw_path:
            continue
        path = raw_path.decode("utf-8", errors="replace")
        if "/migrations/" not in path or not path.endswith(".sql"):
            continue
        match = re.match(r"(\d+)", path.rsplit("/", 1)[-1])
        if not match:
            continue
        driver = next(
            (
                item
                for item in DRIVERS
                if f"/{item}/" in path or f"adapters/{item}/" in path
            ),
            "unknown",
        )
        content = git_bytes("show", f"{revision}:{path}")
        rows.append(
            {
                "branch": branch,
                "revision": revision,
                "version": match.group(1),
                "driver": driver,
                "path": path,
                "sha256": hashlib.sha256(content).hexdigest(),
                "bytes": str(len(content)),
                "lines": str(content.count(b"\n")),
            }
        )
    return sorted(rows, key=lambda row: (row["version"], row["driver"], row["path"]))


def write_tsv(path: Path, rows: list[dict[str, str]], fields: list[str]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fields, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)


def main() -> None:
    GENERATED_DIR.mkdir(parents=True, exist_ok=True)
    inventory = [
        row
        for branch, revision in TIPS.items()
        for row in migration_rows(branch, revision)
    ]
    key_counts = Counter(
        (row["branch"], row["version"], row["driver"]) for row in inventory
    )
    duplicates = [key for key, count in key_counts.items() if count > 1]
    if duplicates:
        raise RuntimeError(f"duplicate migration identity: {duplicates}")

    by_branch = {
        branch: {
            (row["version"], row["driver"]): row
            for row in inventory
            if row["branch"] == branch
        }
        for branch in TIPS
    }
    comparison = []
    for version, driver in sorted(set().union(*(set(rows) for rows in by_branch.values()))):
        niffler = by_branch["niffler"].get((version, driver))
        aether = by_branch["aether"].get((version, driver))
        if niffler and aether:
            relationship = "same_content" if niffler["sha256"] == aether["sha256"] else "same_id_different_content"
        elif niffler:
            relationship = "niffler_only"
        else:
            relationship = "aether_only"
        comparison.append(
            {
                "version": version,
                "driver": driver,
                "relationship": relationship,
                "niffler_path": niffler["path"] if niffler else "",
                "niffler_sha256": niffler["sha256"] if niffler else "",
                "aether_path": aether["path"] if aether else "",
                "aether_sha256": aether["sha256"] if aether else "",
            }
        )

    write_tsv(
        GENERATED_DIR / "migration_inventory.tsv",
        inventory,
        ["branch", "revision", "version", "driver", "path", "sha256", "bytes", "lines"],
    )
    write_tsv(
        GENERATED_DIR / "migration_comparison.tsv",
        comparison,
        [
            "version",
            "driver",
            "relationship",
            "niffler_path",
            "niffler_sha256",
            "aether_path",
            "aether_sha256",
        ],
    )
    counts = Counter(row["relationship"] for row in comparison)
    print(
        "PASS migration inventory "
        + " ".join(f"{name}={count}" for name, count in sorted(counts.items()))
    )


if __name__ == "__main__":
    main()
