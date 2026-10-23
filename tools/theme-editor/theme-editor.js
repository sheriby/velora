(function (global) {
  'use strict';

  const UI = {
    zh: {
      title: 'Velora 主题编辑器',
      subtitle: '在浏览器里实时调整主题变量，导出后导入 Velora',
      baseTheme: '基底主题',
      themeName: '主题名称',
      creator: '作者',
      importTheme: '导入主题',
      exportTheme: '导出主题',
      searchPlaceholder: '搜索 token、说明或分组',
      expandAll: '展开全部',
      collapseAll: '折叠全部',
      resetAll: '全部重置',
      documentTab: '文档',
      interfaceTab: '界面',
      previewHint: '预览是模拟渲染；少数视口相关尺寸主要影响真实应用',
      resetToken: '重置这一项',
      resetGroup: '重置本组',
      tokenCount: '{visible} / {total} 个 token',
      modifiedCount: '{count} 项改动',
      noResults: '没有匹配的 token',
      confirmDiscard: '当前有未保存的改动，确定要丢弃吗？',
      baseChanged: '已切换到基底主题“{name}”',
      importSuccess: '已载入“{name}”（基于 {base}）',
      importError: '导入失败：{detail}',
      exportSuccess: '已导出 {file}。打开 Velora → 主题 → 添加主题配置… 选择该文件即可。',
      nameRequired: '请先填写主题名称',
      creatorRequired: '请先填写作者',
      exportError: '导出失败：{detail}',
      dataMissing: '没有找到 tokens.js，或者文件已损坏。',
      dataMissingHint: '请在仓库根目录运行下面的命令重新生成，然后刷新页面：',
      regenerateCommand: 'VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture',
      languageToggleTitle: 'Switch to English',
    },
    en: {
      title: 'Velora Theme Editor',
      subtitle: 'Tune theme tokens with a live preview, then import the exported file into Velora',
      baseTheme: 'Base theme',
      themeName: 'Theme name',
      creator: 'Creator',
      importTheme: 'Import theme',
      exportTheme: 'Export theme',
      searchPlaceholder: 'Search tokens, descriptions, or groups',
      expandAll: 'Expand all',
      collapseAll: 'Collapse all',
      resetAll: 'Reset all',
      documentTab: 'Document',
      interfaceTab: 'Interface',
      previewHint: 'The preview is a simulation; a few viewport-related sizes only apply in the real app',
      resetToken: 'Reset this token',
      resetGroup: 'Reset group',
      tokenCount: '{visible} / {total} tokens',
      modifiedCount: '{count} changed',
      noResults: 'No matching tokens',
      confirmDiscard: 'There are unsaved changes. Discard them?',
      baseChanged: 'Switched base theme to “{name}”',
      importSuccess: 'Loaded “{name}” (based on {base})',
      importError: 'Import failed: {detail}',
      exportSuccess: 'Exported {file}. Open Velora → Theme → Add Theme Config… and choose that file.',
      nameRequired: 'Please fill in the theme name',
      creatorRequired: 'Please fill in the creator',
      exportError: 'Export failed: {detail}',
      dataMissing: 'tokens.js is missing or damaged.',
      dataMissingHint: 'Run the command below from the repository root, then reload this page:',
      regenerateCommand: 'VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture',
      languageToggleTitle: '切换到中文',
    },
  };

  const WEIGHTS = ['thin', 'light', 'normal', 'medium', 'semibold', 'bold', 'extrabold', 'black'];
  const WEIGHT_NUMBERS = {
    thin: 100,
    light: 300,
    normal: 400,
    medium: 500,
    semibold: 600,
    bold: 700,
    extrabold: 800,
    black: 900,
  };
  const FONT_SUGGESTIONS = [
    '.SystemUIFont',
    'Songti SC',
    'PingFang SC',
    'Microsoft YaHei',
    'Helvetica Neue',
    'Georgia',
    'Charter',
    'Iowan Old Style',
    'SF Pro Text',
    'Menlo',
  ];

  const DATA = global.VELORA_THEME_EDITOR_DATA;
  const tokenByPath = DATA && Array.isArray(DATA.tokens)
    ? new Map(DATA.tokens.map((token) => [token.path, token]))
    : new Map();

  const state = {
    lang: 'zh',
    baseId: 'velora-dark',
    baseValues: {},
    values: {},
    name: 'My Theme',
    creator: 'Anonymous',
    search: '',
    collapsed: new Set(),
    noticeTimer: null,
  };

  // ---------- pure helpers ----------

  function deepClone(value) {
    if (value === undefined) return undefined;
    if (typeof structuredClone === 'function') {
      return structuredClone(value);
    }
    return JSON.parse(JSON.stringify(value));
  }

  function getPath(object, path) {
    return path.split('.').reduce((current, key) => (current == null ? undefined : current[key]), object);
  }

  function setPath(object, path, value) {
    const parts = path.split('.');
    let current = object;
    for (let index = 0; index < parts.length - 1; index += 1) {
      const key = parts[index];
      if (current[key] == null || typeof current[key] !== 'object') {
        current[key] = {};
      }
      current = current[key];
    }
    current[parts[parts.length - 1]] = value;
  }

  function normalizeHexColor(value, fallback) {
    if (typeof value !== 'string') return fallback === undefined ? null : fallback;
    let hex = value.trim().replace(/^#/, '');
    if (/^[0-9a-fA-F]{3}$/.test(hex)) {
      hex = hex.split('').map((char) => char + char).join('') + 'ff';
    } else if (/^[0-9a-fA-F]{4}$/.test(hex)) {
      hex = hex.split('').map((char) => char + char).join('');
    } else if (/^[0-9a-fA-F]{6}$/.test(hex)) {
      hex += 'ff';
    } else if (!/^[0-9a-fA-F]{8}$/.test(hex)) {
      return fallback === undefined ? null : fallback;
    }
    return `#${hex.toLowerCase()}`;
  }

  function alphaOf(color) {
    const normalized = normalizeHexColor(color, '#000000ff');
    return parseInt(normalized.slice(7, 9), 16);
  }

  function fontStack(value) {
    const text = String(value == null ? '' : value).trim();
    const system = '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif';
    if (!text || text.startsWith('.')) return system;
    if (text.includes(',')) return text;
    return `"${text.replace(/"/g, '\\"')}", ${system}`;
  }

  function fontWeightNumber(value) {
    return WEIGHT_NUMBERS[String(value)] || 400;
  }

  function cssValueForToken(token, value) {
    if (token.kind === 'color') return normalizeHexColor(value, '#00000000');
    if (token.kind === 'font') return fontStack(value);
    if (token.kind === 'weight') return String(fontWeightNumber(value));
    if (token.kind === 'number') {
      const number = Number(value);
      const safe = Number.isFinite(number) ? number : 0;
      return token.unit ? `${safe}${token.unit}` : String(safe);
    }
    return value == null ? '' : String(value);
  }

  function roundToStep(value, step) {
    if (!step) return value;
    const text = String(step);
    const decimals = text.includes('.') ? Math.min(6, text.split('.')[1].length) : 0;
    return Number(value.toFixed(decimals));
  }

  function clampNumber(value, min, max) {
    let result = value;
    if (min != null) result = Math.max(min, result);
    if (max != null) result = Math.min(max, result);
    return result;
  }

  function tokenValueEquals(token, left, right) {
    if (token.kind === 'color') {
      return normalizeHexColor(left, '#00000000') === normalizeHexColor(right, '#00000000');
    }
    if (token.kind === 'number') {
      const a = Number(left);
      const b = Number(right);
      if (!Number.isFinite(a) || !Number.isFinite(b)) return false;
      return Math.abs(a - b) < 0.00005;
    }
    return String(left == null ? '' : left) === String(right == null ? '' : right);
  }

  function modifiedCount(values, baseValues, tokens) {
    let count = 0;
    for (const token of tokens) {
      if (!tokenValueEquals(token, getPath(values, token.path), getPath(baseValues, token.path))) {
        count += 1;
      }
    }
    return count;
  }

  function mergeInto(target, patch) {
    if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return target;
    for (const [key, value] of Object.entries(patch)) {
      if (value === undefined || value === null) continue;
      const current = target[key];
      if (
        value &&
        typeof value === 'object' &&
        !Array.isArray(value) &&
        current &&
        typeof current === 'object' &&
        !Array.isArray(current)
      ) {
        mergeInto(current, value);
      } else {
        target[key] = deepClone(value);
      }
    }
    return target;
  }

  function deepMerge(base, patch) {
    return mergeInto(deepClone(base), patch);
  }

  function filterPatchBySchema(patch, schema) {
    if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return {};
    const result = {};
    for (const [key, value] of Object.entries(patch)) {
      if (!(key in schema)) continue;
      const schemaValue = schema[key];
      if (
        value &&
        typeof value === 'object' &&
        !Array.isArray(value) &&
        schemaValue &&
        typeof schemaValue === 'object' &&
        !Array.isArray(schemaValue)
      ) {
        result[key] = filterPatchBySchema(value, schemaValue);
      } else if (value !== undefined && value !== null) {
        result[key] = deepClone(value);
      }
    }
    return result;
  }

  function normalizeImportedTheme(raw, fallbackBaseId, bases) {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
      throw new Error('not-an-object');
    }
    const baseIds = new Set(bases.map((base) => base.id));
    let baseId = fallbackBaseId;
    let patch;
    let name = null;
    let creator = null;

    if (raw.theme && typeof raw.theme === 'object' && !Array.isArray(raw.theme)) {
      patch = raw.theme;
      if (typeof raw.base_theme_id === 'string' && baseIds.has(raw.base_theme_id)) {
        baseId = raw.base_theme_id;
      }
      if (typeof raw.name === 'string') name = raw.name;
      if (typeof raw.creator === 'string') creator = raw.creator;
    } else if (raw.colors || raw.dimensions || raw.typography || raw.placeholders) {
      patch = raw;
      if (typeof raw.name === 'string') name = raw.name;
    } else {
      throw new Error('not-a-theme');
    }

    const base = bases.find((candidate) => candidate.id === baseId) || bases[0];
    if (!base) throw new Error('no-base-theme');
    const filtered = filterPatchBySchema(patch, base.theme);
    delete filtered.name;
    const values = deepMerge(base.theme, filtered);
    if (!values.colors || !values.dimensions || !values.typography || !values.placeholders) {
      throw new Error('incomplete-theme');
    }
    return { baseId: base.id, values, name, creator };
  }

  function buildExportPack(values, name, creator, baseId) {
    const trimmedName = String(name == null ? '' : name).trim();
    const trimmedCreator = String(creator == null ? '' : creator).trim();
    if (!trimmedName) throw new Error('name-required');
    if (!trimmedCreator) throw new Error('creator-required');
    const theme = deepClone(values);
    if (!theme.colors || !theme.dimensions || !theme.typography || !theme.placeholders) {
      throw new Error('incomplete-theme');
    }
    theme.name = trimmedName;
    return {
      name: trimmedName,
      creator: trimmedCreator,
      base_theme_id: baseId,
      theme,
    };
  }

  function sanitizeFilenamePart(value) {
    return String(value == null ? '' : value)
      .trim()
      .replace(/[\\/:*?"<>|\u0000-\u001f]/g, '_')
      .replace(/\s+/g, '_')
      .slice(0, 60) || 'theme';
  }

  function buildFilename(name, creator) {
    return `${sanitizeFilenamePart(name)}_${sanitizeFilenamePart(creator)}.json`;
  }

  function defaultBaseId(prefersDark) {
    return prefersDark ? 'velora-dark' : 'velora-light';
  }

  // ---------- browser environment ----------

  function t(key, variables) {
    const dictionary = UI[state.lang] || UI.zh;
    let text = dictionary[key] || UI.zh[key] || key;
    if (variables) {
      for (const [name, value] of Object.entries(variables)) {
        text = text.replaceAll(`{${name}}`, String(value));
      }
    }
    return text;
  }

  function storageGet(key) {
    try {
      return global.localStorage ? global.localStorage.getItem(`velora-theme-editor:${key}`) : null;
    } catch (_error) {
      return null;
    }
  }

  function storageSet(key, value) {
    try {
      if (global.localStorage) global.localStorage.setItem(`velora-theme-editor:${key}`, String(value));
    } catch (_error) {
      // file:// 或隐私模式下 localStorage 可能不可用；工具功能不依赖它。
    }
  }

  function dataError(message) {
    document.body.replaceChildren();
    const card = document.createElement('div');
    card.style.maxWidth = '720px';
    card.style.margin = '80px auto';
    card.style.padding = '28px';
    card.style.borderRadius = '14px';
    card.style.background = '#161d28';
    card.style.border = '1px solid #2b394d';
    card.style.fontFamily = '-apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif';
    card.style.color = '#e6edf6';
    card.style.lineHeight = '1.6';
    const title = document.createElement('h1');
    title.style.fontSize = '18px';
    title.style.margin = '0 0 10px';
    title.textContent = 'Velora Theme Editor';
    const text = document.createElement('p');
    text.textContent = message;
    const hint = document.createElement('p');
    hint.style.color = '#8d9bad';
    hint.textContent = `${UI.zh.dataMissingHint} / ${UI.en.dataMissingHint}`;
    const command = document.createElement('pre');
    command.textContent = UI.zh.regenerateCommand;
    command.style.whiteSpace = 'pre-wrap';
    command.style.background = '#0d1117';
    command.style.padding = '12px';
    command.style.borderRadius = '8px';
    command.style.color = '#ffd479';
    card.append(title, text, hint, command);
    document.body.appendChild(card);
  }

  function showNotice(message, kind) {
    const element = document.getElementById('notice');
    if (!element) return;
    element.hidden = false;
    element.className = `notice ${kind || 'success'}`;
    element.textContent = message;
    if (state.noticeTimer) clearTimeout(state.noticeTimer);
    if (kind !== 'error') {
      state.noticeTimer = setTimeout(() => {
        element.hidden = true;
      }, 9000);
    }
  }

  function applyStaticText() {
    document.documentElement.lang = state.lang === 'zh' ? 'zh-CN' : 'en';
    document.title = t('title');
    for (const element of document.querySelectorAll('[data-i18n]')) {
      element.textContent = t(element.dataset.i18n);
    }
    for (const element of document.querySelectorAll('[data-i18n-placeholder]')) {
      element.placeholder = t(element.dataset.i18nPlaceholder);
    }
    const toggle = document.getElementById('language-toggle');
    if (toggle) {
      toggle.textContent = state.lang === 'zh' ? 'EN' : '中';
      toggle.title = t('languageToggleTitle');
    }
  }

  function baseOf(baseId) {
    return DATA.bases.find((base) => base.id === baseId) || DATA.bases[0];
  }

  function syncHeaderInputs() {
    const baseSelect = document.getElementById('base-theme');
    if (baseSelect) baseSelect.value = state.baseId;
    const nameInput = document.getElementById('theme-name');
    if (nameInput) nameInput.value = state.name;
    const creatorInput = document.getElementById('theme-creator');
    if (creatorInput) creatorInput.value = state.creator;
  }

  function applyTokenCss(token) {
    const value = getPath(state.values, token.path);
    document.documentElement.style.setProperty(token.cssVar, cssValueForToken(token, value));
  }

  function applyAllCss() {
    for (const token of DATA.tokens) applyTokenCss(token);
    updateEmptyPlaceholder();
  }

  function updateEmptyPlaceholder() {
    const element = document.getElementById('empty-placeholder');
    if (!element) return;
    const value = getPath(state.values, 'placeholders.empty_editing');
    element.textContent = typeof value === 'string' ? value : '';
  }

  function updateStatus() {
    const modified = modifiedCount(state.values, state.baseValues, DATA.tokens);
    const element = document.getElementById('modified-count');
    if (element) element.textContent = t('modifiedCount', { count: modified });
    const countElement = document.getElementById('token-count');
    if (countElement && !state.search.trim()) {
      countElement.textContent = t('tokenCount', { visible: DATA.tokens.length, total: DATA.tokens.length });
    }
  }

  function refreshRow(row, token) {
    const changed = !tokenValueEquals(
      token,
      getPath(state.values, token.path),
      getPath(state.baseValues, token.path),
    );
    row.classList.toggle('modified', changed);
    const reset = row.querySelector('.token-reset');
    if (reset) reset.disabled = !changed;
  }

  function commitValue(token, value, row) {
    setPath(state.values, token.path, value);
    applyTokenCss(token);
    if (row) refreshRow(row, token);
    if (token.path === 'placeholders.empty_editing') updateEmptyPlaceholder();
    updateStatus();
  }

  function buildColorControl(token, row) {
    const value = normalizeHexColor(getPath(state.values, token.path), '#000000ff');
    const wrapper = document.createElement('div');
    wrapper.className = 'color-control';

    const swatch = document.createElement('span');
    swatch.className = 'color-swatch';
    const colorInput = document.createElement('input');
    colorInput.type = 'color';
    colorInput.value = value.slice(0, 7);
    swatch.appendChild(colorInput);

    const hexInput = document.createElement('input');
    hexInput.type = 'text';
    hexInput.className = 'hex-input';
    hexInput.value = value;
    hexInput.spellcheck = false;
    hexInput.maxLength = 9;

    const alphaInput = document.createElement('input');
    alphaInput.type = 'range';
    alphaInput.className = 'alpha-input';
    alphaInput.min = '0';
    alphaInput.max = '255';
    alphaInput.value = String(alphaOf(value));
    alphaInput.title = 'Alpha';

    colorInput.addEventListener('input', () => {
      const next = `${colorInput.value}${value.slice(7, 9)}`;
      const normalized = normalizeHexColor(next, value);
      hexInput.value = normalized;
      alphaInput.value = String(alphaOf(normalized));
      row.classList.remove('invalid');
      commitValue(token, normalized, row);
    });

    hexInput.addEventListener('input', () => {
      const normalized = normalizeHexColor(hexInput.value, null);
      const valid = Boolean(normalized);
      row.classList.toggle('invalid', !valid);
      if (valid) {
        colorInput.value = normalized.slice(0, 7);
        alphaInput.value = String(alphaOf(normalized));
        commitValue(token, normalized, row);
      }
    });

    hexInput.addEventListener('change', () => {
      if (!normalizeHexColor(hexInput.value, null)) {
        hexInput.value = normalizeHexColor(getPath(state.values, token.path), value);
        row.classList.remove('invalid');
      } else {
        hexInput.value = normalizeHexColor(hexInput.value, value);
      }
    });

    alphaInput.addEventListener('input', () => {
      const alpha = Number(alphaInput.value).toString(16).padStart(2, '0');
      const normalized = normalizeHexColor(`${colorInput.value}${alpha}`, value);
      hexInput.value = normalized;
      row.classList.remove('invalid');
      commitValue(token, normalized, row);
    });

    wrapper.append(swatch, hexInput, alphaInput);
    return wrapper;
  }

  function buildNumberControl(token, row) {
    const value = Number(getPath(state.values, token.path));
    const wrapper = document.createElement('div');
    wrapper.className = 'color-control';
    const input = document.createElement('input');
    input.type = 'number';
    input.className = 'number-input';
    input.value = String(Number.isFinite(value) ? value : 0);
    if (token.min != null) input.min = String(token.min);
    if (token.max != null) input.max = String(token.max);
    input.step = String(token.step || 1);
    input.addEventListener('input', () => {
      const parsed = Number(input.value);
      const valid =
        Number.isFinite(parsed) &&
        (token.min == null || parsed >= token.min) &&
        (token.max == null || parsed <= token.max);
      row.classList.toggle('invalid', !valid);
      if (valid) commitValue(token, parsed, row);
    });
    input.addEventListener('change', () => {
      let parsed = Number(input.value);
      if (!Number.isFinite(parsed)) parsed = Number(getPath(state.values, token.path)) || 0;
      parsed = roundToStep(clampNumber(parsed, token.min, token.max), token.step);
      input.value = String(parsed);
      row.classList.remove('invalid');
      commitValue(token, parsed, row);
    });
    const unit = document.createElement('span');
    unit.className = 'unit';
    unit.textContent = token.unit || '';
    wrapper.append(input, unit);
    return wrapper;
  }

  function buildWeightControl(token, row) {
    const select = document.createElement('select');
    select.className = 'select-input';
    for (const weight of WEIGHTS) {
      const option = document.createElement('option');
      option.value = weight;
      option.textContent = weight.charAt(0).toUpperCase() + weight.slice(1);
      select.appendChild(option);
    }
    select.value = String(getPath(state.values, token.path));
    select.addEventListener('change', () => commitValue(token, select.value, row));
    return select;
  }

  function buildFontControl(token, row) {
    const input = document.createElement('input');
    input.type = 'text';
    input.className = 'text-input';
    input.setAttribute('list', 'font-suggestions');
    input.spellcheck = false;
    input.value = String(getPath(state.values, token.path) || '');
    input.addEventListener('input', () => commitValue(token, input.value, row));
    return input;
  }

  function buildStringControl(token, row) {
    const input = document.createElement('input');
    input.type = 'text';
    input.className = 'text-input';
    input.value = String(getPath(state.values, token.path) || '');
    input.addEventListener('input', () => commitValue(token, input.value, row));
    return input;
  }

  function buildControl(token, row) {
    switch (token.kind) {
      case 'color':
        return buildColorControl(token, row);
      case 'number':
        return buildNumberControl(token, row);
      case 'weight':
        return buildWeightControl(token, row);
      case 'font':
        return buildFontControl(token, row);
      default:
        return buildStringControl(token, row);
    }
  }

  function createTokenRow(token) {
    const row = document.createElement('div');
    row.className = 'token-row';
    row.dataset.path = token.path;

    const meta = document.createElement('div');
    meta.className = 'token-meta';
    const description = document.createElement('div');
    description.className = 'token-description';
    description.textContent = (token.description && (token.description[state.lang] || token.description.zh)) || token.key;
    const path = document.createElement('code');
    path.className = 'token-path';
    path.textContent = token.path;
    meta.append(description, path);

    const control = document.createElement('div');
    control.className = 'token-control';
    control.appendChild(buildControl(token, row));

    const reset = document.createElement('button');
    reset.type = 'button';
    reset.className = 'token-reset';
    reset.textContent = '↺';
    reset.title = t('resetToken');
    reset.addEventListener('click', () => resetToken(token));
    control.appendChild(reset);

    row.append(meta, control);
    refreshRow(row, token);
    return row;
  }

  function tokenMatches(token, query) {
    const haystack = [
      token.path,
      token.key,
      token.description && token.description.zh,
      token.description && token.description.en,
    ]
      .filter(Boolean)
      .join(' ')
      .toLowerCase();
    return haystack.includes(query);
  }

  function applySearch() {
    const query = state.search.trim().toLowerCase();
    const container = document.getElementById('token-groups');
    if (container) container.classList.toggle('searching', query.length > 0);
    let visible = 0;
    for (const groupElement of document.querySelectorAll('.group')) {
      const group = DATA.groups.find((candidate) => candidate.id === groupElement.dataset.groupId);
      const groupMatches =
        !query ||
        (group &&
          [group.label.zh, group.label.en, group.id]
            .filter(Boolean)
            .join(' ')
            .toLowerCase()
            .includes(query));
      let groupVisible = 0;
      for (const row of groupElement.querySelectorAll('.token-row')) {
        const token = tokenByPath.get(row.dataset.path);
        const match = !query || groupMatches || (token && tokenMatches(token, query));
        row.hidden = !match;
        if (match) groupVisible += 1;
      }
      groupElement.hidden = groupVisible === 0;
      visible += groupVisible;
    }

    const empty = document.getElementById('no-results');
    if (empty) {
      empty.hidden = visible !== 0;
      empty.textContent = t('noResults');
    }
    const countElement = document.getElementById('token-count');
    if (countElement) {
      countElement.textContent = t('tokenCount', {
        visible: query ? visible : DATA.tokens.length,
        total: DATA.tokens.length,
      });
    }
  }

  function renderTokenGroups() {
    const container = document.getElementById('token-groups');
    if (!container) return;
    container.replaceChildren();

    for (const group of DATA.groups) {
      const tokens = DATA.tokens.filter((token) => token.group === group.id);
      if (tokens.length === 0) continue;

      const groupElement = document.createElement('section');
      groupElement.className = 'group';
      groupElement.dataset.groupId = group.id;
      if (state.collapsed.has(group.id)) groupElement.classList.add('collapsed');

      const header = document.createElement('div');
      header.className = 'group-header';

      const toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.className = 'group-toggle';
      toggle.setAttribute('aria-expanded', String(!groupElement.classList.contains('collapsed')));
      toggle.addEventListener('click', () => {
        const collapsed = groupElement.classList.toggle('collapsed');
        toggle.setAttribute('aria-expanded', String(!collapsed));
        if (collapsed) state.collapsed.add(group.id);
        else state.collapsed.delete(group.id);
      });

      const chevron = document.createElement('span');
      chevron.className = 'group-chevron';
      chevron.textContent = '▾';
      const title = document.createElement('span');
      title.className = 'group-title';
      title.textContent = group.label[state.lang] || group.label.zh;
      const section = document.createElement('span');
      section.className = 'group-section';
      section.textContent = group.section;
      toggle.append(chevron, title, section);

      const resetGroup = document.createElement('button');
      resetGroup.type = 'button';
      resetGroup.className = 'group-reset';
      resetGroup.textContent = '↺';
      resetGroup.title = t('resetGroup');
      resetGroup.addEventListener('click', () => {
        resetGroupTokens(group.id);
      });
      header.append(toggle, resetGroup);

      const body = document.createElement('div');
      body.className = 'group-body';
      for (const token of tokens) {
        const row = createTokenRow(token);
        body.appendChild(row);
      }

      groupElement.append(header, body);
      container.appendChild(groupElement);
    }

    const empty = document.createElement('div');
    empty.id = 'no-results';
    empty.className = 'muted';
    empty.style.padding = '18px 8px';
    empty.hidden = true;
    container.appendChild(empty);

    applySearch();
    updateStatus();
  }

  function resetToken(token) {
    setPath(state.values, token.path, deepClone(getPath(state.baseValues, token.path)));
    applyTokenCss(token);
    renderTokenGroups();
    updateEmptyPlaceholder();
  }

  function resetGroupTokens(groupId) {
    for (const token of DATA.tokens) {
      if (token.group !== groupId) continue;
      setPath(state.values, token.path, deepClone(getPath(state.baseValues, token.path)));
    }
    applyAllCss();
    renderTokenGroups();
  }

  function resetAllTokens() {
    if (modifiedCount(state.values, state.baseValues, DATA.tokens) > 0 && !global.confirm(t('confirmDiscard'))) {
      return;
    }
    state.values = deepClone(state.baseValues);
    applyAllCss();
    renderTokenGroups();
  }

  function expandAllGroups(expanded) {
    state.collapsed.clear();
    if (!expanded) {
      for (const group of DATA.groups) state.collapsed.add(group.id);
    }
    for (const groupElement of document.querySelectorAll('.group')) {
      groupElement.classList.toggle('collapsed', !expanded);
      const toggle = groupElement.querySelector('.group-toggle');
      if (toggle) toggle.setAttribute('aria-expanded', String(expanded));
    }
  }

  function onBaseChange(nextBaseId) {
    if (nextBaseId === state.baseId) return;
    if (modifiedCount(state.values, state.baseValues, DATA.tokens) > 0 && !global.confirm(t('confirmDiscard'))) {
      const baseSelect = document.getElementById('base-theme');
      if (baseSelect) baseSelect.value = state.baseId;
      return;
    }
    const base = baseOf(nextBaseId);
    if (!base) return;
    state.baseId = base.id;
    state.baseValues = deepClone(base.theme);
    state.values = deepClone(base.theme);
    storageSet('base', state.baseId);
    applyAllCss();
    renderTokenGroups();
    showNotice(t('baseChanged', { name: base.name }), 'success');
  }

  function downloadText(filename, text) {
    const blob = new Blob([text], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = filename;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1500);
  }

  function onExport() {
    try {
      const pack = buildExportPack(state.values, state.name, state.creator, state.baseId);
      const filename = buildFilename(pack.name, pack.creator);
      downloadText(filename, `${JSON.stringify(pack, null, 2)}\n`);
      state.name = pack.name;
      state.creator = pack.creator;
      storageSet('name', state.name);
      storageSet('creator', state.creator);
      syncHeaderInputs();
      showNotice(t('exportSuccess', { file: filename }), 'success');
    } catch (error) {
      const message = error && error.message;
      const key =
        message === 'name-required'
          ? 'nameRequired'
          : message === 'creator-required'
            ? 'creatorRequired'
            : 'exportError';
      showNotice(t(key, { detail: message || String(error) }), 'error');
      if (message === 'name-required') document.getElementById('theme-name').focus();
      if (message === 'creator-required') document.getElementById('theme-creator').focus();
    }
  }

  function importFile(file) {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      try {
        const raw = JSON.parse(String(reader.result || ''));
        const currentModified = modifiedCount(state.values, state.baseValues, DATA.tokens);
        if (currentModified > 0 && !global.confirm(t('confirmDiscard'))) return;
        const imported = normalizeImportedTheme(raw, state.baseId, DATA.bases);
        state.baseId = imported.baseId;
        state.baseValues = deepClone(baseOf(imported.baseId).theme);
        state.values = imported.values;
        if (imported.name && imported.name.trim()) state.name = imported.name.trim();
        if (imported.creator && imported.creator.trim()) state.creator = imported.creator.trim();
        syncHeaderInputs();
        storageSet('base', state.baseId);
        storageSet('name', state.name);
        storageSet('creator', state.creator);
        applyAllCss();
        renderTokenGroups();
        showNotice(
          t('importSuccess', { name: state.name, base: baseOf(state.baseId).name }),
          'success',
        );
      } catch (error) {
        console.error(error);
        showNotice(t('importError', { detail: (error && error.message) || String(error) }), 'error');
      }
    };
    reader.onerror = () => {
      showNotice(t('importError', { detail: 'read-error' }), 'error');
    };
    reader.readAsText(file);
  }

  function selectTab(tab) {
    const showDocument = tab === 'document';
    const documentTab = document.getElementById('tab-document');
    const interfaceTab = document.getElementById('tab-interface');
    const documentPanel = document.getElementById('panel-document');
    const interfacePanel = document.getElementById('panel-interface');
    if (!documentTab || !interfaceTab || !documentPanel || !interfacePanel) return;
    documentTab.classList.toggle('selected', showDocument);
    documentTab.setAttribute('aria-selected', String(showDocument));
    interfaceTab.classList.toggle('selected', !showDocument);
    interfaceTab.setAttribute('aria-selected', String(!showDocument));
    documentPanel.hidden = !showDocument;
    interfacePanel.hidden = showDocument;
    storageSet('tab', showDocument ? 'document' : 'interface');
  }

  function setupFontSuggestions() {
    const datalist = document.createElement('datalist');
    datalist.id = 'font-suggestions';
    for (const font of FONT_SUGGESTIONS) {
      const option = document.createElement('option');
      option.value = font;
      datalist.appendChild(option);
    }
    document.body.appendChild(datalist);
  }

  function init() {
    if (!DATA || !Array.isArray(DATA.tokens) || !Array.isArray(DATA.bases) || !Array.isArray(DATA.groups)) {
      dataError(`${UI.zh.dataMissing} / ${UI.en.dataMissing}`);
      return;
    }

    const savedLang = storageGet('lang');
    const browserLang = (global.navigator && global.navigator.language) || 'en';
    state.lang = savedLang === 'en' || savedLang === 'zh'
      ? savedLang
      : browserLang.toLowerCase().startsWith('zh')
        ? 'zh'
        : 'en';

    const savedBase = storageGet('base');
    const prefersDark =
      typeof global.matchMedia === 'function' &&
      global.matchMedia('(prefers-color-scheme: dark)').matches;
    const fallbackBase = defaultBaseId(prefersDark);
    state.baseId = DATA.bases.some((base) => base.id === savedBase) ? savedBase : fallbackBase;
    state.baseValues = deepClone(baseOf(state.baseId).theme);
    state.values = deepClone(state.baseValues);
    state.name = storageGet('name') || 'My Theme';
    state.creator = storageGet('creator') || 'Anonymous';

    setupFontSuggestions();
    applyStaticText();
    syncHeaderInputs();

    const baseSelect = document.getElementById('base-theme');
    if (baseSelect) {
      baseSelect.replaceChildren();
      for (const base of DATA.bases) {
        const option = document.createElement('option');
        option.value = base.id;
        option.textContent = base.name;
        baseSelect.appendChild(option);
      }
      baseSelect.value = state.baseId;
      baseSelect.addEventListener('change', () => onBaseChange(baseSelect.value));
    }

    const nameInput = document.getElementById('theme-name');
    if (nameInput) {
      nameInput.value = state.name;
      nameInput.addEventListener('input', () => {
        state.name = nameInput.value;
        storageSet('name', state.name);
      });
    }

    const creatorInput = document.getElementById('theme-creator');
    if (creatorInput) {
      creatorInput.value = state.creator;
      creatorInput.addEventListener('input', () => {
        state.creator = creatorInput.value;
        storageSet('creator', state.creator);
      });
    }

    const searchInput = document.getElementById('token-search');
    if (searchInput) {
      searchInput.addEventListener('input', () => {
        state.search = searchInput.value;
        applySearch();
      });
    }

    const expand = document.getElementById('expand-groups');
    if (expand) expand.addEventListener('click', () => expandAllGroups(true));
    const collapse = document.getElementById('collapse-groups');
    if (collapse) collapse.addEventListener('click', () => expandAllGroups(false));
    const resetAll = document.getElementById('reset-all');
    if (resetAll) resetAll.addEventListener('click', resetAllTokens);

    const exportButton = document.getElementById('export-button');
    if (exportButton) exportButton.addEventListener('click', onExport);

    const importButton = document.getElementById('import-button');
    const importInput = document.getElementById('import-input');
    if (importButton && importInput) {
      importButton.addEventListener('click', () => importInput.click());
      importInput.addEventListener('change', () => {
        const file = importInput.files && importInput.files[0];
        importFile(file);
        importInput.value = '';
      });
    }

    const languageToggle = document.getElementById('language-toggle');
    if (languageToggle) {
      languageToggle.addEventListener('click', () => {
        state.lang = state.lang === 'zh' ? 'en' : 'zh';
        storageSet('lang', state.lang);
        applyStaticText();
        renderTokenGroups();
        updateEmptyPlaceholder();
        showNotice(state.lang === 'zh' ? '已切换到中文' : 'Switched to English', 'success');
      });
    }

    const documentTab = document.getElementById('tab-document');
    if (documentTab) documentTab.addEventListener('click', () => selectTab('document'));
    const interfaceTab = document.getElementById('tab-interface');
    if (interfaceTab) interfaceTab.addEventListener('click', () => selectTab('interface'));
    selectTab(storageGet('tab') === 'interface' ? 'interface' : 'document');

    document.addEventListener('keydown', (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        onExport();
      }
    });

    applyAllCss();
    renderTokenGroups();
    updateEmptyPlaceholder();
  }

  const api = {
    normalizeHexColor,
    alphaOf,
    fontStack,
    fontWeightNumber,
    cssValueForToken,
    roundToStep,
    clampNumber,
    tokenValueEquals,
    modifiedCount,
    deepClone,
    deepMerge,
    filterPatchBySchema,
    normalizeImportedTheme,
    buildExportPack,
    sanitizeFilenamePart,
    buildFilename,
    defaultBaseId,
    getPath,
    setPath,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
  global.VeloraThemeEditor = api;

  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', init);
    } else {
      init();
    }
  }
})(typeof globalThis !== 'undefined' ? globalThis : this);
