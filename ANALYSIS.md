# Optimizing Adaptive SimHash for Coding-Agent Tool Output

## Result

An output-preserving ASCII SimHash fast path reduced complete local Headroom
compression latency by **17–28%** across the SmartCrusher, LogCompressor, and
SearchCompressor routes evaluated here. At 5,000 records, it also reduced the
underlying `count_unique_simhash` operation by **27–29%**.

The compressed request, token count, and compression strategy remain
identical. This is a local Headroom-processing result, not a claim about model
inference latency, coding quality, or end-to-end coding-agent wall time.

## Premise

Headroom is a compression layer for AI agents. It reduces large tool results,
logs, search results, files, and conversation context before they reach a model.

For this task, I looked for a real performance issue in an existing production
path. I focused on Rust adaptive sizing because it helps decide how many records
to keep when Headroom compresses JSON arrays, logs, and search results.

The final change is small: for ASCII text, SimHash now hashes four-byte windows
directly instead of creating a temporary string for every window. It keeps the
compressed request identical while reducing the local time needed to create it.

This does not assume that users write ASCII. The target is ASCII-heavy *tool
output*: source code, English logs, JSON keys, file paths, IDs, and similar
coding-agent output. Unicode input stays on the existing safe path.

## Interesting Features Exercised

* **Adaptive SimHash sizing:** traced `compute_optimal_k` and its diversity
  estimator, `count_unique_simhash`, in the Rust implementation.
* **SmartCrusher JSON routing:** exercised content detection, adaptive sizing,
  request reconstruction, and token validation through the complete local
  compression route.
* **Log and search compression:** measured the same shared estimator through
  the `LogCompressor` and `SearchCompressor` production routes.
* **Coding-agent integration:** replayed captured Copilot CLI tool output and
  ran a small direct Headroom-wrapped Copilot task-success control.

## Initial hypothesis

The first suspicious part was the greedy comparison loop in
[`count_unique_simhash`](crates/headroom-core/src/transforms/adaptive_sizer.rs#L322).
For each record, Headroom creates a 64-bit SimHash fingerprint and compares it
with earlier group representatives. In the worst case, every record is
different, so comparisons can grow roughly O(n²).

I first measured only that comparison loop in Python:

| Fingerprints | Comparison time |
| ---: | ---: |
| 1,000 | 24.8 ms |
| 2,000 | 57.4 ms |
| 4,000 | 194.7 ms |
| 8,000 | 786.7 ms |

I also made an indexed prototype that split each fingerprint into four 16-bit
parts. It reduced isolated grouping at 8,000 fingerprints from about 805 ms to
8 ms. It used Python, excluded fingerprint generation, and did not exercise
Headroom's real Rust path. This was exploratory evidence rather than a primary
submission result; [Step 1](benchmarks/results/adaptive_sizer/step-01-python-clustering.txt)
contains the details.

## Measuring the real bottleneck

I then benchmarked the real Rust function with repetitive, mixed, and diverse
inputs. The diverse case is closest to the comparison-loop worst case because
almost every record becomes a new group.

| Diverse records | Full `count_unique_simhash` time |
| ---: | ---: |
| 1,000 | 10.3 ms |
| 5,000 | 53.9 ms |

Five times more records made the full function about 5.25 times slower. That
was much closer to linear growth than the isolated comparison experiment had
suggested.

I measured the main components separately at 5,000 diverse records:

| Part | Time | Share |
| --- | ---: | ---: |
| Full function | 53.9 ms | 100% |
| SimHash fingerprint generation | 50.5 ms | 93.6% |
| Greedy comparison | 3.4 ms | 6.3% |

The comparison loop was an issue, but it was not the main cost in the
100–5,000-record range evaluated here. The expensive work was fingerprint
generation. The benchmark definitions are in
[`adaptive_sizer.rs`](crates/headroom-core/benches/adaptive_sizer.rs#L351), and
the component results are in
[Step 4](benchmarks/results/adaptive_sizer/step-04-component-split.txt).

## The change

SimHash gives each record a short 64-bit signature. It lowercases the text,
hashes overlapping four-character windows with MD5, and combines those hashes
into the final fingerprint.

For 5,000 diverse records, making windows and hashing them took 46.5 ms, or
92.1% of fingerprint-generation time. Before the change, every window required
a new Rust `String`:

```text
lowercase text → Vec<char> → new String for every window → MD5
```

For ASCII text, one character is one byte, so the new path can borrow the
four-byte window directly:

```text
ASCII lowercase once → borrow four-byte slices → MD5
```

The production change is in
[`simhash`](crates/headroom-core/src/transforms/adaptive_sizer.rs#L213). It does
not change MD5, window rules, Hamming distance, grouping behavior, or compression
policy. Non-ASCII strings keep the old character-based path because byte slices
are not safe for multi-byte characters.

## Correctness checks

I checked the new implementation against the old behavior for empty and short
strings, mixed-case ASCII, whitespace, punctuation, 448 deterministic random
ASCII strings, accented Latin, Turkish `İ`, Greek, CJK, emoji, and decomposed
Unicode. All fingerprints matched, and the focused adaptive-sizer suite passed
38 tests.

I kept the old implementation behind the default-off
[`legacy-simhash-benchmark`](crates/headroom-core/Cargo.toml#L191) feature. This
lets the same source tree build either the preserved old implementation or the
new fast path, making the before/after comparison reproducible.

I caught one regression during development: an early version made Unicode about
12% slower, so I kept the ASCII and Unicode loops separate.
The final Unicode control showed no clear regression. I make no Unicode speed
claim.

## Controlled performance results

At 5,000 records, the complete internal `count_unique_simhash` operation was:

| Input | Improvement |
| --- | ---: |
| Repetitive | 29.2% faster |
| Mixed | 28.5% faster |
| Diverse | 27.3% faster |

I then measured complete local Headroom routes, including content detection,
routing, adaptive sizing, request rebuilding, and token validation:

| Route | 200 records | 1,000 records | 5,000 records |
| --- | ---: | ---: | ---: |
| SmartCrusher JSON | 20.9% faster | 19.3% faster | 17.4% faster |
| LogCompressor | 27.1% faster | 28.2% faster | 26.3% faster |
| SearchCompressor | 25.2% faster | 26.9% faster | 26.6% faster |

For every ASCII comparison, old and new builds produced the same output hash,
byte length, compression strategy, and token count. The model therefore receives
the same compressed request; only local processing time changes.

The ASCII comparisons were statistically significant (`p < 0.05`). Unicode
controls showed no clear performance change, as expected from the unchanged
Unicode path. Commands, output hashes, and confidence intervals are in the
[benchmark evidence index](benchmarks/results/adaptive_sizer/README.md),
especially [Step 12](benchmarks/results/adaptive_sizer/step-12-log-search-route-benchmark.txt).

## When does greedy comparison matter again?

The original O(n²) concern remains valid at sufficiently large input sizes. I
created a separate worst-case benchmark in which every fingerprint becomes a new
representative. This forces exactly `n(n-1)/2` comparisons.

| Records | Full operation | Greedy comparison | Comparison share |
| ---: | ---: | ---: | ---: |
| 10,000 | 83.5 ms | 13.5 ms | 16.1% |
| 12,500 | 109.2 ms | 21.0 ms | 19.3% |
| 15,000 | 136.4 ms | 31.0 ms | 22.7% |
| 25,000 | 262.6 ms | 85.8 ms | 32.7% |

I called comparison material when it reached 20% of full-function time. The
crossover is between 12,500 and 15,000 distinct records. This does not change
the decision for the 100–5,000-record range evaluated here, but it gives a
clear future direction: indexed clustering becomes worth considering for very
large, high-diversity payloads. This is a worst-case stress test, not an
estimate of observed production traffic. See
[`adaptive_sizer_large.rs`](crates/headroom-core/benches/adaptive_sizer_large.rs)
and [Step 14](benchmarks/results/adaptive_sizer/step-14-high-cardinality-scaling.txt).

## Coding-Agent Evaluation: Latency Replay and Task-Success Control

I used GitHub Copilot CLI in two ways. First, I replayed captured JSON
diagnostics from three small Copilot repair tasks through the old and new
implementations. The replay isolated the SimHash-dependent path with
`SmartCrusher.without_compaction()`.

| Captured task output | Improvement |
| --- | ---: |
| Median repair | 23.6% faster |
| Currency formatting repair | 25.1% faster |
| Slug normalization repair | 25.2% faster |

The replay output was identical in both builds. This is real agent-produced tool
output, but it is still a Headroom-processing replay, not a complete
agent-task measurement. Details are in
[Step 10](benchmarks/results/adaptive_sizer/step-10-real-copilot-agent-replay.txt).

Second, I ran Copilot through Headroom directly on three small deterministic
[repair fixtures](benchmarks/agent_tasks/) that I created. Each had a broken
implementation, a 300-record JSON diagnostic, and a fixed test command.

| Version | Tasks passed |
| --- | ---: |
| Legacy SimHash | 3 / 3 |
| ASCII fast path | 3 / 3 |

Both builds ran the diagnostic, changed only the intended implementation file,
and passed the required tests. Copilot used automatic model selection and took
different tool paths in each run, so total wall time was not directly comparable.
Each version ran each task once, sequentially and without randomization; all six
runs passed. I treat this as task-success evidence only, not a quantitative
agent-performance result. The protocol and raw result are in
[Step 13](benchmarks/results/adaptive_sizer/step-13-live-copilot-agent-ab.txt).

Finally, I ran a fixed 10-instance subset of the published
[SWE-bench Lite](https://www.swebench.com/lite.html) corpus. The subset uses
real `pytest-dev/pytest` issues and base commits rather than self-authored
fixtures. Copilot received only each published problem statement through the
optimized Headroom proxy; after it finished, the harness applied SWE-bench's
published regression test patch and ran the instance's `FAIL_TO_PASS` tests.
All **10/10** tasks passed in the optimized-Headroom arm. Each run changed
exactly one production source file. I did not repeat these ten live tasks with
the legacy build; the legacy-versus-optimized comparison below replays the
captured tool outputs from those optimized-arm sessions.

The ten sessions produced 343 captured tool results totaling 756,087 bytes.
I replayed each task's exact tool-result array through isolated legacy and
optimized builds (10 timed calls after 2 warm-ups per payload). All 10 replays
selected `smart_sample` and had identical input hashes, output hashes, retained
record counts, and strategies. The sum of per-task median local processing time
fell from 1,990.759 ms to 1,978.915 ms: **0.6% lower** (individual payloads
ranged from effectively unchanged to 1.8% lower). This is a descriptive sum of
per-task medians, not one pooled benchmark measurement or a statistical
estimate of an overall effect.

This more realistic, mixed tool-output corpus produces a much smaller local
benefit than the deliberately SimHash-heavy three-payload fallback replay above.
That is the result I would use to characterize the coding-agent effect. It
measures local Headroom work, not task wall time or coding quality. The runner,
per-task target-test logs, hash-recorded payloads, and paired replay summaries
are in
[`swebench_lite_pytest_10/`](benchmarks/results/adaptive_sizer/swebench_lite_pytest_10/).

## Limitations

- All controlled benchmarks ran on one Apple Silicon machine.
- Most route inputs were deterministic synthetic fixtures.
- I did not measure peak memory or allocation traffic.
- I did not measure how often the SimHash-dependent path runs in production.
- The high-cardinality result is a worst-case stress test, not normal traffic.
- The direct Copilot check establishes task success, not faster full-agent work.
- The SWE-bench Lite study is a targeted native-test evaluation, not the
  benchmark's official Docker evaluator. It uses one non-randomized, Auto-model
  run per instance and does not establish an agent-wall-time effect.

## Reproduction and Evidence

The [evidence index](benchmarks/results/adaptive_sizer/README.md) records each
artifact's provenance and the commands used to reproduce the final comparisons.
Steps 1–5 are reconstructed exploratory snapshots, not byte-for-byte terminal
transcripts. The final route-level, coding-agent replay, and high-cardinality
claims rely on the later reproducible artifacts (Steps 8–10 and 12–14), plus
the checked-in SWE-bench Lite corpus artifacts.

## AI assistance

I used AI coding and writing assistants to explore the repository, discuss
benchmark ideas, and improve drafts. I reviewed the code, ran the commands,
checked the results, and kept the claims limited to what the measurements support.

## Conclusion

The resulting ASCII-only optimization removes unnecessary temporary allocations
while preserving fingerprints and model-visible compressed output. It reduced
local compression latency by 17–28% across the measured SmartCrusher,
LogCompressor, and SearchCompressor routes. The standardized SWE-bench Lite
extension adds 10/10 optimized-arm targeted-test successes and shows that the
benefit on mixed real-agent tool output is a modest 0.6% local-processing
reduction—not an agent wall-time claim.
