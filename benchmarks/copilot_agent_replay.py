#!/usr/bin/env python3
"""Replay captured Copilot JSON tool outputs through adaptive SmartCrusher.

Pass one or more JSON-array payloads captured from a real Copilot CLI session.
The harness uses ``SmartCrusher.without_compaction`` to isolate the adaptive
sampling fallback, then emits JSON timing summaries and output hashes. Run the
same command from wheels built with and without ``legacy-simhash-benchmark``.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import statistics
import time
from pathlib import Path

from headroom._core import SmartCrusher
from scenarios.copilot_agent_tool_outputs import captured_payloads


def percentile(samples: list[float], fraction: float) -> float:
    ordered = sorted(samples)
    position = (len(ordered) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def benchmark_payload(payload_name: str, payload: str, warmup: int, iterations: int) -> dict[str, object]:
    records = json.loads(payload)
    if not isinstance(records, list):
        raise ValueError(f"{payload_name} must contain a JSON array")

    crusher = SmartCrusher.without_compaction()
    for _ in range(warmup):
        crusher.crush_array_json(payload)

    samples_ms: list[float] = []
    output_hashes: set[str] = set()
    strategies: set[str] = set()
    output_record_counts: set[int] = set()
    for _ in range(iterations):
        started_ns = time.perf_counter_ns()
        result = crusher.crush_array_json(payload)
        samples_ms.append((time.perf_counter_ns() - started_ns) / 1_000_000)
        output = result["items"]
        output_hashes.add(hashlib.sha256(output.encode("utf-8")).hexdigest())
        strategies.add(result["strategy_info"])
        output_record_counts.add(len(json.loads(output)))

    if len(output_hashes) != 1 or len(strategies) != 1 or len(output_record_counts) != 1:
        raise RuntimeError(f"non-deterministic result while replaying {payload_name}")

    return {
        "payload": payload_name,
        "input_sha256": hashlib.sha256(payload.encode("utf-8")).hexdigest(),
        "input_records": len(records),
        "input_bytes": len(payload.encode("utf-8")),
        "strategy": strategies.pop(),
        "output_records": output_record_counts.pop(),
        "output_sha256": output_hashes.pop(),
        "iterations": iterations,
        "median_ms": round(statistics.median(samples_ms), 6),
        "p10_ms": round(percentile(samples_ms, 0.10), 6),
        "p90_ms": round(percentile(samples_ms, 0.90), 6),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("payload", nargs="*", type=Path, help="Captured JSON-array tool output")
    parser.add_argument(
        "--captured-corpus",
        action="store_true",
        help="Replay the three hash-verified Copilot CLI tool outputs in benchmarks/scenarios",
    )
    parser.add_argument("--warmup", type=int, default=20)
    parser.add_argument("--iterations", type=int, default=300)
    parser.add_argument("--output", type=Path, help="Write the JSON summary to this file")
    args = parser.parse_args()

    if args.warmup < 0 or args.iterations < 1:
        parser.error("--warmup must be non-negative and --iterations must be positive")

    payloads = [(path.name, path.read_text(encoding="utf-8")) for path in args.payload]
    if args.captured_corpus:
        payloads.extend(captured_payloads())
    if not payloads:
        parser.error("provide payload paths or --captured-corpus")

    results = [
        benchmark_payload(payload_name, payload, args.warmup, args.iterations)
        for payload_name, payload in payloads
    ]
    rendered = json.dumps({"results": results}, indent=2, sort_keys=True)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered + "\n", encoding="utf-8")
    print(rendered)


if __name__ == "__main__":
    main()
