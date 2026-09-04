# SWE-bench Lite Copilot Corpus

This directory contains a fixed 10-instance `pytest-dev/pytest` subset of
SWE-bench Lite. GitHub Copilot CLI received each published problem statement
through the optimized Headroom proxy. The harness then applied the dataset's
test patch and ran each instance's published `FAIL_TO_PASS` tests.

Results: 10/10 targeted tests passed in the optimized-Headroom arm. The
sessions produced 343 tool results totaling 756,087 bytes. `summary.json`
records immutable task metadata hashes, base commits, agent outcomes, changed
files, test outcomes, and payload hashes. The live tasks were not repeated with
the legacy build.

`optimized-replay.json` and `legacy-replay.json` replay the captured
`tool-results.json` arrays with 2 warm-ups and 10 timed calls per task. All ten
outputs have identical hashes, strategies, and retained-record counts. The sum
of per-task medians is 1,978.915 ms optimized versus 1,990.759 ms legacy
(0.6% lower local processing time). This is a descriptive sum of per-task
medians, not a pooled benchmark measurement or statistical overall estimate.

This is a targeted native-test study, not SWE-bench's official Docker
evaluation, and it makes no agent-wall-time or coding-quality claim. Run it
again with [`run_swebench_lite_copilot.py`](../../../run_swebench_lite_copilot.py).
