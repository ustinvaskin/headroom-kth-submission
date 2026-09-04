# Experiment Log

This is the shorter version of my working notes. The detailed commands and raw
numbers are still under `benchmarks/results/adaptive_sizer/`.

## Evidence map

- The main change and tests live in
  [`adaptive_sizer.rs`](crates/headroom-core/src/transforms/adaptive_sizer.rs).
- The benchmark definitions live in
  [`adaptive_sizer.rs` benchmark](crates/headroom-core/benches/adaptive_sizer.rs).
- The Rust control feature is in
  [`headroom-core/Cargo.toml`](crates/headroom-core/Cargo.toml).
- The Python proxy uses
  [`headroom-py/Cargo.toml`](crates/headroom-py/Cargo.toml) to choose the old
  or new SimHash version during the live Copilot test.
- The original Python behavior is in
  [`adaptive_sizer.py`](headroom/transforms/adaptive_sizer.py).
- The [benchmark evidence index](benchmarks/results/adaptive_sizer/README.md)
  points to the detailed result files.
- [Step 9](benchmarks/results/adaptive_sizer/step-09-reproducible-internal-baseline.txt)
  has the final internal old/new comparison.
- [Step 10](benchmarks/results/adaptive_sizer/step-10-real-copilot-agent-replay.txt)
  has the captured-output replay.
- [Step 12](benchmarks/results/adaptive_sizer/step-12-log-search-route-benchmark.txt)
  has the final LogCompressor/SearchCompressor results.
- [Step 13](benchmarks/results/adaptive_sizer/step-13-live-copilot-agent-ab.txt)
  has the direct Copilot task A/B; its source tasks are in
  [agent_tasks](benchmarks/agent_tasks/).
- [Step 14](benchmarks/results/adaptive_sizer/step-14-high-cardinality-scaling.txt)
  has the high-cardinality stress test and exact comparison counts.

## Intro

`compute_optimal_k` uses `count_unique_simhash` before choosing how many items
to keep. It makes one 64-bit SimHash per item, then compares each fingerprint
with cluster representatives until it finds one within Hamming distance 3.

That code is used by `SmartCrusher`, `LogCompressor`, and `SearchCompressor`.

## Step 1: I thought the comparison loop was the problem

In the worst case, each new fingerprint is compared with every earlier one.
That can grow like O(n²), so I measured only that comparison loop in Python.
Doubling the number of random fingerprints made the runtime roughly quadruple.

The Python experiment and indexed prototype are recorded in
[Step 1 evidence](benchmarks/results/adaptive_sizer/step-01-python-clustering.txt).

I also tried a chunk-indexed version that was much faster in isolation. But it
did not include fingerprint generation or the real Rust code, so I did not
treat it as production evidence.

## Step 2: I checked the Rust baseline

```bash
cargo test -p headroom-core --lib adaptive_sizer --no-default-features
```

35 tests passed before any changes. The saved baseline record is in
[Step 2 evidence](benchmarks/results/adaptive_sizer/step-02-rust-baseline.txt).

## Step 3: I benchmarked the real Rust function

I measured the real production function, `count_unique_simhash`, before
changing it. The benchmark is defined in
[`bench_count_unique_simhash`](crates/headroom-core/benches/adaptive_sizer.rs#L351).

I used three kinds of input:

```text
repetitive → many repeated records, very few groups
mixed      → some repeated records and some different records
diverse    → almost every record becomes a new group
```

The diverse case is closest to the worst case for the greedy comparison loop.

For diverse input:

| Records | Time |
| ---: | ---: |
| 1,000 | 10.3 ms |
| 5,000 | 53.9 ms |

Going from 1,000 to 5,000 records is 5x more input, but the full function was
only about 5.25x slower. That was much closer to linear growth than the
comparison-only Python test suggested. It suggested that fingerprint generation
and other per-record work were still the main costs.

I did not change clustering yet. The next step was to measure fingerprint
generation and greedy grouping separately.

The full baseline table, input-group check, warnings, and confidence intervals
are in [Step 3 evidence](benchmarks/results/adaptive_sizer/step-03-greedy-baseline.txt).

## Step 4: I checked which part was actually slow

The earlier test showed that greedy comparison can grow quickly. But I needed to
know whether it was actually the slow part of the full Rust function.

Before measuring, I set a simple rule:

- keep investigating indexed clustering if grouping was more than 20% of total
  time;
- otherwise, optimize SimHash if it was more than 80% of total time.

I measured the full function, fingerprint generation alone, and grouping alone.

For 5,000 diverse records:

| Part | Time | Share |
| --- | ---: | ---: |
| Full function | 53.9 ms | 100% |
| Generate SimHash fingerprints | 50.5 ms | 93.6% |
| Greedy grouping | 3.4 ms | 6.3% |

So the comparison loop was real, but it was not the main cost at normal tested
sizes. Even making grouping free would only save a small part of the total time.

I stopped pursuing the indexed-comparison idea and investigated fingerprint
generation instead.

The full component measurements are in
[Step 4 evidence](benchmarks/results/adaptive_sizer/step-04-component-split.txt).

## Step 5: I found the expensive part of SimHash

I split SimHash into three parts:

1. lowercase and prepare the text;
2. make each overlapping four-character window and hash it with MD5;
3. combine the hashes into the final 64-bit fingerprint.

Before measuring, I decided to optimize a part only if it used at least 20% of
the total SimHash time.

For 5,000 diverse records:

| Part | Time | Share |
| --- | ---: | ---: |
| Prepare text | 0.8 ms | 1.5% |
| Make windows + MD5 | 46.5 ms | 92.1% |
| Build final fingerprint | 3.7 ms | 7.3% |

The window-building and MD5 step was clearly the expensive part. The old code
created a new small Rust `String` for every four-character window before
hashing it.

That is the part I decided to optimize next. No production code changed in
this step.

The full measurements and confidence intervals are in
[Step 5 evidence](benchmarks/results/adaptive_sizer/step-05-simhash-stages.txt).

## Step 6: I added an ASCII fast path

For ASCII text, one character is one byte. That means the code can use a
four-byte slice as a four-character window.

The old path did this:

```text
lowercase → Vec<char> → new String for every window → MD5
```

The new ASCII path does this:

```text
ASCII lowercase once → borrow four-byte slices → MD5
```

This removes the repeated temporary `String` allocations found in Step 5. The
production change is in
[`simhash`](crates/headroom-core/src/transforms/adaptive_sizer.rs#L213).

The fingerprint algorithm itself did not change: it still uses the same
windows, MD5 hashes, vote counters, and Hamming-distance grouping. Non-ASCII
text keeps the old character-based path because byte slices would not be safe
for multi-byte characters.

I checked empty and short strings, mixed case, 448 random ASCII strings, and
representative Unicode cases. Fingerprints matched the reference and 38 focused
tests passed.

The first attempt accidentally slowed Unicode by about 12%. My limit was 5%, so
I kept the Unicode loop separate, reran the control, and made no Unicode speed
claim.

At 5,000 records, the complete `count_unique_simhash` operation was about
27-31% faster in this initial comparison. The full validation and initial
results are in [Step 6 evidence](benchmarks/results/adaptive_sizer/step-06-ascii-fast-path.txt).

Step 9 reruns the comparison from the final tree with the reproducible legacy
feature. Use those later 27-29% values as the final internal result:
[Step 9 evidence](benchmarks/results/adaptive_sizer/step-09-reproducible-internal-baseline.txt).

## Step 7 and 8: I tested the real SmartCrusher route

I built a realistic Anthropic request containing a JSON `tool_result` and sent
it through Headroom's full local route:

```text
request → content detection → SmartCrusher → adaptive sizing
→ request rebuilding → token validation
```

This measures Headroom's local work, not LLM inference or a whole agent task.
Old and new builds produced identical request hashes, byte sizes, selected
strategy, and token counts. That means the model receives the same compressed
request; only the local processing time changed.

The first before/after comparison used a temporary manual toggle. To make it
reproducible, I added the default-off `legacy-simhash-benchmark` feature. It
lets the same source tree build either the preserved old SimHash code or the
new fast path without manual edits.

Final reproducible SmartCrusher results:

| Records | Legacy | Fast path | Change |
| ---: | ---: | ---: | ---: |
| 200 | 1.42 ms | 1.12 ms | 21% faster |
| 1,000 | 7.03 ms | 5.68 ms | 19% faster |
| 5,000 | 36.8 ms | 30.4 ms | 17% faster |

The route benchmark and its initial behavior proof are in
[Step 7 evidence](benchmarks/results/adaptive_sizer/step-07-coding-agent-live-zone.txt).
The final reproducible legacy/candidate result is in
[Step 8 evidence](benchmarks/results/adaptive_sizer/step-08-reproducible-baseline.txt).

## Step 9: I repeated the internal benchmark with the same control

This is the internal SimHash operation by itself, not the full request route.

At 5,000 records, `count_unique_simhash` improved by about 29% for repetitive
and mixed inputs and 27% for diverse inputs.

The final internal table and confidence intervals are in
[Step 9 evidence](benchmarks/results/adaptive_sizer/step-09-reproducible-internal-baseline.txt).

## Step 10: I replayed real Copilot tool output

I ran Copilot on three small repair tasks, captured the JSON diagnostics it
produced, and replayed those outputs through legacy and optimized builds.

The replay uses `SmartCrusher.without_compaction()` to isolate the
SimHash-dependent adaptive path. The local replay was 24-25% faster with
identical compressed output.

This is real agent-produced tool output, but it is still a replay. It does not
measure a full Copilot task, and it does not show how often this adaptive path
runs in production traffic.

The replay script is
[`copilot_agent_replay.py`](benchmarks/copilot_agent_replay.py), and the saved
inputs, hashes, and timing results are in
[Step 10 evidence](benchmarks/results/adaptive_sizer/step-10-real-copilot-agent-replay.txt).

## Step 12: I tested the other two callers

SmartCrusher was not the only caller of adaptive sizing. LogCompressor and
SearchCompressor use the same SimHash diversity estimate, but on very different
text shapes.

I added full live-zone benchmarks for both routes. Each benchmark sends a
deterministic synthetic tool result through Headroom's detection, routing,
compression, and token-validation path.

- LogCompressor: about 26-28% faster on the ASCII fixtures.
- SearchCompressor: about 25-27% faster on the ASCII fixtures.

The Unicode controls had no clear performance change, which is expected because
they use the unchanged path. The old and new outputs matched exactly, so these
results show that the shared ASCII fast path helps all three production callers.

The complete route results, output hashes, and confidence intervals are in
[Step 12 evidence](benchmarks/results/adaptive_sizer/step-12-log-search-route-benchmark.txt).

## Step 13: I ran Copilot through Headroom directly

I used Copilot's native Auto route through Headroom on fresh copies of three
small deterministic
[repair fixtures](benchmarks/agent_tasks/) I created for this Copilot A/B
check. Each had a broken implementation, a large JSON diagnostic, and a fixed
test command.

I built two isolated Headroom environments: one with the old SimHash code and
one with the fast path. Every run executed a 300-record JSON diagnostic, changed
only `implementation.py`, and passed the required test.

So task success was the same: 3/3 for legacy and 3/3 for the fast path.

Total task time was not useful as a speed measurement. Copilot used automatic
model selection and took different tool paths in each run, so the wall times are
not directly comparable. I use this as task-success evidence only; I do **not**
claim the whole agent became faster.

The full direct-agent protocol and results are in
[Step 13 evidence](benchmarks/results/adaptive_sizer/step-13-live-copilot-agent-ab.txt).

## Step 14: I finally tested the larger O(n²) case

I made a separate high-cardinality worst-case benchmark. Every fingerprint
becomes a new cluster, so the greedy loop performs exactly every possible
comparison: `n(n-1)/2`.

I used 20% of total time as the point where I would call clustering material.

At 12,500 records, comparing was about 19% of the whole operation. At 15,000,
it was about 23%. So the comparison loop becomes material somewhere between
those sizes. At 25,000 records it was about 33%.

This does not change my conclusion for normal 100-5,000-record workloads. The
ASCII fast path was the right fix there. But it gives a concrete future point
where indexed clustering becomes worth considering if Headroom needs to handle
unusually large, high-diversity payloads.

The benchmark source is
[`adaptive_sizer_large.rs`](crates/headroom-core/benches/adaptive_sizer_large.rs),
and the full timing table is in
[Step 14 evidence](benchmarks/results/adaptive_sizer/step-14-high-cardinality-scaling.txt).

## Still open / maybe not worth doing now

- Measure peak memory or allocation traffic.
- Try a broader captured log and search-result corpus.
- Decide whether unusually large, high-diversity payloads justify an indexed
  clustering implementation in the future.

The important result is still local: the same compressed request is produced
faster for ASCII-heavy tool output, and the direct agent checks did not show a
task-success regression.

The submission analysis is [ANALYSIS.md](ANALYSIS.md).
