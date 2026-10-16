# Velora

**English** · [简体中文](README_CN.md)

---

Velora is a native Markdown editor built on Rust and GPUI: no Electron, no WebView, with the interface rendered directly on the GPU — faster startup, lower overhead, steadier response. WYSIWYG by default, switchable to Markdown source at any time. Beyond raw performance, the interface stays modern, clean and readable — light and fluid.

## 🖼️ Screenshots

*WYSIWYG editing: inline styles, tables, code highlighting and LaTeX, with tabs merged into the title bar.*

![Live WYSIWYG Markdown editing: inline styles, tables, code highlighting and LaTeX](assets/screenshots/editor.png)

*The workspace: file tree and multiple tabs, with frontmatter rendered as a property card, collapsible headings and syntax-highlighted code blocks.*

![The Velora workspace: file tree, multiple tabs and rendered Markdown (frontmatter card, collapsible headings, lists and code blocks)](assets/screenshots/workspace.png)

*Six built-in themes, with automatic switching to the system appearance.*

![The six built-in themes: Velora Light, Paper, Forest, Velora Dark, Midnight and Ink](assets/screenshots/themes.png)

## ✨ Features

### Writing & editing

- **WYSIWYG + source modes** — Markdown renders as you type; switch to raw source at any time, and undo history survives mode switches.
- **A full block set** — headings, ordered/bulleted/task lists, tables, code blocks, quotes, footnotes, callouts, horizontal rules and images; inline bold/italic, inline code, links and strikethrough.
- **Math & diagrams in place** — LaTeX formulas and Mermaid diagrams render directly inside the document.
- **Images** — paste or drag one in and it is copied into the document's assets folder with a relative link; 100% maps to 95% of the visible text column by default and images never overflow it; right-click offers 25% / 50% / 75% / 100% scaling that keeps the aspect ratio, and the chosen ratio is written back to the source as `{width=NN%}`.
- **Editing conveniences** — lists continue on `Enter`; typing a paired character wraps the selection; pasting a URL over a selection creates a link; HTML copied from a browser or Word is converted to Markdown on paste.
- **Smart punctuation** (off by default, one switch in Settings) — context-aware curly quotes and `--` → em dash.
- **VS Code–style find & replace** — powered by ripgrep's engine, in-process: case, whole-word, regex (multi-line patterns included) and fuzzy matching; every match is highlighted while the panel is open, and the result list, highlighting, jumping and replace-all read the same match table.
- **Syntax highlighting** — Rust, JavaScript/TypeScript, C/C++, C#, Go, Java, PHP, Python, Ruby, HTML/CSS, JSON, YAML, TOML and Bash; other text files open and edit as plain text.

### Workspace & knowledge

- **Folder = workspace** — the file tree creates and renames files and folders with inline input, duplicates, copies / pastes and deletes (to the system Trash); sort order (name, modification time or type) is set in Settings; right-click reveals a file in the system file manager or copies its absolute / relative path; hover shows its size and modification time.
- **Global search** matches file names *and* contents, with workspace-wide replace; file bodies are decoded the same way documents are (UTF-8 first, GB18030 fallback).
- **Image preview** — JPG, PNG, WebP, ICO and other images open from the file tree, Open File, quick switcher, recent files or a startup argument as read-only previews that scale to fit the window, with dimensions in the status bar; images never enter the text buffer or save path.
- **Outline pane** — click to jump, follows scrolling to highlight the current section, double-click a heading to rename it in the body.
- **Knowledge links** — `[[wikilinks]]` open or create the note; `[TOC]` renders as a clickable table of contents.
- **Frontmatter & structure** — YAML frontmatter is preserved verbatim; headings fold; links and footnote references show hover previews.
- **Links that behave** — `Cmd/Ctrl+click` goes straight to the target: external URLs to the browser, local files in-app, `#anchors` within the document.
- **External changes** — a clean tab reloads automatically when its file changes on disk; edited tabs get a conflict prompt instead of a silent overwrite.

### Navigation & windows

- **⌘P quick switcher** with IME-friendly input, **⇧⌘P command palette** covering every command.
- **Tabs merged into the title bar** — drag to reorder, `⌘1–9` to jump, middle-click to close, right-click for bulk close.
- **Cursor history** — `⌥⌘←` / `⌥⌘→` walk back and forward across files.
- **Session & windows** — welcome page for empty workspaces; previous session's tabs, window size and position restored on launch; sidebar visibility and the last used panel are remembered (the startup behavior is configurable in Settings); `⌘+` / `⌘−` / `⌘0` zoom; fullscreen toggle.
- **Status bar** — word and selection counts, estimated reading time, clickable breadcrumb path.

### Reliability & performance

- **Your text survives** — autosave with a configurable debounce, atomic writes (temp file + rename), and automatic recovery of unsaved content after a crash.
- **Big documents stay fast** — huge manuscripts build their first screen synchronously while the rest streams in through background chunks; typing and scrolling never stall.
- **Fast startup** — initialization is spread across frames, so the window appears without delay.
- **Built-in updates** — checks GitHub releases at startup (can be disabled or set to include prereleases in Settings; manual check from the menu or command palette); update notices download a SHA-256-verified installer; installing restarts the app.
- **Guarded by tests** — 1,900+ automated tests, golden snapshots of rendered block structure, and a per-block import budget that fails CI on regression.

### Export

- **Formats** — single-file HTML with images embedded as data URIs, PDF, full-height PNG capture, system printing, and copy-as-HTML.
- **Export theme** — pick *current*, *light* or *dark* for anything you export.
- **Chromium requirement** — PDF, printing and PNG rendering go through a headless Chromium. Have Chrome, Chromium or Edge installed, or point the `CHROME` environment variable at a browser binary.

### Look & feel

- **Six built-in themes** — Velora Light / Dark, Paper, Forest, Midnight and Ink — plus automatic switching with the system appearance; custom theme packs load from the user directory (see the [theme token reference](docs/theme-tokens.md)).
- **Focus & typewriter modes**; interface, body and code fonts and sizes set independently; a configurable writing measure.
- **Bilingual UI** — Simplified Chinese and English built in; external language packs load by dropping them into the user directory.
- **In-app preferences** — one window for files, themes, images, shortcuts, window and status-bar options.

## ⌨️ Keyboard shortcuts

macOS keys shown; Ctrl-based equivalents work on other platforms. Shortcuts are customizable in Settings → Shortcuts.

| Keys | Action |
| --- | --- |
| `⌘P` | Quick file switcher |
| `⇧⌘P` | Command palette |
| `⌘F` | Find & replace (`⌘G` next match, `⇧⌘G` previous) |
| `⌃Tab` | Toggle WYSIWYG / source mode |
| `⌘B` / `⌘I` / `⌘U` / `` ⌘` `` | Bold / italic / underline / inline code |
| `⇧⌘A` | Select the entire document |
| `⌘S` / `⇧⌘S` | Save / save as |
| `⇧⌘C` | Copy selection or document as HTML |
| `⌥⌘P` | Print |
| `⌘1`–`⌘9` | Switch to tab N |
| `⌥⌘←` / `⌥⌘→` | Cursor history: back / forward |
| `⌘+` / `⌘−` / `⌘0` | Zoom in / out / reset |
| `⌃⌘F` / `F11` | Toggle fullscreen |
| `⌃W` | Toggle sidebar |
| `⌘N` / `⌘O` | New document / open file |

## 🚀 Building from source

Requires the **Rust stable toolchain**.

The repository's `.cargo/config.toml` wires `sccache` in as the compiler cache wrapper, so make sure it is installed first:

```bash
cargo install --locked sccache --version 0.17.0
```

Then:

```bash
cargo build
cargo run                       # launch with an empty window
cargo run /path/to/workspace    # open a folder as a workspace
cargo test
```

## 📦 Release packages

- **macOS** — run `scripts/package-macos.sh` to produce `dist/velora.app` and `dist/velora-<version>.pkg`. The package is unsigned and unnotarized, intended for local and small-scale internal use.
- **Windows** — run `./scripts/package-windows.ps1` in PowerShell 7 on Windows x64 to build a release installer with the native MSVC toolchain: `dist/velora-<version>-windows-x64-setup.exe`. Requires the Rust MSVC toolchain, Visual Studio C++ Build Tools, the Windows SDK and NSIS. CI builds and tests on native Windows runners.

## ⚙️ Configuration

Configuration lives in the OS app-config directory (`~/Library/Application Support/velora/` on macOS):

| File / folder | Purpose |
| --- | --- |
| `config.toml` | Preferences — most entries are editable in the preferences window |
| `session.json` | Per-workspace open tabs, active tab and sidebar width |
| `languages/` | External language packs |
| `themes/` | External theme packs |

The main `config.toml` sections:

| Section | Keys | Controls |
| --- | --- | --- |
| `[window]` | `default_window_width`, `default_window_height`, `open_position`, `remember_bounds`, `zoom_percent` | Default size, centered vs. remembered opening position, window memory, UI zoom |
| `[editor]` | `tree_sort`, `autosave`, `autosave_debounce_ms`, `new_file_template`, `smart_punctuation`, `external_change_policy`, `delete_policy`, `workspace_sidebar_width` | File-tree sort, autosave on/off, autosave interval, new-file template (`{date}` expands), smart punctuation, external-change handling, Trash vs. permanent delete, sidebar width |
| `[export]` | `theme` | `current` / `light` / `dark` for exported HTML, PDF and PNG |
| `[updates]` | `check_on_startup`, `include_prereleases` | Check for updates at startup; include prerelease versions |

## 🙏 Acknowledgements

- [**Zed**](https://github.com/zed-industries/zed): The GPUI framework behind Velora's native, GPU-rendered interface.
- [**Velotype**](https://github.com/manyougz/velotype): Reference for the editor's early implementation.
- [**Merman**](https://github.com/Latias94/merman): Native Mermaid parsing, layout and SVG rendering.
- [**RaTeX**](https://github.com/erweixin/RaTeX): LaTeX math parsing, layout and SVG rendering.
- [**Tree-sitter**](https://github.com/tree-sitter/tree-sitter): Language parsers and syntax highlighting.
- [**ripgrep**](https://github.com/BurntSushi/ripgrep): In-process search and file traversal through its `grep-*` and `ignore` crates.
- [**pulldown-cmark**](https://github.com/pulldown-cmark/pulldown-cmark): Markdown parsing for HTML export.
- [**htmd**](https://github.com/letmutex/htmd): HTML-to-Markdown conversion for rich-text paste.
- [**resvg**](https://github.com/linebender/resvg): SVG rasterization in GPUI.
- [**chromiumoxide**](https://github.com/mattsse/chromiumoxide): Chromium automation for PDF and PNG export and printing.

## 📄 License

[Apache-2.0](LICENSE-APACHE)
