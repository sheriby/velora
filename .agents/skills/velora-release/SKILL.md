---
name: velora-release
description: Cut a Velora release — decide the version, write docs/releases/vX.Y.Z.md and CHANGELOG.md from the commit history since the last tag, bump Cargo.toml/Cargo.lock, commit chore(release), then (only after the user confirms) push an annotated vX.Y.Z tag that CI turns into packages and a GitHub Release. Use when asked to 发版 / 打 tag / 出包 / 写 release notes / 更新 CHANGELOG, or when a version number needs bumping. Triggers include "发个版本", "打 tag", "release note", "该发版了".
---

# Velora 发版

主线：**写 release notes → 改版本号与 CHANGELOG → 全量绿 → 一笔 `chore(release)` → 注释 tag → 派人确认后 push → CI 出包建 Release。**

多数时候要干的活就是第一件：拿上一版到现在的提交历史，写成一份给人读的 release notes。

## 1 流水线事实（照这个来，别猜）

- 触发：push `v*` tag（`.github/workflows/release.yml`）；或手动 `workflow_dispatch`（输入 tag）只更新 notes。
- **tag 名就是版本号**：`vX.Y.Z`，预发布加后缀（`v0.1.5-beta.1`）。CI 会按 tag 改自己检出里的 `Cargo.toml` 与 `Cargo.lock`；本地打包 `scripts/package-macos.sh:5` 仍读 `Cargo.toml` 的版本，所以仓库里该 bump 还得 bump。
- **notes 路径 = `docs/releases/<tag>.md`**，例 `docs/releases/v0.2.2.md`。文件不存在或只有空白 → CI 退回 `--generate-notes`，发布说明就不是你写的那份了。
- tag 里含 `beta` → 该 Release 自动标 prerelease（`--latest=false`）。
- 产物名（CI 定死）：`velora-<version>-macos-arm64.pkg`、`velora-<version>-macos-x64.pkg`、`velora-<version>-windows-x64-setup.exe`。
- 发完想补 notes：把文件提交后跑 `workflow_dispatch`（输入同一个 tag），它执行 `gh release edit --notes-file`；文件缺失时这个 job 直接报错退出。
- tag 用**注释标签**，消息写一句话：`v0.2.2: 选中菜单、右键菜单与命令面板补齐就近入口…`（见 `git for-each-ref refs/tags`）。旧的 `dist/RELEASE-NOTES-*.md` 是 0.1.x 的老写法，现行只写 `docs/releases/`。

## 2 从提交历史攒素材

```bash
prev=$(git describe --tags --abbrev=0 --exclude='*-beta*' vX.Y.Z^)   # 上一个正式 tag
git log --oneline "$prev..HEAD"          # 一笔一句，先看清这批有多少主题
git log --format='%s%n%b' "$prev..HEAD"  # 正文里的背景/实现/影响/刻意不做，notes 的原料
git diff --stat "$prev..HEAD" | tail -1  # 文件数、+/- 行 → 写进「测试与可靠性」
git rev-list --count "$prev..HEAD"       # 笔数
cargo test --bin velora                  # 测试数当场取，不沿用记忆
```

- **按用户点得到的入口分组**，不按提交顺序。v0.2.2 的分法是：正文右键菜单 / 选中工具栏 / 行内格式与段落 / 插入与剪贴板 / 命令面板与键位 / 标签、现场与落盘。
- 语义变化、已知限制、「刻意不做」从各笔提交正文里**汇总**，不在发布那一笔新造判断。
- 报修类改动写成「现象 → 现在怎样」，不写内部符号名，也不写叙事口吻（见提交信息技能的写作口径）。
- 界面数字用实测值（例：主面板 389 高收到 234、宽 229）；没实测的别写。

## 3 写 release notes：`docs/releases/vX.Y.Z.md`

骨架照**最新一份** notes（`docs/releases/v0.2.2.md` 是当前形态）；`docs/releases/TEMPLATE.md` 是最小清单，**它列的段一个都不能少**（下载 / 新特性 / 修复 / 破坏性变更 / 其他 / 完整变更链接）。段落名以 notes 为准：

```
# vX.Y.Z

<概括段：2–4 句，讲用户拿到什么，不写实现细节>

## 📦 下载

| 平台 | 文件 | 说明 |
| :--- | :--- | :--- |
| macOS (Apple Silicon) | `velora-X.Y.Z-macos-arm64.pkg` | 双击安装 |
| macOS (Intel) | `velora-X.Y.Z-macos-x64.pkg` | 双击安装 |
| Windows (x64) | `velora-X.Y.Z-windows-x64-setup.exe` | 双击安装 |

## ✨ 新增        （按入口分组，粗体小标题）
## ⚡ 性能        （有实测前后数据才写）
## 🐛 修复
## ⚠️ 语义变化     （用户要调整行为的地方）
## 🧪 测试与可靠性  （测试数 旧 → 新 通过/失败/ignored、文件数、+/- 行、笔数、新增测试模块）
## ⚠️ 已知限制     （从各笔「刻意不做/没验」汇总）
## 📝 其他        （依赖、文档、图标、i18n、发布流程）

---

**完整变更**：https://github.com/sheriby/velora/compare/v<prev>...vX.Y.Z
```

写法：面向用户，讲行为与入口；每条能追到一笔提交或一个测试；没内容的段不写（TEMPLATE 里的「破坏性变更」不能省——没有就写清楚没有）。

## 4 CHANGELOG 与版本号

- `CHANGELOG.md` 顶部插 `## [X.Y.Z] - YYYY-MM-DD`，先一句概括段，再按内容取小节：`### ✨ 新特性` / `### ⚡ 性能` / `### 🐛 修复` / `### ⚠️ 语义变化` / `### ⚠️ 破坏性变更` / `### 🧪 测试` / `### ⚠️ 已知限制` / `### 📝 其他`。格式是 Keep a Changelog（中文小节名），**小节名与 notes 不完全同名**：CHANGELOG 用「✨ 新特性」，notes 用「✨ 新增」。
- 版本号：`Cargo.toml` 第一处 `version = "X.Y.Z"`；`Cargo.lock` 里 `[[package]] name = "velora"` 那一段的 `version`。
- 怎么定号：`feat` → minor，`fix`/`perf` → patch，破坏性 → major；预发布写 `-beta.N`。没有 feat 也没有破坏性就 patch；**minor / major 先问人**。

## 5 提交与打 tag

1. `cargo build`（零警告）+ `cargo test --bin velora`（全绿）——把实测数字写进 notes 的「测试与可靠性」和提交正文的「测试结果」。
2. 一笔提交：`chore(release): vX.Y.Z 版本号、CHANGELOG 与发布说明`，只动四个文件：`Cargo.toml`、`Cargo.lock`、`CHANGELOG.md`、`docs/releases/vX.Y.Z.md`。
3. `git status` 干净（`dist/` 与构建产物不进工作树）。
4. 注释 tag：`git tag -a vX.Y.Z -m "vX.Y.Z: <一句话>"`。
5. **停下来问人**：push tag 是公开且不可逆的动作（触发 CI 出三个安装包 + 建 GitHub Release，tag 不能改名重发）。先把这四样摆出来等确认——tag 名、指向的 commit（`git rev-parse --short HEAD`）、notes 文件路径、三个产物名——确认后 `git push origin vX.Y.Z`。
6. push 之后 CI 跑 `build → release`；产物与 Release 出现后，若要改文案：提交 notes → `workflow_dispatch`（输入 tag）。

## 6 发版检查清单

- [ ] notes 文件名与 tag 完全一致（`docs/releases/vX.Y.Z.md`）且已提交
- [ ] 下载表文件名符合 CI 命名规则（`-macos-arm64.pkg` / `-macos-x64.pkg` / `-windows-x64-setup.exe`）
- [ ] 「完整变更」链接的 prev 是上一个 tag（`git describe --tags --abbrev=0`）
- [ ] `CHANGELOG.md` 有日期；`Cargo.toml` 与 `Cargo.lock` 版本一致
- [ ] 测试数字与行数当场取的；验证边界写清（哪些只在测试里验过、没上实机）
- [ ] tag 是注释标签，消息是一句话
- [ ] push 前已得到用户确认
