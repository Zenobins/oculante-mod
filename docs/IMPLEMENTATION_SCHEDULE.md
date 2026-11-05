# 排程与执行台账（IMPLEMENTATION SCHEDULE）

> 配套规格：`docs/INPUT_REMAP_PLAN.md`（v5，唯一需求真相）
> 本文件是**唯一进度真相**：任务状态只在本表更新；任何新会话先读本表 → 再读规格 → 从第一个未完成任务继续。
> 纪律：**一次只做一张任务卡**；每张卡都有"硬门禁"（可执行命令/可 grep 断言），门禁不过不许进下一张。

---

## 0. 进度总表（只更新这里）

| ID | 任务 | 门禁（硬验证） | 状态 |
|---|---|---|---|
| T0 | 构建环境 cmake | `cmake --version` | ✅ |
| T1 | 分支 `feat/input-remap` | `git branch --show-current` | ✅ |
| T2 | `gestures.rs` 几何内核 + 单测 | `cargo test --lib gestures::` 全绿 | ✅ |
| T3 | `utils.rs` 统一入口（`zoom_at`/`toggle_always_on_top`/`view_matrix` 助手） | `cargo check --lib` | ✅ |
| T4 | 绘制层接入矩阵（`draw_textures`/棋盘/边框/瓦片） | 构建后画面可正常显示（未变换时零差异） | ✅ |
| T5 | 滚轮手势（翻图/缩放/90° 旋转） | 手动 3 项 + `cargo test` | ✅ |
| T6 | 右键拖拽缩放 + 右键双击置顶 | 手动 2 项 | ✅ |
| T7 | 左键双击 1:1 ↔ fit | 手动 1 项 | ✅ |
| T8 | 中键状态机（单击镜像 / 拖动精密旋转） | 手动 3 项 | ✅ |
| T9 | 几何一致性收尾（AABB/平移逆变换/`cursor_relative`/去 `trunc()`） | 手动 3 项 + 单测 | ✅ |
| T10 | 禁用 `ClearImage`（4 处） | `grep -rn "ClearImage" src/` 只剩枚举/过滤 | ✅ |
| T11 | 文档（README/CHANGELOG）+ patch 导出 | `git diff --stat` + patch 文件存在 | ✅ |
| T12 | `cargo build --release` + 验收清单 | `target/release/oculante` 可运行 | ✅ |
| T13 | macOS `.app` 打包脚本与产物 | `dist/Oculante.app` 可 `open` 启动 | ✅ |
| T14 | 升级用变更日志 + 方法论文档 | `docs/MOD_CHANGELOG.md`、`docs/MOD_WORKFLOW.md` | ✅ |
| T15 | 多实例（多窗口）+ 每窗口独立置顶 + 设置写锁 | 双进程各开一张图；第二实例日志出现只读警告；`settings::tests` 全绿 | ✅ |
| T16 | macOS 打开事件（运行中打开第二张图 / 再次双击 app） | 运行中开新图不再报错；再次双击 app 出新窗口 | ✅ |
| T17 | 多选打开合并（`application:openFiles:` + 250ms 批量） | 一次选 3 张 → 1 个窗口带 3 个路径 | ✅ |

状态图例：☐ 未开始 / ▶ 进行中 / ✅ 完成 / ⚠ 阻塞（必须在下方"阻塞记录"写原因）

---

## 1. 任务卡

### T0 构建环境

- 目的：`turbojpeg-sys` / `libaom-sys` 需要 cmake（当前 `cargo check` 已在 turbojpeg-sys 失败）
- 命令：`brew install cmake`
- 门禁：`cmake --version | head -1`
- 回滚：`brew uninstall cmake`
- 备注：磁盘增量约 100–200 MB

### T1 分支

- 命令：`git switch -c feat/input-remap`
- 门禁：`git branch --show-current` 输出 `feat/input-remap`
- 备注：上游为 `--depth 1` 克隆，无远端分支可切，直接建新分支

### T2 几何内核（最高优先级，先测后用）

- 新增 `src/gestures.rs`，`src/lib.rs` 注册 `pub mod gestures;`
- 内容（见规格 §3.1）：
  - `Mods` / `WheelAction{PrevImage,NextImage,Zoom(i32),RotateCw,RotateCcw}`
  - `Accumulator::steps(delta, threshold) -> i32`（保余量）
  - `WheelState::resolve(dx, dy, mods) -> Option<WheelAction>`（shift>ctrl>plain，单帧导航最多 1 步）
  - `ViewTransform{angle_deg, mirror_x}`：`add_angle`（归一化 + 吸附）、`quarter_steps`、`aabb`、`matrix`、`untransform_point`、`untransform_delta`、`is_upright`
  - 常量：`NAV_THRESHOLD=30`、`ROTATE_THRESHOLD=50`、`ROT_DRAG_GAIN=0.20`、`ROT_DRAG_GAIN_FINE=0.04`、`ROT_SNAP_DEG=1.5`、`CLICK_MAX_TRAVEL=3.0`、`SHIFT_AXIS_SIGN=1.0`、`ROT_DIR_SIGN=1.0`
- 单测（必须全绿）：
  1. 累积器：55 → 1 步且余 5；三次 20 → 第 3 次才 1 步
  2. 优先级：shift+ctrl+wheel 只出旋转；ctrl+wheel 出 Zoom；无修饰出翻图
  3. 导航限步：一次 delta=200 只返回 1 步
  4. 角度：`add_angle(12.3)` 保留 12.3；`+89` → 吸附 90；`-0.8` → 0；`quarter_steps` 在 12.3° 上得到 90
  5. AABB：0° →(w,h)；90° →(h,w)；45° →(√2/2(w+h), 同)
  6. **矩阵互逆**：angle∈{0,12.3,45,90,135,180,270} × mirror∈{false,true}，随机点 `untransform_point(matrix(p)) ≈ p`（1e-3）
  7. 位移逆变换：`untransform_delta` 与线性部分互逆（同样 14 组）
- 门禁：`cargo test --lib gestures::`

### T3 统一入口

- `utils.rs` 新增 `zoom_at`、`toggle_always_on_top`、`anchored()`（内部走 `untransform_point`）
- `set_zoom` 改为用 `anchored()`；`ZoomIn/ZoomOut` 改调 `zoom_at`（等价重构）
- 门禁：`cargo check --lib` 无 error；`grep -n "fn zoom_at" src/utils.rs`

### T4 绘制层矩阵

- `texture_wrapper.rs::draw_textures(&self, draw, m: Mat3, tile_px: (f32,f32))`，内部 `draw.image(tex).transform(m * T(tile_px))`
- `main.rs` 绘制段：`let m = state.view.matrix(scale, offset, pivot)`；pivot = 合成图中心 `offset + aabb?` → 用**未旋转**尺寸中心（旋转围绕图像中心）
- 棋盘 `pattern`、`show_frame` rect、tiling 循环全部复用 `m`
- 门禁：构建后打开任意图片，与改前像素级观感一致（变换为恒等时 `m` 应等价于 `T*S`）；`cargo test` 仍绿

### T5 滚轮

- 替换 `main.rs:576-617`：`state.wheel.resolve(...)` 分派
- 导航冷却 100ms + `pending_nav` 补发（在 `update()` 检查）
- 门禁：滚轮翻图（一格一张）、Ctrl+滚轮缩放、Shift+滚轮 90°

### T6 右键

- `MouseDown/Up(Right)` → `zoom_drag`；`update()` 内按 `drag_zoom_delta` 缩放；每帧用 `!is_down(Right)` 兜底
- egui 内 `button_double_clicked(Secondary)` → `toggle_always_on_top`
- 门禁：上拖放大/下拖缩小；双击置顶

### T7 左键双击

- 替换 `main.rs:1059-1067`：`is_actual_size` ? reset(fit) : `set_zoom(1.0)`
- 门禁：双击在 1:1 与自适应间切换；全屏仍可用 `F`

### T8 中键状态机

- `gestures::MiddleGesture{Idle,Pressed{start,max_travel},Rotating}`
- 按下→`Pressed`；每帧更新 `max_travel`，>3px → `Rotating`；`Rotating` 累加 `dx*gain`（Shift 精密档）
- 抬起：`Pressed` → 切镜像；`Rotating` → 结束
- 删除 `main.rs:693` 的 `|| app.mouse.is_down(Middle)` 平移分支
- 旋转读数：`Rotating` 时 egui 角落显示 `θ`（1s 节流）
- 门禁：单击翻转、再单击恢复、拖动只转不飘、Shift 精密档生效

### T9 几何一致性收尾

- `limit_offset` / reset fit 用 `view.aabb()`
- 平移（左键拖 + 键盘 Pan）用 `untransform_delta`
- `cursor_relative`（`main.rs:706`、`ui/mod.rs:576`）用 `untransform_point`
- 变换非恒等时不做 `offset.trunc()`（避免瓦片 1px 缝）
- 门禁：45° 下双击 fit 完整可见；旋转/镜像后平移方向正确；信息面板坐标正确

### T10 禁用 ClearImage

- `shortcuts.rs:133` 删默认绑定；`main.rs:505-507` 删调用；`ui/top_bar.rs:226-237` 删按钮；`settings_ui.rs` 过滤列表
- 门禁：`grep -rn "ClearImage" src/` 仅剩枚举定义与过滤处；Shift+Delete 无反应

### T11 文档与补丁

- README 增 "Mouse" 表；CHANGELOG 顶部加 `Unreleased`
- `git format-patch -1 --stdout > input-remap.patch`（或 `git diff master > input-remap.patch`）
- 门禁：`ls -l input-remap.patch`

### T12 构建与验收

- `cargo build --release`
- 逐条勾选规格 §7（含 `stat -f %m` 校验不写盘）
- 门禁：`target/release/oculante` 存在且可启动

---

## 2. 阻塞记录

| 时间 | 任务 | 现象 | 决策 |
|---|---|---|---|
| 2026-11-05 | T0 | `brew install cmake` 失败：Homebrew 内部的 `sandbox-exec` 在本环境被禁止（`sandbox_apply: Operation not permitted`），加 `HOMEBREW_NO_SANDBOX=1` 亦无效 | 改为下载 CMake 官方 macOS 预编译包到项目内 `tools/cmake-3.31.6-macos-universal/`（240MB），构建时临时加 PATH。**不污染系统目录**，删除 `tools/` 即可完全回退 |
| 2026-11-05 | T0 | 直连 cmake.org 下载被中断（速度 ~2MB/s 且超时） | 走本机 10808 端口代理下载（75MB，37s），见下方命令 |

构建命令（每次 shell 都需要）：

```bash
export PATH="$HOME/tools/cmake-3.31.6-macos-universal/CMake.app/Contents/bin:$PATH"
cd ~/oculante
cargo test --lib gestures::
cargo build --release
```

---

## 3. 变更记录

| 版本 | 内容 |
|---|---|
| v1 | 建立排程，拆 T0–T12，确定"先测后用"顺序（T2 先于所有接线） |
| v2 | 全部任务完成：16 个单测通过，release 二进制约 30MB，patch 已导出；T5–T9 的手动验收留给用户 |
| v3 | 用户反馈滚轮翻图需滚 3 格：`NAV_THRESHOLD` 30→10，新增 `wheel_nav_sensitivity` 设置（默认 1.0，设置页可调），导航冷却 100→70ms；新增 3 个单测（共 19 个） |

---

## 4. 交付物

| 项 | 路径 |
|---|---|
| release 二进制 | `oculante/target/release/oculante`（约 30 MB，sha256 前缀 `0a2f6efb`） |
| 补丁 | `oculante/input-remap.patch`（220 KB，含文档与打包脚本） |
| 分支 | `feat/input-remap`（19 个功能与修复提交 + 若干文档提交（实时数量见 compare 链接），基于 `master` @ fdc05eb） |
| 方案规格 | `oculante/docs/INPUT_REMAP_PLAN.md` |
| 本排程 | `oculante/docs/IMPLEMENTATION_SCHEDULE.md` |
| 便携 cmake | `应用mod/tools/cmake-3.31.6-macos-universal/`（240 MB，可整目录删除） |
| macOS 应用 | `oculante/dist/Oculante.app`、`oculante/dist/Oculante-0.9.6-mod-20261106-0229.zip` |
| 打包脚本 | `oculante/scripts/make_macos_app.sh` + `oculante/packaging/macos/Info.plist` |
| 升级变更日志 | `oculante/docs/MOD_CHANGELOG.md` |
| 方法论 | `oculante/docs/MOD_WORKFLOW.md` |

本地运行：

```bash
cd ~/oculante
./target/release/oculante tests/moss.jpg
```

---

## 5. 验收记录

| 日期 | 验收人 | 结论 |
|---|---|---|
| 2026-11-06 | 用户 | **全部通过**：滚轮翻图、Ctrl/⌘+滚轮缩放（锚点跟鼠标）、菜单 1:1 以图像中心、左键双击 1:1↔fit、右键拖拽缩放、右键双击置顶、中键单击镜像、中键拖拽精密旋转、Shift+滚轮 90°、`[`/`]` 无损旋转、Shift+Delete 失效、多窗口独立置顶、运行中打开第二张图、再次双击 .app 开新窗口、多选打开合并 |

验收后状态：**冻结**。后续只在上游更新、或用户提出新需求时改动；改动前先读 `docs/MOD_CHANGELOG.md §4` 的升级流程。
