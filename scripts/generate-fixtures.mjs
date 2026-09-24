import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const outputDir = process.argv[2] ?? 'tests/fixtures/perf';
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

mkdirSync(outputDir, { recursive: true });

for (const [name, size] of [['one-mib.md', 1_048_576], ['ten-mib.md', 10_485_760]]) {
  const repeats = Math.floor((size - 1) / Buffer.byteLength(unit));
  const padding = size - 1 - repeats * Buffer.byteLength(unit);
  writeFileSync(join(outputDir, name), unit.repeat(repeats) + ' '.repeat(padding) + '\n');
}
