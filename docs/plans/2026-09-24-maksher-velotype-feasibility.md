# maksher Velotype Feasibility Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Determine with reproducible evidence whether Velotype's native editor is a viable source for maksher's selective GPUI editor extraction.

**Architecture:** Keep the current `maksher` main branch as the design record. Execute this plan in a dedicated worktree; build the pinned Velotype commit in a separate temporary clone so `../velotype` remains untouched. Generate fixed documents, measure the actual release application, test macOS Chinese IME, then decide whether to write the editor-extraction plan. No Tauri code enters this phase.

**Tech Stack:** Rust 2024, GPUI 0.2, Cargo, Node.js 24 for fixture generation and measurement reporting, macOS.

---

## Scope and source

- Read [approved design](2026-09-24-maksher-gpui-editor-design.md) before starting. The user chose a new maksher GPUI application structure and one-time extraction of selected Velotype editor code, with manual upstream patching only.
- Pin local Velotype source to `ed65977be94f2f2703037fcb8b6cbab2e7579571`. Upstream: `https://github.com/manyougz/velotype`.
- This plan is only the **first feasibility gate**. Do not copy Velotype into `maksher` or build workspace features yet. The later extraction plan depends on baseline results and the actual editor-host boundary.
- The earlier untracked Tauri scaffold has been removed at the user's request. Work in `/private/tmp/maksher-gpui-feasibility`; do not recreate that scaffold.
- Each code task below is a separate commit. If a command fails, record the original output and fix the cause; do not weaken checks or silently relax performance thresholds.

### Task 1: Isolate both checkouts

**Files:** No tracked files.

**Step 1: Create the maksher worktree**

Run from the `maksher` root:

```bash
git worktree add -b feat/gpui-feasibility /private/tmp/maksher-gpui-feasibility main
```

Expected: the worktree contains the approved design and no Tauri files. If the path or branch already exists, inspect it before choosing a fresh name; do not delete it blindly.

**Step 2: Make an independent Velotype checkout**

```bash
git clone --local ../velotype /private/tmp/maksher-velotype-baseline
git -C /private/tmp/maksher-velotype-baseline checkout ed65977be94f2f2703037fcb8b6cbab2e7579571
git -C /private/tmp/maksher-velotype-baseline status --short
```

Expected: checkout is clean and `HEAD` matches the pinned commit. Do not modify the user's `../velotype` checkout.

### Task 2: Generate repeatable Markdown test documents

**Files:**
- Create: `scripts/generate-fixtures.mjs`
- Create: `scripts/generate-fixtures.test.mjs`
- Create: `.gitignore`

**Step 1: Write the failing public-behavior test**

Use `node:test` to run the generator in a temporary directory. Put this in `scripts/generate-fixtures.test.mjs`; it should fail first because the generator is absent:

```js
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('creates fixed, valid Markdown documents', () => {
  const dir = mkdtempSync(join(tmpdir(), 'maksher-fixtures-'));
  try {
    execFileSync(process.execPath, ['scripts/generate-fixtures.mjs', dir]);
    for (const [name, size] of [['one-mib.md', 1_048_576], ['ten-mib.md', 10_485_760]]) {
      const bytes = readFileSync(join(dir, name));
      assert.equal(bytes.length, size);
      const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
      for (const marker of ['# 长篇写作性能样本', '  - 嵌套项', '| --- | --- |', '- [ ]', '```rust', '中文']) {
        assert.ok(text.includes(marker), `${name} lacks ${marker}`);
      }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
```

**Step 2: Run the test and confirm failure**

```bash
node --test scripts/generate-fixtures.test.mjs
```

Expected: FAIL due to missing generator, not because the assertion was skipped.

**Step 3: Implement the smallest generator**

Write `scripts/generate-fixtures.mjs` with this deterministic core; use the first argument as output directory and create it if absent:

```js
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const out = process.argv[2] ?? 'tests/fixtures/perf';
mkdirSync(out, { recursive: true });
const unit = [
  '# 长篇写作性能样本',
  '一段中文与 English text，用于段落和输入测试。',
  '- 第一项',
  '  - 嵌套项',
  '- [ ] 待办项',
  '| 名称 | 值 |',
  '| --- | --- |',
  '| a | b |',
  '```rust',
  'fn main() {}',
  '```',
  '',
].join('\n');

for (const [name, bytes] of [['one-mib.md', 1_048_576], ['ten-mib.md', 10_485_760]]) {
  const unitBytes = Buffer.byteLength(unit);
  const repeated = unit.repeat(Math.floor((bytes - 1) / unitBytes));
  const padding = bytes - Buffer.byteLength(repeated);
  writeFileSync(join(out, name), repeated + ' '.repeat(padding - 1) + '\n');
}
```

Add `tests/fixtures/perf/*.md` to `.gitignore`; commit only generator and test, not generated files.

**Step 4: Verify and commit**

```bash
node --test scripts/generate-fixtures.test.mjs
git add scripts/generate-fixtures.mjs scripts/generate-fixtures.test.mjs .gitignore
git commit -m "test(perf): add deterministic Markdown fixtures"
```

Expected: test PASS and a clean commit. Compare byte lengths with `Buffer.byteLength`, not JavaScript string length.

### Task 3: Build and smoke-test unmodified Velotype

**Files:**
- Create: `docs/benchmarks/2026-09-24-velotype-baseline.md`

**Step 1: Generate samples and build the pinned source**

Run `node scripts/generate-fixtures.mjs /private/tmp/maksher-velotype-fixtures` from the maksher worktree. In `/private/tmp/maksher-velotype-baseline`, run:

```bash
cargo test --locked
cargo build --release --locked
```

Expected: all existing tests pass and `target/release/velotype` exists. Build may be slow; record wall time and toolchain version. If a platform dependency is missing, report it rather than changing Velotype source.

**Step 2: Open each sample in the release app**

Run `target/release/velotype /private/tmp/maksher-velotype-fixtures/one-mib.md`, then the 10 MiB sample, closing the app between runs. Check that both reach rendered editing and accept input; record any source-mode fallback, hang, crash or visual corruption. Avoid saving changes to the generated samples.

**Step 3: Record evidence and commit**

In `docs/benchmarks/2026-09-24-velotype-baseline.md`, record pinned commit, Mac model/RAM/macOS, Rust version, exact commands and raw pass/fail results. Commit the report. Do not claim speed targets passed from this smoke test.

### Task 4: Define and implement an end-to-end timing probe

**Files:**
- Create: `scripts/summarize-bench.mjs`
- Create: `scripts/summarize-bench.test.mjs`
- Create: `docs/benchmarks/velotype-probe.patch`
- Modify: `docs/benchmarks/2026-09-24-velotype-baseline.md`
- Temporary-only modifications: `/private/tmp/maksher-velotype-baseline/src/main.rs`, `src/editor/render.rs`, `src/components/block/input.rs`

**Step 1: Write the failing summary test**

The public script reads newline-delimited records `{ "kind": "launch_ms" | "open_ms" | "input_ms", "value": number }` from stdin. Put this in `scripts/summarize-bench.test.mjs`; it should fail first because the script is absent:

```js
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';

test('reports median and nearest-rank p95', () => {
  const input = [1, 2, 3, 4, 5]
    .map(value => JSON.stringify({ kind: 'input_ms', value })).join('\n') + '\n';
  const result = spawnSync(process.execPath, ['scripts/summarize-bench.mjs'], {
    input, encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  const summary = JSON.parse(result.stdout);
  assert.deepEqual(summary.input_ms, {
    samples: [1, 2, 3, 4, 5], count: 5, median: 3, p95: 5,
  });
});
```

Reject records with missing or nonfinite values. Run `node --test scripts/summarize-bench.test.mjs` and confirm FAIL before implementing the script.

**Step 2: Implement and verify the summary**

Sort a copy of each series numerically; median uses the middle value or average of two middle values, p95 uses the nearest-rank index `Math.ceil(0.95 * n) - 1`. Print JSON keyed by metric with raw samples, median, p95 and count. Re-run the test; expected PASS. Commit the script and test.

**Step 3: Instrument only the temporary Velotype clone**

Add monotonic timestamps at process start (`src/main.rs`), immediately before reading the target file, at the first `Editor::render` pass where the editor has an active focused block (`src/editor/render.rs`), and from committed input in `Block::replace_text_in_range` to the next painted editor frame (`src/components/block/input.rs`). Emit machine-readable `kind/value` records to stderr. Exclude uncommitted IME composition updates from ordinary typing latency; test those separately. The source patch must live only in the temporary checkout and be saved to `docs/benchmarks/velotype-probe.patch` for reproducibility.

**Step 4: Check the probe against a control**

Rebuild the temporary release app. Run a tiny file and check every emitted value is finite and nonnegative; check that a manual keypress produces one input result. If first-render timing fires before the app accepts text, move the ready marker to the first focused frame and document why. Do not label a first-render timestamp as “editable” unless this control passes.

### Task 5: Measure performance and Chinese IME on macOS

**Files:**
- Modify: `docs/benchmarks/2026-09-24-velotype-baseline.md`
- Create: `docs/benchmarks/2026-09-24-velotype-ime.md`

**Step 1: Collect timed runs**

With the release build and fixed fixtures, run 10 separate process launches for the blank editor and each file size. Record each raw startup-to-ready and file-read-to-ready sample, not just averages. Enter at least 200 ordinary input events in the 1 MiB document across beginning, middle and end; summarize p95 using `scripts/summarize-bench.mjs`. Record whether the rendered mode stayed active for 10 MiB.

**Step 2: Test real macOS Chinese Pinyin**

In rendered and source modes, enter candidate words, replace an active composition, edit near inline formatting and a block boundary, undo, save and reopen. Record exact sample text, expected text, actual text and any misplaced candidate window or lost/duplicated character. Automated UTF-16 tests in Velotype are supporting evidence, not a substitute for real IME operation.

**Step 3: Compare with approved thresholds**

Current Mac mini M4/16 GB targets: cold app launch ≤1 s; open 1 MiB ≤1 s; open 10 MiB ≤5 s; input p95 ≤50 ms. Report each target as pass/fail/untestable with raw data. If any fail, record the measured gap and likely hot path; do not change thresholds. Commit the two reports and the probe patch.

### Task 6: Record the gate decision and prepare the next plan

**Files:**
- Create: `docs/plans/2026-09-24-maksher-editor-extraction-decision.md`

**Step 1: Make the decision from evidence**

Record one of: (a) baseline meets targets and IME is stable, proceed to a separate exact extraction plan; (b) failures appear bounded and fixable, plan focused repairs before extraction; (c) failures are severe or measurement is inconclusive, return to architecture discussion. Include the compiler/test results, benchmark report links and the specific files likely to move across the editor-host boundary.

**Step 2: Verify and commit**

Re-read the decision and raw reports; check they make no claim unsupported by a run. Run `git diff --check`, then commit the decision. Merge the feasibility branch only after review. A future implementation plan must cover the new GPUI shell, selective extraction, workspace/tabs, safe persistence and packaging; it must not revive the paused Tauri scaffold.
