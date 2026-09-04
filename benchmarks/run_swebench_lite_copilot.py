#!/usr/bin/env python3
"""Run a small, reproducible SWE-bench Lite Copilot corpus through Headroom.

The runner intentionally treats coding-agent task success and local Headroom
latency as separate measurements. Copilot receives only the published SWE-bench
problem statement. Once it exits, the harness applies the dataset's test patch
and runs the published ``FAIL_TO_PASS`` tests. It also extracts the tool-result
payloads from Copilot's JSONL transcript so they can be replayed through both
the legacy and optimized Headroom builds with ``copilot_agent_replay.py``.

This is not the official Docker-based SWE-bench evaluator. It is a portable,
targeted-test study over a fixed Lite subset for comparing an output-preserving
proxy optimization; the result must retain that scope in any write-up.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import urlencode
from urllib.request import urlopen


DATASET = "princeton-nlp/SWE-bench_Lite"
DATASET_CONFIG = "default"
DATASET_SPLIT = "test"
PYTEST_SUBSET = (
    "pytest-dev__pytest-11143",
    "pytest-dev__pytest-11148",
    "pytest-dev__pytest-8365",
    "pytest-dev__pytest-8906",
    "pytest-dev__pytest-9359",
    "pytest-dev__pytest-7220",
    "pytest-dev__pytest-7432",
    "pytest-dev__pytest-7490",
    "pytest-dev__pytest-7373",
    "pytest-dev__pytest-5413",
)
ROWS_URL = "https://datasets-server.huggingface.co/rows"


def command(args: list[str], *, cwd: Path | None = None, timeout: int | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        timeout=timeout,
    )


def fetch_instances(instance_ids: tuple[str, ...]) -> dict[str, dict[str, object]]:
    """Fetch all Lite metadata pages and return the requested fixed subset."""
    rows: list[dict[str, object]] = []
    for offset in range(0, 300, 100):
        query = urlencode(
            {
                "dataset": DATASET,
                "config": DATASET_CONFIG,
                "split": DATASET_SPLIT,
                "offset": offset,
                "length": 100,
            }
        )
        with urlopen(f"{ROWS_URL}?{query}", timeout=30) as response:
            page = json.load(response)
        rows.extend(item["row"] for item in page["rows"])

    by_id = {str(row["instance_id"]): row for row in rows}
    missing = [instance_id for instance_id in instance_ids if instance_id not in by_id]
    if missing:
        raise RuntimeError(f"SWE-bench Lite instances unavailable: {', '.join(missing)}")
    return {instance_id: by_id[instance_id] for instance_id in instance_ids}


def write(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def extract_tool_payload(transcript: Path, worktree: Path) -> tuple[str, int, int]:
    """Return a JSON-array payload plus tool-result count and source bytes."""
    results: list[dict[str, str]] = []
    for line in transcript.read_text(encoding="utf-8").splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") != "tool.execution_complete":
            continue
        data = event.get("data", {})
        result = data.get("result", {})
        content = result.get("content")
        if not isinstance(content, str):
            continue
        results.append(
            {
                "tool": str(data.get("toolCallId", "unknown")),
                "content": content.replace(str(worktree), "<WORKTREE>"),
            }
        )
    payload = json.dumps(results, ensure_ascii=False, separators=(",", ":"))
    return payload, len(results), len(payload.encode("utf-8"))


def changed_files(worktree: Path) -> list[str]:
    completed = command(["git", "diff", "--name-only"], cwd=worktree)
    return [line for line in completed.stdout.splitlines() if line]


def run_instance(
    instance: dict[str, object],
    *,
    cache: Path,
    result_root: Path,
    headroom: Path,
    port: int,
    timeout_seconds: int,
) -> dict[str, object]:
    instance_id = str(instance["instance_id"])
    task_root = result_root / instance_id
    worktree = task_root / "worktree"
    transcript = task_root / "copilot.jsonl"
    if task_root.exists():
        raise RuntimeError(f"refusing to overwrite existing result directory: {task_root}")
    task_root.mkdir(parents=True)

    base_commit = str(instance["base_commit"])
    command(["git", "worktree", "prune"], cwd=cache)
    add_worktree = command(
        ["git", "worktree", "add", "--detach", str(worktree), base_commit], cwd=cache
    )
    if add_worktree.returncode:
        write(task_root / "setup.log", add_worktree.stdout)
        return {"instance_id": instance_id, "setup_error": add_worktree.stdout}

    problem = str(instance["problem_statement"])
    prompt = (
        f"Solve the SWE-bench Lite issue {instance_id}. Work only in the "
        "repository; do not modify tests or install files globally. Understand the "
        "issue, make the smallest correct source fix, and run relevant tests before "
        f"finishing.\n\nIssue:\n\n{problem}"
    )
    started = time.monotonic()
    try:
        agent = command(
            [
                str(headroom),
                "wrap",
                "copilot",
                "--subscription",
                "--native",
                "--port",
                str(port),
                "--",
                "-C",
                str(worktree),
                "-p",
                prompt,
                "--allow-all",
                "--no-remote",
                "--no-auto-update",
                "--output-format",
                "json",
                "--log-dir",
                str(task_root / "copilot-logs"),
                "--usage-output-file",
                str(task_root / "usage.json"),
            ],
            cwd=worktree,
            timeout=timeout_seconds,
        )
        write(transcript, agent.stdout)
        agent_exit = agent.returncode
    except subprocess.TimeoutExpired as exc:
        write(transcript, (exc.stdout or "") + "\nTIMEOUT\n")
        agent_exit = None

    payload, tool_results, tool_bytes = extract_tool_payload(transcript, worktree)
    write(task_root / "tool-results.json", payload)
    files_changed = changed_files(worktree)

    test_patch = str(instance["test_patch"])
    patch = subprocess.run(
        ["git", "apply", "-"],
        cwd=worktree,
        input=test_patch,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    write(task_root / "test-patch.log", patch.stdout)
    test_exit: int | None = None
    test_output = ""
    if patch.returncode == 0:
        environment = task_root / "venv"
        setup = command([sys.executable, "-m", "venv", str(environment)])
        if setup.returncode == 0:
            install = command(
                [str(environment / "bin/python"), "-m", "pip", "install", "--quiet", str(worktree)]
            )
            targets = json.loads(str(instance["FAIL_TO_PASS"]))
            if install.returncode == 0:
                test = command(
                    [str(environment / "bin/python"), "-m", "pytest", "-q", *targets], cwd=worktree
                )
                test_exit, test_output = test.returncode, test.stdout
            else:
                test_exit, test_output = install.returncode, install.stdout
        else:
            test_exit, test_output = setup.returncode, setup.stdout
    write(task_root / "target-test.log", test_output)

    return {
        "instance_id": instance_id,
        "repo": instance["repo"],
        "base_commit": base_commit,
        "problem_sha256": hashlib.sha256(problem.encode()).hexdigest(),
        "test_patch_sha256": hashlib.sha256(test_patch.encode()).hexdigest(),
        "agent_exit": agent_exit,
        "agent_elapsed_seconds": round(time.monotonic() - started, 3),
        "target_test_exit": test_exit,
        "target_test_passed": test_exit == 0,
        "files_changed_before_test_patch": files_changed,
        "tool_result_count": tool_results,
        "tool_result_bytes": tool_bytes,
        "tool_payload_sha256": hashlib.sha256(payload.encode()).hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", type=Path, required=True, help="New directory for result artifacts")
    parser.add_argument("--cache", type=Path, required=True, help="Persistent clone cache directory")
    parser.add_argument("--headroom", type=Path, required=True, help="Headroom executable for the tested arm")
    parser.add_argument("--limit", type=int, default=len(PYTEST_SUBSET))
    parser.add_argument("--timeout-seconds", type=int, default=600)
    args = parser.parse_args()

    if args.limit < 1 or args.limit > len(PYTEST_SUBSET):
        parser.error(f"--limit must be between 1 and {len(PYTEST_SUBSET)}")
    if args.results.exists():
        parser.error(f"--results must not already exist: {args.results}")
    if not args.headroom.is_file():
        parser.error(f"Headroom executable not found: {args.headroom}")

    args.results = args.results.resolve()
    args.cache = args.cache.resolve()
    args.headroom = args.headroom.resolve()
    selected = PYTEST_SUBSET[: args.limit]
    instances = fetch_instances(selected)
    args.results.mkdir(parents=True)
    if not args.cache.exists():
        clone = command(["git", "clone", "https://github.com/pytest-dev/pytest.git", str(args.cache)], timeout=600)
        if clone.returncode:
            raise RuntimeError(clone.stdout)

    summary = {
        "dataset": DATASET,
        "config": DATASET_CONFIG,
        "split": DATASET_SPLIT,
        "agent": "GitHub Copilot CLI",
        "agent_protocol": "one non-interactive run per instance; automatic model selection",
        "evaluation": "published FAIL_TO_PASS tests after agent completion; not Docker SWE-bench evaluation",
        "results": [],
    }
    for position, instance_id in enumerate(selected):
        result = run_instance(
            instances[instance_id],
            cache=args.cache,
            result_root=args.results,
            headroom=args.headroom,
            port=9100 + position,
            timeout_seconds=args.timeout_seconds,
        )
        summary["results"].append(result)
        write(args.results / "summary.json", json.dumps(summary, indent=2, sort_keys=True) + "\n")

    passed = sum(item.get("target_test_passed") is True for item in summary["results"])
    print(f"completed {len(selected)} tasks; published target tests passed: {passed}/{len(selected)}")


if __name__ == "__main__":
    main()
