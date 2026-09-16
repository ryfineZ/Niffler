#!/usr/bin/env python3
"""Fail closed when the generated divergence audit loses coverage or consistency."""

from __future__ import annotations

import csv
import subprocess
from collections import Counter
from pathlib import Path


AUDIT_DIR = Path(__file__).resolve().parent
REPO_DIR = AUDIT_DIR.parents[2]
GENERATED_DIR = AUDIT_DIR / "generated"
BASE = "ed75ae6d56ab03eb5e6e3cd87f2137880c99694d"
TIPS = {
    "niffler": "908443291a2826b57286f56f1555fd10e922c0b3",
    "aether": "654c4f69789f02d08e926a77338f1b94f34f8658",
}
ALLOWED_DECISIONS = {
    "niffler": {
        "KEEP",
        "KEEP_REBASE",
        "REPLACE_UPSTREAM",
        "REMOVE",
        "RESTORE_UPSTREAM",
        "DECISION_REQUIRED",
        "SPLIT",
        "HISTORY_ONLY",
    },
    "aether": {
        "ABSORB_DIRECT",
        "ABSORB_SEMANTIC",
        "ABSORB_AFTER_FOUNDATION",
        "DEFER",
        "REJECT",
        "ALREADY_EQUIVALENT",
        "SPLIT",
        "HISTORY_ONLY",
    },
}


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=REPO_DIR).decode()


def read_tsv(name: str) -> list[dict[str, str]]:
    with (GENERATED_DIR / name).open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle, delimiter="\t"))


def git_path_set(old: str, new: str) -> set[str]:
    raw = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z", old, new],
        cwd=REPO_DIR,
    )
    return {
        item.decode("utf-8", errors="replace")
        for item in raw.split(b"\0")
        if item
    }


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def validate_branch(branch: str, tip: str) -> dict[str, int]:
    live_commits = {
        item for item in git("rev-list", f"{BASE}..{tip}").splitlines() if item
    }
    tables = {
        "commits": read_tsv(f"{branch}_commits.tsv"),
        "impacts": read_tsv(f"{branch}_commit_impacts.tsv"),
        "catalog": read_tsv(f"{branch}_commit_catalog.tsv"),
        "decisions": read_tsv(f"{branch}_commit_decisions.tsv"),
    }
    for name, rows in tables.items():
        commits = [row["commit"] for row in rows]
        require(len(commits) == len(set(commits)), f"{branch}/{name} has duplicates")
        require(set(commits) == live_commits, f"{branch}/{name} commit coverage differs")

    decisions = tables["decisions"]
    for row in decisions:
        decision = row["recommended_disposition"]
        require(
            decision in ALLOWED_DECISIONS[branch],
            f"{branch} invalid decision {decision}: {row['commit']}",
        )
        if row["is_merge"] == "yes":
            require(
                row["feature_cluster"] == "integration_merge",
                f"{branch} merge classified as feature: {row['commit']}",
            )
            require(decision == "HISTORY_ONLY", f"{branch} merge is actionable")
        else:
            require(decision != "HISTORY_ONLY", f"{branch} non-merge hidden as history")
        require(row["first_parent"], f"{branch} missing first parent: {row['commit']}")
        require(not row["feature_cluster"].startswith("misc"), f"{branch} misc cluster remains")

    return {
        "commits": len(decisions),
        "non_merges": sum(row["is_merge"] == "no" for row in decisions),
        "merges": sum(row["is_merge"] == "yes" for row in decisions),
    }


def main() -> None:
    require(
        git("rev-parse", "--is-shallow-repository").strip() == "false",
        "audit repository is shallow",
    )
    require(git("merge-base", *TIPS.values()).strip() == BASE, "merge base changed")
    for tip in TIPS.values():
        require(git("cat-file", "-t", tip).strip() == "commit", f"missing tip {tip}")

    branch_stats = {
        branch: validate_branch(branch, tip) for branch, tip in TIPS.items()
    }

    inventory = read_tsv("path_inventory.tsv")
    inventory_paths = [row["path"] for row in inventory]
    require(len(inventory_paths) == len(set(inventory_paths)), "duplicate inventory path")
    live_sets = {branch: git_path_set(BASE, tip) for branch, tip in TIPS.items()}
    require(set(inventory_paths) == set().union(*live_sets.values()), "path union differs")

    coverage = read_tsv("path_coverage_ledger.tsv")
    require(
        {row["path"] for row in coverage} == set(inventory_paths),
        "coverage ledger path set differs",
    )
    require(
        all(row["coverage_state"] == "mapped" for row in coverage),
        "incomplete path provenance remains",
    )
    for row in coverage:
        for branch in TIPS:
            changed = bool(row[f"{branch}_status"])
            provenance = row[f"{branch}_provenance"]
            if changed:
                require(provenance == "mapped", f"{branch} provenance mismatch: {row['path']}")
            else:
                require(
                    provenance in {"historical_only", "not_changed_on_branch"},
                    f"{branch} provenance mismatch: {row['path']}",
                )

    final_different = git_path_set(TIPS["niffler"], TIPS["aether"])
    scopes = Counter(row["change_scope"] for row in inventory)
    risks = Counter(row["conflict_risk"] for row in inventory)
    print("PASS audit coverage and consistency")
    print(f"base={BASE}")
    for branch, stats in branch_stats.items():
        print(
            f"{branch}: commits={stats['commits']} "
            f"non_merges={stats['non_merges']} merges={stats['merges']} "
            f"changed_paths={len(live_sets[branch])}"
        )
    print(
        f"path_union={len(inventory)} final_different={len(final_different)} "
        f"coverage={len(coverage)}"
    )
    print("scopes=" + ",".join(f"{key}:{value}" for key, value in sorted(scopes.items())))
    print("risks=" + ",".join(f"{key}:{value}" for key, value in sorted(risks.items())))


if __name__ == "__main__":
    main()
