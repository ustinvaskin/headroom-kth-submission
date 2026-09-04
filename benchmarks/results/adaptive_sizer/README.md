# Adaptive Sizer Experiment Artifacts

These files preserve the evidence behind `EXPERIMENT_LOG.md` so the narrative can be rewritten without losing commands, measurements, failures, or decisions.

## Provenance

- Steps 1-5 were reconstructed on 2026-09-02 from the chronological experiment log after the original terminal sessions completed.
- Step 6 was captured from the experiment log and the terminal results produced during implementation.
- Reconstructed files are evidence snapshots, not byte-for-byte raw terminal transcripts.
- Criterion's generated `target/criterion` directory was not available when this archive was created.

## Environment

- Apple Silicon macOS
- Seed: `20260902`
- Rust/Cargo: `1.95.0`
- Criterion: `0.8.2`
- Rust commands require:

```bash
PATH="$HOME/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin:$PATH"
```

## Files

- `step-01-python-clustering.txt`: Python scaling check and indexed prototype
- `step-02-rust-baseline.txt`: toolchain setup and unchanged test baseline
- `step-03-greedy-baseline.txt`: original full-operation Criterion baseline
- `step-04-component-split.txt`: SimHash versus clustering decomposition
- `step-05-simhash-stages.txt`: preprocessing, gram hashing, and voting decomposition
- `step-06-ascii-fast-path.txt`: implementation validation and before/after results
- `step-07-coding-agent-live-zone.txt`: real production-route behavior proof and latency comparison
- `step-08-reproducible-baseline.txt`: feature-based reproducible baseline and superseding results
- `step-09-reproducible-internal-baseline.txt`: final-tree internal operation comparison
- `step-10-real-copilot-agent-replay.txt`: replay of hash-verified JSON tool output from three successful Copilot CLI repair tasks
- `step-12-log-search-route-benchmark.txt`: standard-sample production-route LogCompressor and SearchCompressor comparisons
- `step-13-live-copilot-agent-ab.txt`: direct native-proxy Copilot task-success A/B and agent-level limitations
- `step-14-high-cardinality-scaling.txt`: worst-case diverse scaling and the clustering materiality crossover
- `swebench_lite_pytest_10/`: a fixed 10-instance SWE-bench Lite Copilot corpus,
  including per-task target-test logs, captured tool payloads, and paired
  legacy/optimized replay summaries

Keep these files unchanged when rewriting `EXPERIMENT_LOG.md`. Add future raw command output as a new numbered artifact rather than replacing prior evidence.
