# MOD WORKFLOW — 改造这个项目的方法、工具、坑与流程

> 目标读者：未来继续维护/扩展这个 fork 的人（包括"上下文清空后的自己"）。
> 配套：`docs/INPUT_REMAP_PLAN.md`（需求规格）、`docs/MOD_CHANGELOG.md`（改了什么）、`docs/IMPLEMENTATION_SCHEDULE.md`（执行台账）。

---

## 1. 心法（这几条决定了这个 mod 是否好维护）

1. **规格先于代码**：所有"手感参数"和"语义约定"先写进规格文档，再写代码。参数改了也先改文档。
2. **纯函数化**：凡是"事件 → 动作"的判断，都放进 `src/gestures.rs`，不依赖窗口/GPU。手感问题用 `cargo test` 验证，而不是"开窗口点点看"。
3. **单一真相**：绘制、命中检测、缩放、平移都从**同一个变换矩阵**派生；不允许"渲染用一套、点选用另一套"。
4. **数据驱动**：进度只写 `IMPLEMENTATION_SCHEDULE.md`，变更只写 `MOD_CHANGELOG.md`。任何新会话先读这三份文档，不依赖对话记忆。
5. **最小侵入**：不改解码/编辑/缓存；上游能合的改动尽量保持"附加提交"形态，方便 rebase。
6. **先审查再编译**：改完代码先过一遍"安全性/健壮性清单"（§8），再 build。

---

## 2. 环境

### 2.1 为什么本机不能用 brew 装 cmake

`turbojpeg-sys` / `libaom-sys` 的 build script 需要 `cmake`。本机执行 `brew install cmake` 会失败：

```
sandbox-exec: sandbox_apply: Operation not permitted
```

Homebrew 自己在解包 bottle 时会调 `sandbox-exec`，而当前执行环境禁止嵌套沙箱；`HOMEBREW_NO_SANDBOX=1` 也无效。

**解法**：把官方预编译包放到项目内，构建时临时加 PATH（不污染系统目录，删目录即回退）。

```bash
# 一次性准备（约 75MB 下载，解压后 240MB）
mkdir -p ../tools && cd ../tools
curl -fL --proxy http://127.0.0.1:10808 -o cmake.tar.gz \
  https://cmake.org/files/v3.31/cmake-3.31.6-macos-universal.tar.gz
tar xzf cmake.tar.gz && rm cmake.tar.gz
```

直连 cmake.org 在本网络下会中断，**走本机 10808 代理**才稳定（37s 下完）。

### 2.2 每次构建前

```bash
export PATH="$HOME/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"
cd ~/oculante
```

依赖已 `cargo fetch` 过；首次 `cargo build` 约 5 分钟（dev）/ 11 分钟（release，`lto = "fat"`），之后增量 3~4 分钟。

### 2.3 磁盘与产物

| 目录 | 大小 | 说明 |
|---|---|---|
| `oculante/target/` | 6~9 GB | 可 `cargo clean` 回收 |
| `../tools/cmake-...` | 240 MB | 可整目录删除 |
| `oculante/dist/` | ~40 MB | .app + zip（已被 .gitignore 忽略） |

---

## 3. 常用命令

```bash
# 类型检查（只编 lib + bin，不要用 --all-targets，见 §5.4）
cargo check --bin oculante

# 单元测试（手感/几何全靠它）
cargo test --lib
cargo test --lib gestures::          # 只跑手势与几何

# 调试启动（trace 里有滚轮原始 delta）
RUST_LOG=debug ./target/debug/oculante tests/moss.jpg
RUST_LOG=trace ./target/release/oculante tests/moss.jpg 2>&1 | grep "Mouse wheel"

# 发布构建 + 打包
cargo build --release
./scripts/make_macos_app.sh          # -> dist/Oculante.app + dist/Oculante-<ver>.zip
open dist/Oculante.app
```

---

## 4. 文档驱动流程（怎么接着往下做）

```
需求/想法
  │ ① 写进 docs/INPUT_REMAP_PLAN.md（含"用户可见的最终映射表"）
  ▼
任务卡（docs/IMPLEMENTATION_SCHEDULE.md，每张卡带"硬门禁"）
  │ ② 一次只做一张卡；门禁不过不许进下一张
  ▼
实现（先纯函数 + 单测，再接 GUI）
  │ ③ 单测绿 → 编译 → 冒烟 → 手工验收
  ▼
docs/MOD_CHANGELOG.md（记录"上游原实现 → 本地实现"）
  │ ④ 更新变更日志与决策记录
  ▼
commit（原子提交）+ 刷新 input-remap.patch
```

任务卡模板（复制即可用）：

```markdown
### T<n> 标题
- 目的：
- 改动文件：
- 硬门禁（可执行/可 grep）：cargo test --lib gestures:: && ...
- 回滚方式：
```

**为什么强调"硬门禁"**：这个 mod 90% 的坑是"看起来对、手感不对"。门禁必须是能跑的断言（单测 / grep / otool），不能是"我试了感觉可以"。

---

## 5. 代码组织

### 5.1 三层职责

| 层 | 文件 | 职责 | 依赖 |
|---|---|---|---|
| 纯逻辑 | `src/gestures.rs` | 滚轮分类/累积、修饰键优先级、视图变换矩阵与其逆、中键状态机 | 只有 `notan::math`（glam），无窗口无 GPU |
| 状态与几何助手 | `src/appstate.rs` | `view_pivot()`、`view_matrix()`、`unview_point()`、`image_dims_f()` | 引入 `Mat3` |
| 接线 | `src/main.rs` / `utils.rs` | 把事件映射到动作；绘制时用矩阵 | 全部 |

新增手势时，**先加 `gestures.rs` 的判定 + 单测**，再在 `main.rs` 里接一行。

### 5.2 几何模型（本 mod 的核心结论，务必先读）

`offset` 是**纯屏幕平移**；旋转/镜像绕**图像中心**（随 offset 移动）。由此推出四条铁律：

| 操作 | 公式 | 反例（曾经踩过） |
|---|---|---|
| 平移 | `offset += 屏幕位移` | 逆旋转位移 → 图片沿旋转后的轴走 |
| 缩放锚点 | `offset -= (cursor - offset) * Δ / scale`（cursor 用原始屏幕坐标） | 先把 cursor 逆变换 → 缩放完全不跟鼠标 |
| 命中检测 | `M⁻¹`（只含旋转/镜像）→ 再 `/scale` | 用完整 `M⁻¹`（含 scale）→ 双重变换 |
| 边界 | 屏幕空间限制图像中心 `pivot ∈ [W - aabb/2, aabb/2]` | 在 offset 空间按 AABB 夹 → 限制框随图旋转、边缘斜卡 |

绘制矩阵：`M = Mirror(pivot) * R(pivot) * T(offset) * S(scale)`，pivot = `offset + 尺寸*scale/2`。
大图瓦片：`M * T(瓦片像素偏移)`（所有瓦片共用一个 `M`）。

**验证方式**：`cargo test --lib gestures::` 里的
`panning_is_a_pure_screen_translation`、`zooming_keeps_the_pixel_under_the_cursor`、`matrix_and_untransform_are_inverse`、`aabb_matches_known_values`。

### 5.3 状态放哪里

- 需要参与持久化的 → `PersistentSettings`（如 `wheel_nav_sensitivity`）。
- 只在本会话有效的 → `OculanteState`（`view`、`view_path`、`wheel`、`middle`、`zoom_drag`、`last_wheel_at`…）。
- **绝对不要**把旋转/镜像写进 settings：它们是"临时视图"，切图即归零（`view_path != current_path` 的单点钩子）。

### 5.4 Rust 工程坑（都踩过）

| 坑 | 现象 | 解法 |
|---|---|---|
| `current_texture.get()` 是 `&mut self` | `state` 已被可变借用，再取 `state.image_dims_f()` 报 E0502 | 先把所有需要的数值算好，再进入 `if let Some(texture)` |
| `cargo check --all-targets` | 上游 `benches/my_benchmark.rs` 与当前 `open_image` 签名不一致，直接报错 | 用 `--bin oculante`；bench 的坏是上游遗留，别去改它 |
| `#[serde(default)]` 容器级属性 | 缺字段时用 `Default::default()` 填充（不是字段类型默认值） | 新字段一定要在 `Default` 里给合理值 |
| 枚举变体删除 | 旧 `settings.json` 反序列化整体失败 | 禁用功能只删调用点，保留变体 |
| release `lto = "fat"` | 每次改一行也要 3~11 分钟 | 开发用 `cargo check`/`cargo test`，只在交付时 release |
| `.gitignore` 没盖住产物 | `git add -A` 把 99KB patch 提交进仓库 | `.gitignore` 加 `input-remap.patch`、`dist/`、`target/` |
| `git rm --cached` 对"新暂存文件"报错 | 补丁文件反复被提交 | 用 `git restore --staged <file>` 或 `git rm --cached -f` |
| 未使用 import 警告 | 无关痛痒但会掩盖新警告 | 保留上游既有警告，别引入新的 |

---

## 6. macOS 输入栈知识（这个 mod 最核心的领域知识）

三层结构，任何一层都可能改变数值语义：

```
AppKit NSEvent
  └─ winit      : hasPreciseScrollingDeltas ? PixelDelta : LineDelta
       └─ notan : LineDelta ×50  |  PixelDelta ÷10   ← 量纲被改写，且有损
            └─ egui : ctrl/cmd + wheel 被转成 Event::Zoom   ← 另一个陷阱
```

关键事实（均已核对源码）：

1. **两种滚轮量纲**：行（±1~3，notan 后 = 50/150）与像素（每事件 0.5~1.0）。这是"滚一格要滚三次"的根因，任何单一阈值都救不了。
2. **`hasPreciseScrollingDeltas` 为 true 的不只是触控板**：带平滑滚动的鼠标（罗技 Options、Magic Mouse）也是像素流。
3. **Shift + 滚轮在 macOS 会换轴**：`delta_y = 0, delta_x ≠ 0`，所以旋转手势必须取主导轴。
4. **Ctrl/⌘ + 滚轮会被 egui 吞成 `Event::Zoom`**（`notan_egui/src/plugin.rs`）。项目已设 `zoom_with_keyboard = false`，所以它不会真的缩放 UI，可以安全地拿来做图片缩放。
5. **winit 的 LineDelta 在新版里会 `to_physical` 并反转 X**（`scrollingDeltaX * -1.0`）；升级 notan/winit 后要重新核对，否则水平方向和数值都会变。
6. **负行列式（镜像）不会被剔除**：notan 默认 `cull_mode = CullMode::None`。若上游改成剔除，镜像后的图会整张消失，这时要改为镜像 UV 而不是顶点。

调试手段：`RUST_LOG=trace` 下 `Event::MouseWheel` 会打印 `dx/dy` 原始值（`main.rs` 的 wheel 分支首行），这是判断"量纲/设备类型"最快的证据。

---

## 6.5 多实例（多窗口）架构

### 为什么是多进程

`notan::App` 与 `OculanteState` 都是**单窗口**模型（一个 `App` 一个 window，绘制/输入/egui 全挂在上面）。
要在同进程开第二个窗口，需要把 `state`、渲染目标、输入路由全部改成多实例，等于重写。
多进程方案的额外好处：**每个窗口的 always-on-top、缩放、当前图片、编辑栈天然隔离**，不需要任何同步代码。

### macOS 上的三个事实

1. **Finder 双击已运行的 .app 只会激活**（LaunchServices），不产生第二个进程。`open -n` 才强制新实例。
2. **直接 spawn `.app` 内的二进制**也能跑，但它不是通过 LaunchServices 启动的，Dock/菜单/Apple Event 行为不如 `open -n` 正规 → 所以在 bundle 内优先用 `open -n`。
3. **`src/mac.rs` 的 chainload**：无参数启动会先起一个 fruitbasket 的 NSApplication 等 `openFile:`，再把自己以 `-c` 拉起。
   因此"spawn 自己"时必须带 `-c`，否则新进程会再次进入等待循环。

```rust
fn new_instance_command() -> io::Result<Command> {
    let exe = std::env::current_exe()?;
    match bundle_root(&exe) {                 // 向上找 .app 目录
        Some(bundle) => { let mut c = Command::new("/usr/bin/open"); c.arg("-n").arg(bundle); Ok(c) }
        None => { let mut c = Command::new(exe); c.arg("-c"); Ok(c) }
    }
}
```

### 共享状态：设置文件是唯一的冲突点

其它资源都是进程私有（解码缓存、缩略图、编辑栈）；唯一共享的是 `~/Library/Application Support/oculante/` 下的两个 JSON。

| 问题 | 后果 | 解法 |
|---|---|---|
| 两个进程每 5 秒都写 `config.json` / `config_volatile.json` | 窗口几何、设置互相覆盖 | `flock(LOCK_EX\|LOCK_NB)` 选主：只有锁持有者写 |
| `File::create` + `to_writer_pretty` 不是原子写 | 另一个进程/崩溃可能读到半截 JSON，设置丢失 | 写临时文件 → `sync_all` → `rename` |
| 用 `create_new` 当锁文件 | 进程崩溃会留下死锁，应用再也存不了设置 | **必须用 `flock`**，内核在进程退出时自动释放 |

```rust
// settings.rs
pub struct InstanceLock { _file: File }          // 只要它活着，锁就在
pub fn try_acquire_in(dir: &Path) -> Option<Self>;
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()>;
```

调用侧：`state.may_persist()` 门控所有 `save_blocking()`；次级窗口在首帧提示只读。
单测：`settings::tests::only_one_instance_may_persist`、`atomic_write_leaves_no_temp_file`。

### Finder「打开方式」为什么曾经坏掉（上游 #33）

macOS 把"打开文件"作为 Apple Event 发给**正在运行的应用**，由 `NSApplicationDelegate` 的 `application:openFile:` 接收。
上游 `mac.rs` 的做法是"助手收到一个文件就 spawn 子进程并**退出**"，所以应用运行期间没人接事件 → 系统弹 "cannot open files in the Format"。

现在的结构：

```
open .app / 双击图片
   └─ LaunchServices
        └─ 助手进程（fruitbasket, Accessory, 无窗口, 常驻）
             ├─ application:openFile: → spawn 窗口进程（带文件路径）
             ├─ applicationShouldHandleReopen: → spawn 窗口进程（无文件）
             └─ 轮询子进程，全退出后自己退出
```

两个必须记住的坑：

1. **不能在 `FruitApp::new()` 之后立刻注入 delegate 方法**：那时 `NSApp.delegate` 还是 nil。要在 `applicationDidFinishLaunching:` 回调里注入。
2. **`rapp`（reopen）事件到不了 `NSAppleEventManager` 的 handler**：NSApplication 先消费了它，只走 delegate。所以 reopen 只能用 `class_addMethod` 补到 delegate 类上（type encoding `B@:@B`）。

窗口进程（带 `-c` 参数）**不安装**任何 delegate，也不进入助手逻辑。

补充三条实测经验：

- 单文件是 `application:openFile:`，**多文件是 `application:openFiles:`（复数）**，两个都要接。
- 多文件事件要在 250ms 内合并，再一次性传给一个窗口（路径作为多个位置参数），否则"全选打开"会起 N 个进程。
- `mac.rs` 自己的 clap 也要允许 `INPUT` 多值，否则带多个路径的子进程会在 clap 处**静默退出**（日志里只有一行 usage，看起来像"没反应"）。

### 新增窗口的接线

`InputEvent::NewWindow`（默认 ⌘/Ctrl+N）→ `spawn_new_instance()` → 成功给 toast、失败给错误 toast。
注意 `main.rs` 里已有 `use clap::Command`，spawn 时要用全限定名 `std::process::Command`，否则名字冲突。

---

## 7. 验证策略

### 7.1 三层验证，成本从低到高

| 层 | 手段 | 覆盖什么 |
|---|---|---|
| 几何/判定 | `cargo test --lib`（39 个） | 阈值、优先级、矩阵互逆、平移方向、缩放锚点、AABB、中键消歧 |
| 编译/启动 | `cargo check --bin oculante` + 冒烟启动 6 秒看日志 | 借用错误、运行时 panic、字体/GPU 初始化 |
| 手感 | 手工验收清单（`MOD_CHANGELOG.md §4.3`） | 灵敏度、方向、双击/拖拽冲突 |

**不许跳过第 1 层**：这个 mod 修过的两个最严重 bug（缩放不跟鼠标、平移沿斜轴走）都是靠不变量单测定位的，肉眼很难看清。

### 7.2 冒烟脚本（不弹窗太久）

```bash
(./target/release/oculante tests/moss.jpg > /tmp/smoke.log 2>&1 &)
sleep 6
pgrep -f "target/release/oculante" >/dev/null && echo running-ok
pkill -f "target/release/oculante"
grep -iE "panic|error" /tmp/smoke.log | head
```

### 7.3 打包验证

```bash
./scripts/make_macos_app.sh      # 脚本末尾会打印"是否仍链接 Homebrew 库"
open dist/Oculante.app --args "$(pwd)/tests/moss.jpg"
codesign -dv dist/Oculante.app   # 期望 flags=0x2(adhoc)
```

本 fork 构建时**不带** `heif`/`avif_native` 特性，所以二进制只依赖 `/usr/lib` 与 `/System`，`.app` 不需要打包 dylib，也不需要 `install_name_tool`（上游 `build_mac.sh` 那套复杂流程是为了 heif）。

---

## 8. 改完代码、编译之前的安全性/健壮性清单

逐条过，全部有对应实现或单测：

| # | 检查项 | 本 mod 的做法 |
|---|---|---|
| 1 | 除零 | `to_notches` 除常量；`nav_px.max(1.0)`；灵敏度非法值回落 1.0 |
| 2 | `clamp(min > max)` panic | `limit_offset` 用小图分支归一化；`cursor_relative` 加 `dimensions > 0` 守卫 |
| 3 | 单事件暴走 | 精确滚动单事件 clamp ±200px；翻一张后余量清零 |
| 4 | 事件丢失导致状态卡死 | `zoom_drag`/`middle` 每帧与 `app.mouse.is_down()` 对齐 |
| 5 | UI 与画布抢事件 | `pointer_over_ui`、`settings_enabled`、`edit_state.painting` 三重守卫；面板滚动不更新滚轮时间戳 |
| 6 | 坐标空间混用 | 只有四处空间：image px / 未旋转屏幕 / 最终屏幕；每个函数注明期望哪一种 |
| 7 | 状态跨图残留 | `view_path != current_path` 单点复位（覆盖 9 个翻图入口与动画） |
| 8 | 编辑态错位 | 进入编辑模式强制 upright；编辑态禁止翻图/旋转/镜像 |
| 9 | 数据安全 | 新增手势零写盘；无损旋转仍是上游 `[`/`]` 行为；Delete 走废纸篓 |
| 10 | 配置兼容 | 新字段有默认值；禁用功能保留枚举变体 |

---

## 9. 交付与版本管理

```bash
git switch -c feat/input-remap           # 从 master 开分支
# 原子提交：逻辑 / 接线 / 禁用功能 / 文档，分开提
git diff master..HEAD > input-remap.patch
```

- 分支始终是"master + 附加提交"，**不改写上游历史**，方便 rebase。
- `input-remap.patch` 不入库（`.gitignore`），每次改完刷新。
- `.app` 产物在 `dist/`，同样不入库。

---

## 10. 后续可扩展的方向（按性价比排序）

1. **手势可配置**：把 `gestures.rs` 里的常量与"哪个键做什么"提升为设置项（目前只有滚轮灵敏度可调）。需要给鼠标手势设计一个设置页区块。
2. **垂直翻转**：`ViewTransform` 加 `mirror_y`，矩阵再加一个反射；中键双击或某个修饰键触发。
3. **旋转读数常显**：现在只在 `Rotating` 时显示 `∠`，可以改成"角度 ≠ 0 时常显"。
4. **信息面板放大镜跟随旋转**：`draw_zoomed` 需要重写瓦片裁剪逻辑，成本较高、收益一般。
5. **Windows/Linux 校准**：滚轮量纲在某些驱动下也不同，`classify_wheel` 已有单测，可按平台加偏移。
6. **上游合并**：`oculante` 处于 maintenance mode（README 顶部已声明），若上游开始 `oculante-next` 重写，本 mod 的几何模型与文档可直接迁移。

---

## 12. 收尾与交接（2026-11-06 冻结）

当前状态：需求全部实现并通过用户验收，分支 `feat/input-remap` 停在 14 个附加提交。

| 项 | 值 |
|---|---|
| release 二进制 | `target/release/oculante`，30 MB，sha256 前缀 `0a2f6efb` |
| 应用包 | `dist/Oculante.app`，版本 `0.9.6-mod.20261106-0229` |
| 分享包 | `dist/Oculante-0.9.6-mod-20261106-0229.zip`（17 MB） |
| 补丁 | `input-remap.patch`（193 KB，含文档与打包脚本） |
| 单测 | `cargo test --lib --bin oculante` → 41 + 1 全绿 |

再次接手时的顺序：

1. `docs/README.md`（索引 + 当前状态）
2. `docs/MOD_CHANGELOG.md §4`（上游升级流程）；若只是小改，读 §2 对应条目
3. `docs/MOD_WORKFLOW.md §4`（文档驱动流程）与 §8（改完先审查再编译）
4. 环境：`export PATH=".../tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"`

**已知边界（不是 bug）**：

- 窗口 = 进程，多窗口时 Dock 里会有多个图标；`⌘N` 用于手动再开一个。
- 只有一个窗口写设置，其它窗口只读（启动时提示），避免互相覆盖。
- 旋转/镜像仅显示，切图即复位；编辑模式下禁用。
- 助手进程（无窗口、不占 Dock）在最后一个窗口关闭后约 3 秒退出。

## 11. 一页速查

```bash
export PATH="$HOME/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"
cd ~/oculante

cargo test --lib          # 39 项，必须全绿
cargo build --release     # 3~11 分钟
./scripts/make_macos_app.sh
open dist/Oculante.app
```

改动前的三条铁律：**offset 是屏幕平移**、**旋转/镜像绕图像中心**、**锚点用原始屏幕坐标**。
