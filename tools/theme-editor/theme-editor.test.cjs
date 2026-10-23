'use strict';

// 主题编辑器前端逻辑的纯函数测试。
// token 清单与 Theme 结构的一致性由 `theme_editor_*` 那组 Rust 测试守；
// 这里只覆盖网页自己的取值、归一化与导出逻辑。
//
// 跑法：node --test tools/theme-editor/theme-editor.test.cjs

const test = require('node:test');
const assert = require('node:assert/strict');

const data = require('./tokens.js');
const editor = require('./theme-editor.js');

const bases = data.bases;
const dark = bases.find((base) => base.id === 'velora-dark');
const light = bases.find((base) => base.id === 'velora-light');
const paper = bases.find((base) => base.id === 'paper');

function token(kind, unit) {
  return { kind, unit: unit || '' };
}

test('tokens.js 带有可用数据', () => {
  assert.equal(data.schemaVersion, 1);
  assert.ok(Array.isArray(data.groups) && data.groups.length > 0);
  assert.ok(Array.isArray(data.tokens) && data.tokens.length > 100);
  assert.equal(bases.length, 6);
  for (const base of bases) {
    assert.ok(base.theme.colors && base.theme.dimensions);
    assert.ok(base.theme.typography && base.theme.placeholders);
  }
});

test('normalizeHexColor 接受 3/4/6/8 位并统一成 #RRGGBBAA', () => {
  assert.equal(editor.normalizeHexColor('#abc'), '#aabbccff');
  assert.equal(editor.normalizeHexColor('abcd'), '#aabbccdd');
  assert.equal(editor.normalizeHexColor('#AABBCC'), '#aabbccff');
  assert.equal(editor.normalizeHexColor('11223344'), '#11223344');
  assert.equal(editor.normalizeHexColor('  #FFF  '), '#ffffffff');
});

test('normalizeHexColor 对非法输入返回 fallback 或 null', () => {
  assert.equal(editor.normalizeHexColor('nope'), null);
  assert.equal(editor.normalizeHexColor('nope', '#12345678'), '#12345678');
  assert.equal(editor.normalizeHexColor(undefined, '#000000ff'), '#000000ff');
  assert.equal(editor.normalizeHexColor(42), null);
});

test('alphaOf 返回 0-255 的透明度', () => {
  assert.equal(editor.alphaOf('#00000000'), 0);
  assert.equal(editor.alphaOf('#ffffffff'), 255);
  assert.equal(editor.alphaOf('#abcdef80'), 0x80);
});

test('fontStack 只给自定义字体加引号，系统字体保持原样', () => {
  assert.match(editor.fontStack('.SystemUIFont'), /^-apple-system/);
  assert.equal(editor.fontStack('Georgia'), '"Georgia", -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif');
  assert.equal(editor.fontStack('Charter, Georgia, serif'), 'Charter, Georgia, serif');
  assert.match(editor.fontStack(''), /^-apple-system/);
});

test('fontWeightNumber 映射 FontWeightDef，未知值退回 400', () => {
  assert.equal(editor.fontWeightNumber('thin'), 100);
  assert.equal(editor.fontWeightNumber('semibold'), 600);
  assert.equal(editor.fontWeightNumber('black'), 900);
  assert.equal(editor.fontWeightNumber('whatever'), 400);
});

test('cssValueForToken 按 token 类型产出 CSS 值', () => {
  assert.equal(editor.cssValueForToken(token('color'), '#abc'), '#aabbccff');
  assert.equal(editor.cssValueForToken(token('weight'), 'bold'), '700');
  assert.equal(editor.cssValueForToken(token('number', 'px'), 12.5), '12.5px');
  assert.equal(editor.cssValueForToken(token('number', 'em'), 0.02), '0.02em');
  assert.equal(editor.cssValueForToken(token('number', ''), 1.5), '1.5');
  assert.equal(editor.cssValueForToken(token('string'), '空文档'), '空文档');
  assert.match(editor.cssValueForToken(token('font'), 'Georgia'), /^"Georgia"/);
  assert.equal(editor.cssValueForToken(token('number'), Number.NaN), '0');
});

test('roundToStep 按步长的小数位取整，clampNumber 把值夹在范围内', () => {
  assert.equal(editor.roundToStep(1.2345, 0.01), 1.23);
  assert.equal(editor.roundToStep(3.7, 1), 4);
  assert.equal(editor.roundToStep(1.2345, 0), 1.2345);
  assert.equal(editor.clampNumber(5, 0, 10), 5);
  assert.equal(editor.clampNumber(-5, 0, 10), 0);
  assert.equal(editor.clampNumber(15, 0, 10), 10);
  assert.equal(editor.clampNumber(15, null, null), 15);
});

test('tokenValueEquals 对颜色做归一化、对数字做容差、对字符串严格比较', () => {
  assert.ok(editor.tokenValueEquals(token('color'), '#fff', '#ffffff'));
  assert.ok(!editor.tokenValueEquals(token('color'), '#fff', '#000'));
  assert.ok(editor.tokenValueEquals(token('number'), '12', 12));
  assert.ok(!editor.tokenValueEquals(token('number'), 'NaN', 1));
  assert.ok(editor.tokenValueEquals(token('string'), 'a', 'a'));
  assert.ok(!editor.tokenValueEquals(token('string'), 'a', 'b'));
});

test('modifiedCount 只数相对基底真的改过的 token', () => {
  const tokens = [
    { path: 'colors.a', kind: 'color' },
    { path: 'dimensions.b', kind: 'number' },
    { path: 'placeholders.c', kind: 'string' },
  ];
  const base = { colors: { a: '#ffffff' }, dimensions: { b: 12 }, placeholders: { c: '空' } };
  const changed = { colors: { a: '#000000' }, dimensions: { b: 12 }, placeholders: { c: '空' } };
  assert.equal(editor.modifiedCount(base, base, tokens), 0);
  assert.equal(editor.modifiedCount(changed, base, tokens), 1);
});

test('deepMerge 不改动基底，filterPatchBySchema 丢掉未知字段', () => {
  const base = { colors: { a: '#fff', b: '#000' }, extra: { keep: 1 } };
  const merged = editor.deepMerge(base, { colors: { a: '#123456' }, unknown: true });
  assert.equal(base.colors.a, '#fff');
  assert.equal(merged.colors.a, '#123456');
  assert.equal(merged.colors.b, '#000');
  assert.equal(merged.unknown, true);

  const filtered = editor.filterPatchBySchema(
    { colors: { a: '#123456', nope: '#fff' }, unknown: true, name: 'x' },
    base,
  );
  assert.deepEqual(filtered, { colors: { a: '#123456' } });
});

test('normalizeImportedTheme 读取主题包的完整快照', () => {
  const source = editor.deepClone(paper.theme);
  source.colors.editor_background = '#010203ff';
  const imported = editor.normalizeImportedTheme(
    {
      name: 'Paper Tweak',
      creator: 'Ada',
      base_theme_id: 'paper',
      theme: source,
    },
    'velora-dark',
    bases,
  );
  assert.equal(imported.baseId, 'paper');
  assert.equal(imported.name, 'Paper Tweak');
  assert.equal(imported.creator, 'Ada');
  assert.equal(imported.values.colors.editor_background, '#010203ff');
  assert.equal(
    imported.values.typography.body_font_family,
    source.typography.body_font_family,
  );
});

test('normalizeImportedTheme 接受裸 Theme JSON 并从基底补齐缺失 section', () => {
  const imported = editor.normalizeImportedTheme(
    { colors: { editor_background: '#010203ff' } },
    'velora-dark',
    bases,
  );
  assert.equal(imported.baseId, 'velora-dark');
  assert.equal(imported.values.colors.editor_background, '#010203ff');
  assert.equal(imported.values.dimensions.writing_max_width, dark.theme.dimensions.writing_max_width);
  assert.equal(imported.values.placeholders.empty_editing, dark.theme.placeholders.empty_editing);
});

test('normalizeImportedTheme 对未知基底 id 退回调用方给的基底', () => {
  const imported = editor.normalizeImportedTheme(
    { base_theme_id: 'not-a-theme', theme: { colors: { editor_background: '#010203ff' } } },
    'velora-light',
    bases,
  );
  assert.equal(imported.baseId, 'velora-light');
  assert.equal(imported.values.dimensions.writing_max_width, light.theme.dimensions.writing_max_width);
});

test('normalizeImportedTheme 拒绝不是主题的 JSON', () => {
  assert.throws(() => editor.normalizeImportedTheme(null, 'velora-dark', bases), /not-an-object/);
  assert.throws(() => editor.normalizeImportedTheme([], 'velora-dark', bases), /not-an-object/);
  assert.throws(() => editor.normalizeImportedTheme({ hello: 1 }, 'velora-dark', bases), /not-a-theme/);
});

test('buildExportPack 导出完整快照而不是 diff', () => {
  const values = editor.deepClone(dark.theme);
  values.colors.editor_background = '#010203ff';
  const pack = editor.buildExportPack(values, '  Night Owl  ', '  Ada  ', 'velora-dark');

  assert.deepEqual(Object.keys(pack), ['name', 'creator', 'base_theme_id', 'theme']);
  assert.equal(pack.name, 'Night Owl');
  assert.equal(pack.creator, 'Ada');
  assert.equal(pack.base_theme_id, 'velora-dark');
  assert.equal(pack.theme.name, 'Night Owl');
  assert.equal(pack.theme.colors.editor_background, '#010203ff');
  assert.ok(pack.theme.colors && pack.theme.dimensions);
  assert.ok(pack.theme.typography && pack.theme.placeholders);
  assert.equal(pack.theme.colors.text_default, dark.theme.colors.text_default);
});

test('buildExportPack 要求名称、作者与四个 section', () => {
  const values = editor.deepClone(dark.theme);
  assert.throws(() => editor.buildExportPack(values, '  ', 'Ada', 'velora-dark'), /name-required/);
  assert.throws(() => editor.buildExportPack(values, 'Name', '', 'velora-dark'), /creator-required/);
  assert.throws(
    () => editor.buildExportPack({ colors: {} }, 'Name', 'Ada', 'velora-dark'),
    /incomplete-theme/,
  );
});

test('导出文件名去掉路径分隔符与控制字符', () => {
  assert.equal(editor.sanitizeFilenamePart('  '), 'theme');
  assert.equal(editor.sanitizeFilenamePart('My Theme'), 'My_Theme');
  assert.equal(editor.sanitizeFilenamePart('a/b\\c:d*e?f"g<h>i|j'), 'a_b_c_d_e_f_g_h_i_j');
  assert.equal(editor.sanitizeFilenamePart('x\u0000\u001fy'), 'x__y');
  assert.equal(editor.buildFilename('Night Owl', 'Ada Lovelace'), 'Night_Owl_Ada_Lovelace.json');
});

test('defaultBaseId 跟随系统深浅色', () => {
  assert.equal(editor.defaultBaseId(true), 'velora-dark');
  assert.equal(editor.defaultBaseId(false), 'velora-light');
});

test('getPath / setPath 支持 section.key 形式的 token 路径', () => {
  const values = {};
  editor.setPath(values, 'colors.editor_background', '#fff');
  editor.setPath(values, 'dimensions.writing_max_width', 720);
  assert.equal(editor.getPath(values, 'colors.editor_background'), '#fff');
  assert.equal(editor.getPath(values, 'dimensions.writing_max_width'), 720);
  assert.equal(editor.getPath(values, 'colors.missing'), undefined);
});

test('每个 token 的默认值都能被 cssValueForToken 转成字符串', () => {
  for (const item of data.tokens) {
    const [section, key] = item.path.split('.');
    const value = dark.theme[section][key];
    assert.notEqual(value, undefined, `${item.path} 在基底里没有值`);
    const css = editor.cssValueForToken(item, value);
    assert.equal(typeof css, 'string', `${item.path} 转出的 CSS 值不是字符串`);
    // empty_editing 默认是空串（应用在别处补默认文案），其余 token 不该转空。
    if (item.section !== 'placeholders' && value !== '') {
      assert.ok(css.length > 0, `${item.path} 转出了空 CSS 值`);
    }
  }
});
