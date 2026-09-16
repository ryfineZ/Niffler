#!/usr/bin/env python3
"""Associate every changed path with the unique commits and feature clusters that touched it."""

from __future__ import annotations

import csv
import subprocess
from collections import Counter, defaultdict
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
REPO_DIR = AUDIT_DIR.parents[2]
GENERATED_DIR = AUDIT_DIR / "generated"


def git_paths(first_parent: str, commit: str) -> list[str]:
    raw = subprocess.check_output(
        [
            "git",
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            first_parent,
            commit,
        ],
        cwd=REPO_DIR,
    )
    return [item.decode("utf-8", errors="replace") for item in raw.split(b"\0") if item]


def read_tsv(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def write_tsv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle, fieldnames=fields, delimiter="\t", lineterminator="\n"
        )
        writer.writeheader()
        writer.writerows(rows)


def map_branch(branch: str) -> None:
    catalog = read_tsv(GENERATED_DIR / f"{branch}_commit_catalog.tsv")
    path_inventory = {
        row["path"]: row
        for row in read_tsv(GENERATED_DIR / "path_inventory.tsv")
    }
    by_path: dict[str, list[dict[str, str]]] = defaultdict(list)
    cluster_path_counts: Counter[tuple[str, str]] = Counter()

    for row in catalog:
        for path in git_paths(row["first_parent"], row["commit"]):
            by_path[path].append(row)
            cluster_path_counts[(row["feature_cluster"], path)] += 1

    rows: list[dict[str, object]] = []
    for path in sorted(by_path):
        commits = sorted(by_path[path], key=lambda item: (item["date"], item["commit"]))
        clusters = Counter(item["feature_cluster"] for item in commits)
        inventory = path_inventory.get(path, {})
        rows.append(
            {
                "branch": branch,
                "path": path,
                "primary_subsystem": inventory.get("primary_subsystem", "unclassified"),
                "change_scope": inventory.get("change_scope", "outside_final_diff"),
                "final_exists": inventory.get(f"{branch}_exists", "unknown"),
                "commit_count": len(commits),
                "feature_clusters": ",".join(
                    f"{cluster}:{count}" for cluster, count in sorted(clusters.items())
                ),
                "first_commit": commits[0]["commit"],
                "first_date": commits[0]["date"],
                "last_commit": commits[-1]["commit"],
                "last_date": commits[-1]["date"],
                "last_subject": commits[-1]["subject"],
                "commits": ",".join(item["commit"] for item in commits),
            }
        )

    fields = [
        "branch",
        "path",
        "primary_subsystem",
        "change_scope",
        "final_exists",
        "commit_count",
        "feature_clusters",
        "first_commit",
        "first_date",
        "last_commit",
        "last_date",
        "last_subject",
        "commits",
    ]
    write_tsv(GENERATED_DIR / f"{branch}_path_commit_map.tsv", rows, fields)

    cluster_rows: list[dict[str, object]] = []
    for cluster in sorted({row["feature_cluster"] for row in catalog}):
        paths = sorted(path for item_cluster, path in cluster_path_counts if item_cluster == cluster)
        subsystem_counts = Counter(
            path_inventory.get(path, {}).get("primary_subsystem", "unclassified")
            for path in paths
        )
        cluster_rows.append(
            {
                "branch": branch,
                "feature_cluster": cluster,
                "distinct_paths": len(paths),
                "top_subsystems": ",".join(
                    f"{name}:{count}" for name, count in subsystem_counts.most_common(8)
                ),
                "paths": ",".join(paths),
            }
        )
    write_tsv(
        GENERATED_DIR / f"{branch}_cluster_paths.tsv",
        cluster_rows,
        ["branch", "feature_cluster", "distinct_paths", "top_subsystems", "paths"],
    )


def main() -> None:
    map_branch("niffler")
    map_branch("aether")


if __name__ == "__main__":
    main()
