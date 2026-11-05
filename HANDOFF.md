# 交接文档：oculante-mod（oculante 输入操作改造）

> 更新：2026-11-06 ｜ 状态：**已实现、已验收、已推送 fork、冻结**
> 技术规格见 `docs/INPUT_REMAP_PLAN.md`；逐项改动见 `docs/MOD_CHANGELOG.md`；方法与坑见 `docs/MOD_WORKFLOW.md`。
> 本文件是**上下文与运维手册**：账号、远端、Git 规范、发布流程、排障。**新会话从这一篇开始读。**

---

## 0. 一页速览

| 项目 | 内容 |
|---|---|
| 目标 | 把 oculante 的鼠标操作改成"顺手"的一套（滚轮翻图 / 右键缩放 / 中键镜像与精密旋转等），并支持多窗口 |
| 结论 | 只改输入映射 + 禁用 ClearImage；旋转与镜像**仅显示不写盘**；多窗口用多进程实现 |
| 上游 | <https://github.com/woelper/oculante>（基线 `master` @ `fdc05eb`，v0.9.6） |
| 本仓库 | **<https://github.com/Zenobins/oculante-mod>**（公开，默认分支 `feat/input-remap`） |
| 差异 | <https://github.com/Zenobins/oculante-mod/compare/master...feat/input-remap>（19 提交 / 24 文件，截至 2026-11-06） |
| 交付 | `dist/Oculante.app`（`0.9.6-mod.20261106-0229`）、`dist/Oculante-0.9.6-mod-20261106-0229.zip`、`input-remap.patch` |
| 测试 | `cargo test --lib --bin oculante` → 41 + 1 全绿 |
| 本机状态 | 已装到 `/Applications`（用户自装），多窗口、置顶独立、Finder 打开第二张图均实测通过 |
| 当前阻塞 | 无 |
| 下次先看 | §2 现状 → §3 GitHub → §4 Git 规范 → §7 文档维护规则 |

---

## 1. 时间线（2026-11-05 → 06）

| 时间 | 做了什么 | 产物 |
|---|---|---|
| 11-05 22:2x | `--depth 1` 克隆上游，摸清事件循环 / 快捷键表 / 绘制链路 | 代码地图 |
| 11-05 22:4x | 分析需求、写方案 v1→v5（中键镜像、精密旋转、几何不变量） | `docs/INPUT_REMAP_PLAN.md` |
| 11-05 23:0x | 装 cmake（brew 被沙箱拦住，改便携包）、建分支、写 `gestures.rs` + 单测 | 26 个单测 |
| 11-05 23:2x | 接入事件、绘制矩阵、禁用 ClearImage，首次出 release | `target/release/oculante` |
| 11-05 23:5x | 用户反馈缩放不跟鼠标 / 平移方向被旋转影响 → **几何模型返工**（offset 是纯屏幕平移） | 2 个不变量单测 |
| 11-06 00:0x | 滚轮灵敏度：单阈值模型失败 → **行/像素分类 + 手势分段** | M1 |
| 11-06 00:3x | `.app` 打包脚本 + 升级用变更日志 + 方法论 | M10、docs |
| 11-06 01:2x | 多窗口（多进程）+ 每窗口独立置顶 + 设置单写者 | M11 |
| 11-06 02:0x | 修上游 #33：运行中打开第二张图报错、再次双击 .app 不开新窗口 | M12 |
| 11-06 02:3x | 多选打开合并（`openFiles:` + 250ms） | M12 |
| 11-06 02:4x | 推 fork、改名 `oculante-mod`、README 顶部加说明、默认分支切到 `feat/input-remap` | 本文件 |

---

## 2. 现状

### 2.1 仓库与远端

| remote | URL | 用途 |
|---|---|---|
| `origin` | `https://github.com/woelper/oculante.git` | **上游**，只读用途；`git fetch origin` 看更新 |
| `fork` | `https://github.com/Zenobins/oculante-mod.git` | **推送目标**；`git push fork feat/input-remap` |

- fork 默认分支已设为 **`feat/input-remap`**，访客打开仓库直接看到 mod 说明；`master` 保持上游镜像，不写入任何本地改动。
- 本机分支 `feat/input-remap` 跟踪 `fork/feat/input-remap`。
- 克隆是 `--depth 1` 起家，推 fork 前必须 `git fetch --unshallow origin`（否则 GitHub 拒绝浅推送）。

### 2.2 交付物

| 项 | 值 |
|---|---|
| 源码分支 | `feat/input-remap`，19 个提交 |
| 应用 | `dist/Oculante.app`，版本 `0.9.6-mod.20261106-0229` |
| 分享包 | `dist/Oculante-0.9.6-mod-20261106-0229.zip`（17 MB） |
| 补丁 | `input-remap.patch`（`git diff master..HEAD`，未入库） |
| 二进制 | `target/release/oculante`，30 MB，sha256 前缀 `0a2f6efb`，无 Homebrew 依赖 |

版本号规则：`<cargo version>-mod.<构建时间戳>`，由 `scripts/make_macos_app.sh` 写入 `CFBundleShortVersionString`，Finder「显示简介」可核对。

### 2.3 账号与工具

| 项 | 值 |
|---|---|
| GitHub 账号 | **Zenobins**（2026-10 前后由 `DogDod` 改名；`gh auth status` 里可能仍显示旧 label，**以 `gh api user --jq .login` 为准**） |
| gh | 2.102.0（Homebrew），登录 github.com，凭据在 keyring |
| token scopes | `gist, read:org, repo, workflow` |
| 推送协议 | HTTPS + keyring 凭据助手（本机**没有**注册 SSH key） |
| 重新登录 | `gh auth login --hostname github.com --git-protocol https --web` |
| 切换账号 | `gh auth switch --user <login>` |

> 任何情况下不要把 token 写进仓库、文档或提交信息；本仓库所有文档都不含凭据。

### 2.4 本机路径

| 用途 | 路径 |
|---|---|
| 仓库 | `~/oculante`（本机实际：`~/WorkBuddy/应用mod/oculante`） |
| 便携 cmake | `~/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin`（240 MB，可删） |
| 产物 | 仓库内 `dist/`、`target/`（都被 `.gitignore` 忽略） |
| 日志 | 运行时可加 `RUST_LOG=debug`，滚轮原始值用 `RUST_LOG=trace ... \| grep "Mouse wheel"` |

---

## 3. GitHub 用法

### 3.1 认证与身份

```bash
gh auth status                      # 看登录状态（注意 label 可能是旧用户名）
gh api user --jq '.login, .name'    # 看真实身份 ← 以这个为准
gh auth switch --user Zenobins      # 多账号时切换
```

### 3.2 建 fork（推荐用 API，别用 `gh repo fork`）

`gh repo fork <repo>` 会把现有 `origin` 改名成 `upstream`、再把 fork 设成 `origin`，容易把本地远端搞乱。用 API 只建远端仓库、不动本地配置：

```bash
cd ~/oculante
gh api repos/woelper/oculante/forks -X POST --jq '.full_name'
git remote add fork https://github.com/Zenobins/oculante-mod.git   # 已配好，不用重复
```

改名 / 改默认分支：

```bash
gh api repos/Zenobins/oculante -X PATCH -f name=oculante-mod --jq '.full_name'
gh api repos/Zenobins/oculante-mod -X PATCH -f default_branch=feat/input-remap --jq '.default_branch'
```

### 3.3 看差异 / 查内容

```bash
# 与上游的差异（提交数 + 文件列表）
gh api "repos/Zenobins/oculante-mod/compare/master...feat/input-remap" \
  --jq '"commits=\(.ahead_by) files=\(.files|length)"'
gh api "repos/Zenobins/oculante-mod/compare/master...feat/input-remap" \
  --jq '.files[] | "\(.status)\t+\(.additions) -\(.deletions)\t\(.filename)"'

# 读远端文件（注意 ref：默认分支不是 master 时要显式给 ?ref=）
gh api "repos/Zenobins/oculante-mod/contents/docs?ref=feat/input-remap" --jq '.[].name'
curl -s https://raw.githubusercontent.com/Zenobins/oculante-mod/feat/input-remap/README.md | head
```

> 坑：`contents` 接口默认取**默认分支**；路径在当前分支不存在时会 404，不是文件丢了。

### 3.4 PR / Release（需要时）

```bash
# 从 fork 向上游开 PR（本仓库未开，上游处于 maintenance mode）
gh pr create --repo woelper/oculante --base master --head Zenobins:feat/input-remap \
  --title "..." --body-file <(sed -n '1,60p' docs/MOD_CHANGELOG.md)

# 在 fork 上发布 App（别人不用编译就能下载）
gh release create mod-0.9.6 \
  dist/Oculante-0.9.6-mod-20261106-0229.zip input-remap.patch \
  --repo Zenobins/oculante-mod \
  --title "oculante-mod 0.9.6 (input remap)" \
  --notes-file <(sed -n '1,40p' docs/MOD_CHANGELOG.md)
```

### 3.5 同步上游

```bash
git fetch origin
git log --oneline master..origin/master          # 上游有哪些新提交
git diff master origin/master --stat             # 改了哪些文件
git rebase origin/master                         # 或 merge，保持"附加提交"形态
# 冲突按 docs/MOD_CHANGELOG.md §4 的"上游改动 → 风险 → 处理"表判断
```

---

## 4. Git 规范

### 4.1 分支

| 分支 | 规则 |
|---|---|
| `master` | **上游镜像，禁止直接提交**（否则以后 rebase 冲突且 diff 失真） |
| `feat/input-remap` | 本次改造的全部提交，默认分支；历史上已于验收后冻结 |
| 未来的大改动 | 从 `master` 拉 `feat/<short-name>`，验收后再决定是否合并进 `feat/input-remap` |

### 4.2 提交信息（Conventional Commits + 说明"为什么"）

```
<type>(<scope>): <祈使句，≤72 字符，英文>

<正文：为什么这么改、影响面、坑；一行一句，≤72 字符/行>
```

- `type`：`feat` / `fix` / `docs` / `chore` / `refactor` / `test`
- `scope`：`input`、`gestures`、`mac`、`dist`、`docs`
- 本次示例：`feat(input): remap mouse gestures and add display-only rotate/mirror`、`fix(mac): handle Finder opens while running and reopen as new window`
- **一次提交一件事**：逻辑内核（`gestures.rs`）/ 接线（事件与绘制）/ 禁用功能 / 文档 / 打包，各自独立，便于 revert 与阅读 diff
- 提交信息里写清"上游原实现 → 本地实现"的话，半年后回看最省力

### 4.3 禁止 / 避免

| 禁止 | 原因 |
|---|---|
| `git commit --amend` 已推送的提交 | 会让 fork 与本地不一致；需要修正就再提一个 `fix` 提交 |
| `git push --force`（除明确重整自己分支且告知用户） | 同上 |
| 提交 `dist/`、`target/`、`input-remap.patch`、`*.zip` | 体积大；已在 `.gitignore` |
| 提交本机绝对路径 / token / 截图里的个人信息 | fork 是公开的；文档统一用 `~/oculante`、`$HOME/tools/...` |
| 在 `master` 上提交 | 见 §4.1 |

### 4.4 提交前检查（硬门禁）

```bash
cargo test --lib --bin oculante     # 41 + 1 必须全绿
cargo check --bin oculante          # 不要用 --all-targets（上游 bench 是坏的）
git status --short                  # 只应出现本次要提交的文件
```

### 4.5 `.gitignore` 的坑（已踩）

上游 `.gitignore` 里有一行 `Info.plist`，会把新建的 `packaging/macos/Info.plist` 一起忽略——曾经导致 fork 里没有该文件、别人 clone 后**打不出 .app**。已加例外：

```
!packaging/macos/Info.plist
```

新增打包/资源文件时先 `git status --ignored` 确认没被吞掉。

---

## 5. 构建与发布

```bash
export PATH="$HOME/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"
cd ~/oculante

cargo test --lib --bin oculante
cargo build --release          # 首次 ~11 分钟，增量 3~6 分钟（lto = "fat"）
./scripts/make_macos_app.sh    # -> dist/Oculante.app + dist/Oculante-<ver>-mod-<stamp>.zip
```

- 脚本末尾会打印源二进制 sha 前 16 位、构建时间与是否仍链接 Homebrew 库（应为 `none`）。
- 包内可执行文件的 sha 与 `target/release/oculante` **不同是正常的**：ad-hoc 签名会改写 Mach-O。
- 发布到 fork：见 §3.4 的 Release 命令；二进制不要进 git。

---

## 6. 排障速查

| 现象 | 原因 | 处理 |
|---|---|---|
| `cargo build` 报找不到 cmake | 本机禁止 brew 的自带沙箱 | 用 §5 的便携 cmake PATH（原因见 `MOD_WORKFLOW.md §2.1`） |
| 打包脚本说 `missing packaging/macos/Info.plist` | 被上游 `.gitignore` 忽略 | 已经是例外规则；确认文件在库里（§4.5） |
| 推 fork 被拒：`shallow update not allowed` | 克隆是 `--depth 1` | `git fetch --unshallow origin` |
| `gh api .../contents/...` 返回 404 | 默认分支上没有该路径 | 加 `?ref=<branch>` |
| 运行中打开第二张图报 "cannot open files in the Image format" | 上游 #33，已修 | 确认 `src/mac.rs` 的助手常驻与 openFile/openFiles 回调在（M12） |
| 双击 .app 不出新窗口 | reopen 注入失败 | 注入必须在 `applicationDidFinishLaunching:` 之后；日志看 `reopen handler installed` |
| 多选图打开后"没反应" | 子进程被 clap 拒绝（多参数） | `mac.rs` 的 `INPUT` 必须 `multiple_values(true)` |
| 第二个窗口改设置不生效 | 设置单写者（`flock`） | 设计如此，只有第一个窗口持久化；关掉其它窗口重启即接管 |
| 缩放不跟鼠标 / 平移沿斜轴走 | 几何模型用错 | 见 `MOD_WORKFLOW.md §5.2` 四条铁律；单测 `zooming_keeps_the_pixel_under_the_cursor` / `panning_is_a_pure_screen_translation` |
| 滚轮一格翻好几张 / 一格翻不动 | 行/像素量纲混淆 | `gestures.rs` 的 `classify_wheel`；参考 `MOD_CHANGELOG.md` M1 |

---

## 7. 文档地图与维护规则

| 文档 | 内容 | 什么时候**必须**更新 |
|---|---|---|
| `HANDOFF.md`（本文件） | 账号、远端、Git 规范、发布、排障、文档规则 | 换账号/改远端/改发布流程/新增排障条目 |
| `docs/INPUT_REMAP_PLAN.md` | 需求规格、映射表、几何不变量、风险与决策 | 改任何用户可见行为、加手势、改常量语义 |
| `docs/MOD_CHANGELOG.md` | 逐项改动（M1–M12）、上游对照、升级流程、版本历史 | 每次代码改动；新增一节 M<n> 并更新版本历史 |
| `docs/MOD_WORKFLOW.md` | 方法论、工具链、几何模型、macOS 输入栈、坑 | 工具链变化、发现新坑、流程调整 |
| `docs/IMPLEMENTATION_SCHEDULE.md` | 任务卡、硬门禁、阻塞、交付物、验收记录 | 新任务/新阻塞/交付物或版本号变化 |
| `docs/README.md` | 文档索引 + 当前状态头 | 新增/重命名文档、产出版本变化 |
| `README.md`（仓库根） | 面向访客的 mod 说明 + 输入映射表 | 映射表变化、新增/删除文件、差异链接变化 |
| `CHANGELOG.md`（仓库根） | 面向用户的一句话变更 | 每次发布 |

写作约定：

- 中文为主，代码/命令/标识符用英文；表格优先于段落。
- 命令一律可直接复制；路径用 `~/` 或 `$VAR`，不写个人绝对路径。
- 每条结论尽量带"证据"（命令、日志行、单测名），方便下一次核对。
- 改代码同时改文档算"完成"的一部分；只改代码不改文档视为未完成。

---

## 8. 待办与风险

**可选待办**

1. 在 fork 上发 Release（附 zip + patch），方便直接下载。
2. 上游若发新版（尤其 notan/winit 升级），按 `MOD_CHANGELOG.md §4` 走一遍并回归 9 项验收。
3. Windows/Linux 回归：`classify_wheel` 的量纲常量是按 macOS 实测定的，其它平台可能需要微调。
4. 手势可配置化：目前只有"滚轮灵敏度"进设置，其余是常量。

**风险 / 已知限制**

- 助手进程（无窗口、不占 Dock）在窗口存在期间常驻，约 30–50 MB；最后一个窗口关闭后自动退出。
- 多窗口 = 多进程，Dock 里会有多个图标。
- 旋转/镜像仅显示，切图即复位；编辑模式下禁用（叠加层按未变换坐标绘制）。
- 未做代码签名/公证：分发到别的机器首次打开可能需要 `xattr -dr com.apple.quarantine`。

---

## 9. 命令速查

```bash
# 环境
export PATH="$HOME/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"
cd ~/oculante

# 开发循环
cargo test --lib --bin oculante
cargo build --release
./scripts/make_macos_app.sh
open dist/Oculante.app

# GitHub
gh api user --jq .login                                   # 真实账号
gh api "repos/Zenobins/oculante-mod/compare/master...feat/input-remap" --jq '.files|length'
git push fork feat/input-remap
git fetch origin && git log --oneline master..origin/master

# 排障
RUST_LOG=debug ./target/release/oculante tests/moss.jpg
pgrep -fl "MacOS/oculante"        # 看助手与窗口进程（-c = 窗口）
otool -L dist/Oculante.app/Contents/MacOS/oculante | grep -c homebrew   # 应为 0
```
