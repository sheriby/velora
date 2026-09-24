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
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
