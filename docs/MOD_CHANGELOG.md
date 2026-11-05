# MOD CHANGELOG — 本 fork 相对上游的全部改动

> 用途：**上游更新时，用这份文档做差异比对和升级**。
> 基线：`woelper/oculante` @ `fdc05eb`（`master`，v0.9.6）
> 本地分支：`feat/input-remap`（4+ 个提交，全部是附加提交，未修改上游历史）
> 补丁文件：`input-remap.patch`（`git diff master..HEAD`）
> 设计规格：`docs/INPUT_REMAP_PLAN.md`｜执行台账：`docs/IMPLEMENTATION_SCHEDULE.md`
> 方法论：`docs/MOD_WORKFLOW.md`

---

## 0. 一句话总结

只改**输入映射**，外加禁用 `ClearImage`；为支持"仅显示的旋转/镜像"，新增了一个**纯逻辑手势模块**和**绘制期矩阵变换**。没有改任何解码、编辑、文件写入逻辑。

---

## 1. 变更总览（按文件）

| 文件 | 类型 | 改动规模 | 作用 |
|---|---|---|---|
| `src/gestures.rs` | **新增** | ~800 行（含 30 单测） | 滚轮/手势/视图变换的纯逻辑，无 GUI 依赖 |
| `src/lib.rs` | 修改 | +1 行 | 注册 `pub mod gestures;` |
| `src/appstate.rs` | 修改 | +110 行 | 视图/手势状态字段、几何助手、`instance_lock` 与 `may_persist()` |
| `src/utils.rs` | 修改 | +40 行 | `zoom_at` / `toggle_always_on_top` / `reset_view_transform` |
| `src/main.rs` | 修改 | 约 250 行（替换为主） | 事件循环、绘制段、双击、边界限制、导航节流 |
| `src/texture_wrapper.rs` | 修改 | ~30 行 | `draw_textures` 改为接收完整变换矩阵 |
| `src/shortcuts.rs` | 修改 | +2/-1 行 | 移除 `ClearImage` 默认键；新增 `NewWindow`（⌘/Ctrl+N） |
| `src/ui/top_bar.rs` | 修改 | -14 行 | 移除 Clear Image 按钮；菜单 1:1 锚点；置顶走 helper |
| `src/ui/settings_ui.rs` | 修改 | +8 行 | 过滤 ClearImage 键位行；新增滚轮灵敏度滑杆 |
| `src/settings.rs` | 修改 | +95 行 | 新设置项；`InstanceLock`（多实例写锁）；`write_atomic` 原子写 |
| `README.md` / `CHANGELOG.md` | 修改 | +45 行 | 用户可见的映射表与变更记录 |
| `packaging/macos/Info.plist` | **新增** | 50 行 | .app 的 plist 模板 |
| `scripts/make_macos_app.sh` | **新增** | 80 行 | 一键出 .app / .zip |
| `docs/*` | **新增** | 4 个文件 | 规格、台账、本变更日志、方法论 |
| `.gitignore` | 修改 | +4 行 | 忽略 `input-remap.patch`、`dist/`、`target/` |
| `Cargo.toml` | 修改 | +4 行 | unix 平台新增 `libc`（实例锁用） |

---

## 2. 逐项变更（含"上游原实现 → 本地实现"对照）

### M1 滚轮：上一张 / 下一张

**上游**：滚轮 = 缩放；`Ctrl + 滚轮` = 翻图（`main.rs` `Event::MouseWheel`，直接按 `delta_y > 0` 判断）。
**本地**：滚轮 = 翻图；`Ctrl + 滚轮` = 缩放（两者对调）。

关键难点：**macOS 的滚轮有两种量纲**（已核对 winit / notan 源码）：

| 设备 | `NSEvent.hasPreciseScrollingDeltas` | winit 事件 | notan 变换 | 一格的实际数值 |
|---|---|---|---|---|
| 经典鼠标滚轮 | false（行） | `LineDelta(±1..3)` | **×50** | 50 / 150 |
| 触控板、平滑滚动鼠标（罗技/Magic Mouse） | true（像素） | `PixelDelta(px)` | **÷10** | 摊成多事件，每事件 0.5~1.0 |

因此**单一阈值不可能同时适配**（本 mod 最初踩过：阈值 30 时平滑鼠标要滚 3 格才翻一张）。

实现（`gestures.rs`）：

```rust
pub const WHEEL_NOTCH: f32 = 50.0;   // notan 对 LineDelta 的放大倍数
pub const LINE_MIN: f32 = 45.0;      // 低于此值不可能是行滚动
pub const PX_PER_IMAGE: f32 = 30.0;  // 精确滚动：每 30px 翻一张
pub const GESTURE_GAP_MS: f32 = 250.0;

pub enum WheelInput { Line(f32), Pixel(f32) }
pub fn classify_wheel(dy: f32) -> WheelInput; // 行值必为 50 的整数倍
```

- **行滚动**：一格 = 一张，不累积、不缩放（防"一次滑 3 格跳 3 张"）。
- **精确滚动**：累积像素，达到 `PX_PER_IMAGE / 灵敏度` 翻一张；**翻一张后余量清零**（防一次 300px 的大事件连翻 10 张）；单事件 clamp ±200px。
- **手势分段**：距上一次滚轮事件 > 250ms 视为新手势，丢弃余量（否则要先把"欠账"滚掉才动）。
- 导航附加 70ms 冷却 + `pending_nav` 补偿（丢掉的那次在冷却结束后补发）。
- 灵敏度设置：`PersistentSettings::wheel_nav_sensitivity`（默认 1.0，范围 0.2–10，设置页可调）。

单测：`classifies_line_and_pixel_deltas`、`one_line_notch_is_one_image`、`precise_scrolling_switches_after_enough_pixels`、`a_pause_starts_a_new_gesture`、`a_huge_event_switches_only_one_image`、`sensitivity_divides_the_pixel_threshold`、`notch_conversion_treats_40px_as_one_notch`。

---

### M2 缩放：`Ctrl/⌘ + 滚轮`、右键拖拽、`=`/`-`、`1`~`5`、双击 1:1

**上游**：`main.rs` 里滚轮缩放直接改 `offset/scale`；`utils::set_zoom` 用光标作锚点。
**本地**：全部收敛到 `utils::zoom_at(delta, anchor, state)` 与 `utils::set_zoom`，锚点语义统一。

**几何结论（重要，曾推导错误一次）**：`offset` 是**纯屏幕平移**，旋转/镜像都发生在"跟着 offset 一起移动的图像中心"上，因此：

```
以光标为锚点缩放：offset -= (cursor - offset) * Δ / scale     // cursor 用原始屏幕坐标
```

旋转项会自然抵消，**不需要也不能**把锚点先逆旋转（逆了就会"缩放不跟鼠标"，这是本 mod 修过的 bug）。

菜单 `View 1:1` 与双击的差别（用户要求）：

| 入口 | 锚点 |
|---|---|
| 顶栏菜单 `View 1:1` | **图像中心** `state.view_pivot()`（用菜单时鼠标不在图上） |
| 左键双击 1:1 ↔ fit | **光标** |
| 键盘 `1`~`5` | 光标 |

单测：`zooming_keeps_the_pixel_under_the_cursor`（4 角度 × 2 镜像，断言光标下像素漂移 < 0.01px）。

---

### M3 左键双击：1:1 ↔ Reset view（原为全屏）

**上游**：`main.rs` egui 闭包里 `button_double_clicked(Primary)` → `toggle_fullscreen`。
**本地**：同样位置改为 `is_actual_size(scale) ? reset_image=true : set_zoom(1.0, None)`。

- 全屏保留：`F` 键 + 顶栏按钮，仅"双击"这一个入口被替换。
- 回 fit 时同时清空旋转/镜像（`reset_view_transform`）。
- 守卫：`!pointer_over_ui && !mouse_grab && !edit_state.painting`。

---

### M4 右键：按住上下拖 = 缩放；双击 = Always on top

**上游**：右键完全未使用（全仓库无 `MouseButton::Right` 业务逻辑）。
**本地**：

- `MouseDown(Right)` → `zoom_drag = true`；`update()` 内按 `mouse_delta.y` 调 `zoom_at`（上移放大）；每帧用 `!app.mouse.is_down(Right)` 兜底清除（防丢失 `MouseUp`）。
- `zoom_drag_moved`（位移 > 1px）标记：右键双击判定时要求它为 false，避免"拖过一下再双击"误触发置顶。
- `button_double_clicked(Secondary)` → `toggle_always_on_top`。
- 与中键旋转互斥：进入 `Rotating` 时清 `zoom_drag`。

---

### M5 中键：单击 = 水平镜像；按住拖 = 精密旋转（原为平移）

**上游**：中键按住 = 平移（`main.rs` update 里 `app.mouse.is_down(Middle)` → `offset += mouse_delta`）。
**本地**：该平移分支**已删除**，改为状态机 `gestures::MiddleGesture`：

```
Idle → Pressed{start, max_travel} → (max_travel > 3px) → Rotating
MouseUp: 仍 Pressed → 切换镜像；Rotating → 结束（不镜像）
```

- 用**按下期间最大位移**判定（拖出去再拖回不算单击）。
- 旋转：`angle += dx * gain`，`gain = 0.20°/px`，按住 Shift → `0.04°/px`（精密档），角度实时显示在画布左上角。
- 旋转角度**连续**，并吸附 90° 倍数（±1.5°），所以"拖到 0.8°"会自动回正。
- 镜像与旋转都是**仅显示**，切图/`V`/双击回 fit 时归零。
- 编辑模式下禁用（编叠加层在未变换坐标系绘制）。

单测：`middle_click_and_drag_are_told_apart`、`rotate_gain_has_a_fine_mode`、`angles_are_normalized_and_snapped`、`quarter_steps_quantize_before_stepping`、`aabb_matches_known_values`。

---

### M6 Shift + 滚轮 = 90° 步进旋转（仅显示）

**上游**：无此手势（`[`/`]` 是无损旋转，写盘）。
**本地**：`view = view.quarter_steps(±1)`：**先把当前角度量化到最近 90°，再 ±90°**，避免 12.3° + 90° = 102.3°。
macOS 上 Shift 会**把滚轮换成横向轴**（AppKit 行为），所以取 `dx/dy` 的主导轴（`dominant_delta`），符号由 `SHIFT_AXIS_SIGN` 常量兜底。
`[`/`]` 键**保持上游语义**（无损写盘旋转），未做改动。

---

### M7 视图变换的绘制实现

**上游**：`texture_wrapper::draw_textures(draw, tx, ty, scale)` 内部 `.scale().translate()`；棋盘、边框各自 `.scale().translate()`。
**本地**：`draw_textures(draw, transform: Mat3, repeat_px: (f32,f32))`，所有瓦片用同一矩阵：

```
M = Mirror(pivot) * R(pivot) * T(offset) * S(scale)
```

- `pivot = offset + 图像尺寸*scale/2`（图像中心，含 tiling 倍数）。
- 大图瓦片附加 `M * T(瓦片在图像内的像素偏移)`，拼接不错位。
- 棋盘 `pattern`、`show_frame` 边框、tiling 重复全部复用 `M`。
- 非恒等变换时**不再对 offset 做 `trunc()`**（否则非 90° 旋转会出现 1px 接缝）。
- 负行列式（镜像）不会触发背面剔除，因为 notan 默认 `cull_mode: CullMode::None`（已核对 `notan_graphics/src/pipeline.rs`）。

单测：`matrix_and_untransform_are_inverse`（8 组角度×镜像）、`image_centre_is_the_fixed_point`、`mirror_flips_horizontally_around_the_centre`、`view_space_matrix_ignores_scale_and_offset`。

---

### M8 平移与边界（必须与 M7 一起理解）

**上游**：`offset += mouse_delta`；`limit_offset` 按未旋转尺寸直接夹取 offset。
**本地**：

- 平移仍是 `offset += 屏幕位移`（**原样**）。曾错误地逆旋转位移，导致"图片沿旋转后的轴走"（用户反馈的 bug）。
- `limit_offset` 改为在**屏幕空间限制图像中心**：
  `pivot ∈ [W - aabb/2, aabb/2]`，再 `offset = pivot - 尺寸/2`。
  这样限制框始终与窗口对齐，转动任意角度后拖到边缘也不会斜着卡住；小图（小于窗口）时保持完全可见。
- 平移/缩放/命中检测三者的坐标空间关系写进了 `docs/INPUT_REMAP_PLAN.md §3.2b`。

单测：`panning_is_a_pure_screen_translation`（8 组）。

---

### M9 禁用 `ClearImage`（唯一的功能删除）

**上游**：`Shift+Delete` / 顶栏按钮 → 把图片从浏览列表移除（不删盘）。
**本地**：四处入口全部移除：

| 位置 | 改动 |
|---|---|
| `shortcuts.rs` | 删除 `add_keys(ClearImage, &["LShift","Delete"])` |
| `main.rs` | 删除 `key_pressed(...ClearImage)` 调用块 |
| `ui/top_bar.rs` | 删除 "Clear image" 按钮 |
| `ui/settings_ui.rs` | 键位列表过滤掉该行 |

**保留**：`InputEvent::ClearImage` 枚举变体（否则旧 `settings.json` 反序列化会整体失败）、`utils::clear_image()`（`delete_file` 内部仍需用它清视图）。
`Delete` 仍是 `trash::delete` + 确认弹窗，未改动。

---

### M12 修复 macOS「打开第二张图报错」与「再次双击 App 不开新窗口」

**现象**（用户验收）：
1. 已有一个窗口时，在 Finder 里双击第二张图 → 系统弹 “The document … could not be opened. Oculante cannot open files in the "Image" format.”；关掉窗口后再打开则正常。
2. 不打开图片、只双击 .app，永远只有一个窗口。

**根因**（已核对上游 issue）：这是上游的已知缺陷 `woelper/oculante#33`，维护者原话是“fruitbasket 必须跑在主线程，没法常驻监听 macOS 的打开事件，大概要深入 objc”。具体机制：

- 上游 `mac.rs` 的 chainload：无参启动 → 起一个 fruitbasket 的 NSApplication 收 `openFile:` → 收到文件后 **spawn 子进程并立即退出**。
- 于是"应用已运行"时，系统把 odoc 事件发给正在运行的子进程（winit），**它根本没有 delegate 处理**，AppKit 就弹出上面那个错误。
- 同理，双击 .app 触发的是 `rapp`（reopen）事件，NSApplication 会调用 delegate 的 `applicationShouldHandleReopen:hasVisibleWindows:`，fruitbasket 没提供该方法 → 只会激活旧窗口。

**修复**：

| 改动 | 说明 |
|---|---|
| 助手常驻 | `mac.rs` 的 fruitbasket 进程不再"收一个文件就退出"，而是作为**无窗口助手**常驻（`ActivationPolicy::Accessory`，不显示 Dock 图标） |
| 每个事件开一个窗口 | `application:openFile:` → `spawn_window(Some(path))`；冷启动无文件时，延迟 400ms 后开一个空窗口（若期间收到文件则取消） |
| reopen 注入 | fruitbasket 只注册固定几个 selector，因此用 `objc` 的 `class_addMethod` 往 **delegate 的类**上补 `applicationShouldHandleReopen:hasVisibleWindows:`（编码 `B@:@B`），返回 true 并开新窗口 |
| 注入时机 | 必须在 **`applicationDidFinishLaunching:` 之后**注入：在此之前 `NSApp.delegate` 还是 nil（这个坑实测踩到，日志 `No application delegate`） |
| 助手生命周期 | 每 1.5s 轮询自己 spawn 的子进程；全部退出后再等一个周期即 `stopper.stop()`，不留僵尸进程；15s 内没成功开窗也会退出 |
| 依赖 | macOS 目标新增 `objc = "0.2"`（已在校验锁里，无新下载） |

**实测矩阵**（用 `pgrep -fl` 看命令行区分助手与窗口）：

| 场景 | 结果 |
|---|---|
| 冷启动、无文件 | 1 助手 + 1 窗口 |
| 冷启动、带文件 | 1 助手 + 1 个带文件参数的窗口（**不会**多开空窗口） |
| 运行中打开另一个文件 | 新增 1 个窗口，无错误弹窗 |
| 运行中再双击 .app | 新增 1 个窗口（日志 `Reopen requested`） |
| 关掉所有窗口 | 助手在 ~3s 内退出，无残留进程 |

**多选打开（Open With 选一堆图）**：

- AppKit 对"一次打开多个文档"调用的是 **`application:openFiles:`（复数）**，不是逐个 `openFile:`；fruitbasket 也没提供它 → 同样用 `class_addMethod` 注入，签名编码 `v@:@@`，从 `NSArray<NSString*>` 里取路径。
- 事件会集中 250ms（`BATCH_WINDOW`）后再开窗口：**一个窗口带上全部文件路径**（app 的 `INPUT` 本来就支持多个值，会一起进 scrubber），避免"选 50 张图 → 起 50 个进程"。
- 追加一个隐藏坑：`src/mac.rs` 自己的 clap 里 `INPUT` 原本没有 `multiple_values`，导致"多参数子进程"在**日志里只留一行 clap 报错就静默退出**（表现为"没反应"）。已补上。

**安全边界**：`unsafe` 只集中在 `install_delegate_handlers`；delegate 为空时只 warn 不崩；`class_addMethod` 返回 false（方法已存在）时直接返回；两处 ObjC 回调都用 `catch_unwind` 包住，绝不让 panic 穿过 FFI；闭包只捕获 `Arc<Mutex<..>>`，在 AppKit 主线程调用。

---

### M11 多窗口（多实例）+ 每窗口独立的 Always on top

**需求**：同时开多个窗口看不同的图，且置顶状态按窗口独立（有的置顶、有的不置顶）。

**分析结论**：

- 上游（以及 notan）是**单窗口架构**：`notan::App` 只持有一个 window，同进程多窗口需要重写整个架构。**多进程**是唯一现实路径，而且天然满足"每窗口独立置顶"（每个进程操作自己的 NSWindow）。
- "无法多开"的真正原因不在 Oculante 本身：**Finder 双击一个已在运行的 .app 只会激活它，不会启动第二个实例**（LaunchServices 行为）。`open -n` 可以强制新实例，普通用户不会这么做。
- macOS 上还有 `src/mac.rs` 的 chainload 舞蹈：无参数启动时先跑一个 fruitbasket 的 NSApplication 收 `openFile:`，再把自己以 `-c` 重新拉起并退出。这条路径**不阻止**多实例，但让"新开一个空窗口"没有入口。

**实现**：

| 位置 | 改动 |
|---|---|
| `shortcuts.rs` | 新增 `InputEvent::NewWindow`，默认 `LControl+N`（macOS 自动映射为 ⌘N） |
| `main.rs` | `spawn_new_instance()`：在 `.app` 内用 `/usr/bin/open -n <bundle>`（新实例有自己的 Dock 项与 Apple Event 处理）；裸二进制则 spawn 自己并传 `-c` 跳过 chainload |
| 全部 `save_blocking()` 调用点 | 加 `state.may_persist()` 门控（快捷键退出 / `Event::Exit` / 每 5 秒自动保存） |
| `settings.rs` | 新增 `InstanceLock`（`flock(LOCK_EX\|LOCK_NB)`，进程退出由内核自动释放，不会留死锁）；`write_atomic()` 用"临时文件 + rename"替换 `File::create` |
| `appstate.rs` | 新增 `instance_lock: Option<InstanceLock>` 与 `may_persist()` |
| `Cargo.toml` | unix 平台新增 `libc`（已在校验锁里，无新下载） |

**为什么必须动设置持久化**：两个实例每 5 秒都会写 `config.json` / `config_volatile.json`。
(1) 会互相覆盖窗口几何、设置；
(2) 原来的 `File::create + to_writer_pretty` **不是原子写**，另一个进程可能读到半截 JSON → 设置直接丢失。
现在的规则：**第一个窗口持有写锁并持久化，其余窗口只读**（启动时提示 "Settings are read only"）。次级窗口内的设置改动不会落盘——这是有意的取舍，换取不会互相覆盖。

**已知取舍**：主窗口关闭后，其余窗口不会接管写权限（重启后自然恢复）。

---

### M10 打包 / 文档（非功能）

- `packaging/macos/Info.plist` + `scripts/make_macos_app.sh`：生成自包含 `dist/Oculante.app`（binary 无 Homebrew 依赖，ad-hoc 签名）与 zip。
- `README.md` 增加 "Mouse & keyboard (local fork)" 表。
- `CHANGELOG.md` 顶部增加 `Unreleased (local fork: input remap)`。
- `docs/`：`INPUT_REMAP_PLAN.md`（规格 v5）、`IMPLEMENTATION_SCHEDULE.md`（台账）、`MOD_WORKFLOW.md`（方法论）、本文件。

---

## 3. 常量速查（调手感只改这些）

| 常量 | 值 | 位置 | 含义 |
|---|---|---|---|
| `WHEEL_NOTCH` | 50 | gestures.rs | notan 对 `LineDelta` 的放大倍数（上游若改需同步） |
| `LINE_MIN` | 45 | gestures.rs | 行/像素判定阈值 |
| `PX_PER_IMAGE` | 30 | gestures.rs | 精确滚轮每 30px 翻一张 |
| `GESTURE_GAP_MS` | 250 | gestures.rs | 手势分段 |
| `PX_PER_NOTCH` | 40 | gestures.rs | 精确滚轮折算"一格"（缩放/旋转用） |
| `DRAG_ZOOM_SPEED` | 0.004 | gestures.rs | 右键拖拽缩放灵敏度 |
| `ROT_DRAG_GAIN` / `_FINE` | 0.20 / 0.04 | gestures.rs | 中键旋转 度/像素 |
| `ROT_SNAP_DEG` | 1.5 | gestures.rs | 90° 吸附容差 |
| `CLICK_MAX_TRAVEL` | 3.0 | gestures.rs | 中键单击/拖拽消歧 |
| `SHIFT_AXIS_SIGN` | 1.0 | gestures.rs | macOS Shift 换轴方向（反了就改 -1） |
| `ROT_DIR_SIGN` | 1.0 | gestures.rs | 旋转方向（反了就改 -1） |
| `ZOOM_MIN` / `ZOOM_MAX` | 0.01 / 40 | gestures.rs | 缩放边界 |
| `NAV_THROTTLE_MS` | 70 | main.rs | 翻图冷却 |

---

## 4. 上游升级流程（可复用 checklist）

### 4.1 判断上游"改了什么"

```bash
cd oculante
git fetch origin
git log --oneline master..origin/master            # 上游新增提交
git diff master origin/master --stat               # 改动范围
git diff master origin/master -- src/main.rs src/shortcuts.rs src/texture_wrapper.rs src/ui/
```

按下面这张表判断"是否与我们的改动重叠"：

| 上游若有改动 | 冲突风险 | 处理 |
|---|---|---|
| `Cargo.toml` 里 `notan` 版本 | **高** | 新版 notan 可能改变 `LineDelta*50` / `PixelDelta/10`、`DrawTransform`、`Event::MouseWheel`；先核对 `notan_winit/src/mouse.rs` 与 `notan_draw/src/transform.rs`，必要时更新 `WHEEL_NOTCH`、`draw_textures` |
| `main.rs` 的事件循环 / 绘制段 / `update()` | **高** | 我们的改动集中在这三块，通常会文本冲突；以"上游结构 + 我们的语义"合并 |
| `shortcuts.rs`（`InputEvent`/默认键） | 中 | 保留我们的"无 ClearImage 默认键"，新增上游事件照旧加 |
| `ui/top_bar.rs` / `settings_ui.rs` | 中 | 我们的按钮删除/键位过滤可能被上游重写，重做这 3 处小改动 |
| `settings.rs`（`PersistentSettings`） | 中 | 我们的 `wheel_nav_sensitivity` 必须保留；上游新增字段用 `#[serde(default)]` 已由容器级属性兜住 |
| `texture_wrapper.rs` 绘制 | 中 | 若上游重构 `draw_textures` 签名，需要把矩阵版本重新套上 |
| 解码/编辑/缓存逻辑 | 低 | 我们完全没动，直接接受上游 |

### 4.2 合并动作

```bash
git switch feat/input-remap
git rebase origin/master          # 或 merge，二选一，保持"附加提交"形态
# 逐个冲突按上表处理
cargo test --lib                  # 必须全绿（尤其 gestures:: 与 utils::scroll_tests）
cargo build --release
./scripts/make_macos_app.sh
```

### 4.3 升级后必须重跑的手工验收（改动语义能否被破坏的检查点）

1. 滚轮：经典鼠标一格一张；平滑鼠标约一格一张；触控板按 30px 一张。
2. Ctrl/⌘+滚轮：光标下像素钉住不动（旋转 23° 后再试一次）。
3. 菜单 `View 1:1` = 以图像中心缩放；左键双击 = 以光标缩放。
4. 右键上下拖 = 缩放；右键双击 = 置顶（拖过一下再双击不应误触发）。
5. 中键单击 = 镜像；中键拖 = 旋转（`∠` 读数变化），Shift 变精密档。
6. Shift+滚轮 = 90° 步进；`[`/`]` 仍是无损写盘旋转。
7. 旋转/镜像后：平移跟手、边界不斜卡、信息面板坐标正确、切图后复位。
8. `Shift+Delete` 无反应；`Delete` 进废纸篓且弹确认。
9. `F` 仍全屏；`←/→` 仍翻图。

### 4.4 若决定重做而不是 rebase

`input-remap.patch` 覆盖全部改动（含文档）。重做时从本文档第 2 节的 M1–M8 逐条套用最快，因为每条都标明了"上游原实现"与"本地实现"的对应函数。

---

## 5. 版本历史

| 日期 | 版本 | 内容 |
|---|---|---|
| 2026-11-05 | v1 | 首次改造：滚轮翻图 / Ctrl+滚轮缩放 / 右键缩放与置顶 / 双击 1:1 / 中键镜像 / Shift+滚轮与中键旋转 / 禁用 ClearImage |
| 2026-11-05 | v2 | 修复滚轮灵敏度（阈值 30→10，新增灵敏度设置）——当时仍是"单阈值"模型，未彻底解决 |
| 2026-11-06 | v3 | **滚轮模型重写**：行/像素分类 + 手势分段 + 单事件限流；**缩放锚点修正**（原始屏幕坐标）；**平移改回纯屏幕位移**；边界改为屏幕空间限制；新增 8 组不变量单测；打包 .app |
