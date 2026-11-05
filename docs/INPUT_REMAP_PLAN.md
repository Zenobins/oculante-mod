# Oculante 输入操作重构 — 方案文档（v2）

- 仓库：`woelper/oculante` → 本地 `/Users/raymond/WorkBuddy/应用mod/oculante`
- 基线：`master` @ `fdc05eb`，v0.9.6，Rust 2021，notan 0.14 / egui 0.31
- 状态：**待最终确认后开工**
- v5 变更：旋转由"离散 90°"升级为**连续角度**；中键拖动由平移改为**精密旋转**，中键单击仍为镜像；新增 AABB 几何与吸附规则

---

## 1. 项目分析

### 1.1 输入相关代码地图

| 职责 | 位置 |
|---|---|
| 原始事件入口 | `main.rs::process_events`（`KeyDown/KeyUp/MouseWheel/MouseDown/MouseUp/Drop`） |
| 帧循环 / 拖拽 / 双击 / reset view | `main.rs::update`（鼠标增量 691、拖拽平移 693、双击 1059-1067、reset 1154-1178、绘制 1184-1291） |
| 键位表 | `shortcuts.rs`（`InputEvent` 枚举 + `BTreeMap<InputEvent, BTreeSet<String>>`，持久化 JSON） |
| 键位设置页 | `ui/settings_ui.rs::keybinding_ui`（711-743） |
| 视图/文件操作 | `utils.rs`（`next_image/prev_image/set_zoom/zoomratio/delete_file/clear_image/toggle_fullscreen`） |
| 纹理绘制 | `texture_wrapper.rs::draw_textures`（692-716）/ `draw_zoomed`（718-828） |

### 1.2 改前鼠标映射

| 手势 | 现行为 | 位置 |
|---|---|---|
| 滚轮 | 缩放（光标锚点） | `main.rs:594-614` |
| **Ctrl+滚轮** | **上一张/下一张** | `main.rs:585-592` |
| 左键拖拽 | 平移 | `636-650` + `693` |
| 中键拖拽 | 平移（无视 UI 遮挡） | `693` |
| 左键双击 | 全屏 | `1059-1067` |
| 右键 | **完全未占用** | — |
| ← / → | 上一张/下一张 | `shortcuts.rs:118,121` |
| `[` / `]` | 无损旋转（写盘，仅 JPEG） | `shortcuts.rs:129-130`、`main.rs:432-462` |
| Shift+Delete | `ClearImage`（仅移除列表条目） | `shortcuts.rs:133`、`main.rs:505` |
| Delete | `DeleteFile` → `trash` 废纸篓 | `utils.rs:572` |

### 1.3 关键约束（决定实现方式）

1. **滚轮单位不统一**：winit `LineDelta*50`（真滚轮 1 格=50）；`PixelDelta=px/10`（触控板 0.1~10）。→ 翻图必须累积+阈值+冷却，否则一次滑动翻几十张。
2. **Ctrl+滚轮被 egui 转成 `Event::Zoom`**（`notan_egui/plugin.rs:208-219`）。项目已设 `zoom_with_keyboard=false`（`main.rs:267`），该事件只填 `zoom_factor_delta`，不改 `ctx.zoom_factor` → **ctrl+滚轮可安全用于图片缩放**。
3. **macOS Shift+滚轮被 AppKit 换轴**：变成 `delta_x≠0, delta_y=0`。→ 旋转手势取主导轴；方向符号做成常量，实测反了改一行。
4. **右键零占用**：全仓库无 `MouseButton::Right` / `PointerButton::Secondary` 业务逻辑。
5. **绘制层支持旋转**：notan `DrawTransform::rotate_from(point, angle)` 的矩阵是后乘（`T/S` 先，`R(pivot)` 后），可对 `draw.image()` 施加绕屏幕某点的旋转 → **显示旋转不需要动像素缓冲**。
6. **`turbojpeg-sys` 与 `libaom-sys` 依赖 cmake**，本机未装（`cargo check` 已在 turbojpeg-sys 处失败）。

---

## 2. LLM 视角：把"改键位"做对的方法

1. **Spec 驱动**：本文件即唯一事实源，任何新会话只看它就能续做（含验收清单 + 决策记录）。
2. **纯函数化**：`事件 → 动作` 的判定抽成无 GUI 依赖的 `gestures.rs`，`cargo test` 直接验阈值/符号/优先级，不靠手感回归。
3. **单一真相**：键盘缩放、Ctrl+滚轮、右键拖拽共用 `zoom_at()`；旋转状态只有一个 `ViewRotation`，绘制与尺寸推导共用。
4. **最小侵入**：不动 `next_image/prev_image/lossless_tx/clear_image` 实现，只在调用侧接线；改动 5 个文件 + 1 新模块。
5. **可回退**：独立分支 + 原子 commit + 导出 `input-remap.patch`。
6. **可观测**：每个新触发点一条日志；越界操作（编辑态旋转、非预期手势）给 toast。

---

## 3. 实现方案

### 3.1 新增 `src/gestures.rs`（纯逻辑 + 单测）

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods { pub ctrl: bool, pub shift: bool, pub alt: bool }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelAction { PrevImage, NextImage, Zoom(f32), RotateCw, RotateCcw }

pub const LINE_MIN: f32 = 45.0;         // 低于此值不可能是"行"滚轮（notan 已把像素值除以 10）
pub const PX_PER_IMAGE: f32 = 30.0;     // 精确滚动每 30px 翻一张（设置页灵敏度可缩放）
pub const GESTURE_GAP_MS: f32 = 250.0;  // 停顿超过该值视为新手势，丢弃余量
pub const ROTATE_THRESHOLD: f32 = 50.0; // 与一格滚轮对齐
pub const DRAG_ZOOM_SPEED: f32 = 0.004; // 右键拖拽灵敏度
pub const ROT_DRAG_GAIN: f32 = 0.20;    // 中键拖拽：度/像素
pub const ROT_DRAG_GAIN_FINE: f32 = 0.04; // 按住 Shift 的精密档
pub const ROT_SNAP_DEG: f32 = 1.5;      // 吸附到 90° 倍数的容差
pub const CLICK_MAX_TRAVEL: f32 = 3.0;  // 中键单击/拖拽消歧阈值
pub const SHIFT_AXIS_SIGN: f32 = 1.0;   // macOS 换轴方向修正（实测可翻转）

#[derive(Debug, Default)]
pub struct Accumulator { rest: f32 }
impl Accumulator {
    pub fn steps(&mut self, delta: f32, threshold: f32) -> i32; // 取整并保留余量
    pub fn reset(&mut self);
}

/// 优先级：shift（旋转） > ctrl（缩放） > 无修饰（翻图）
pub fn resolve_wheel(acc: &mut Accumulator, dx: f32, dy: f32, mods: Mods) -> Option<WheelAction>;
pub fn dominant_delta(dx: f32, dy: f32) -> f32;   // Shift 换轴取主导轴
pub fn drag_zoom_delta(dy: f32, scale: f32) -> f32; // 上移=放大
pub fn is_actual_size(scale: f32) -> bool;          // |scale-1| < 0.005

/// 仅显示的视图变换：连续角度 + 水平镜像
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ViewTransform { pub angle_deg: f32, pub mirror_x: bool }
impl ViewTransform {
    pub fn add_angle(self, deg: f32) -> Self;        // 归一化到 (-180,180] 并吸附 90° 倍数
    pub fn quarter_steps(self, n: i32) -> Self;      // 先吸附到 90° 倍数，再 ±90°
    pub fn is_upright(self) -> bool;                 // 角度为 0 且未镜像
    pub fn aabb(self, w: f32, h: f32) -> (f32, f32); // 旋转后外接矩形
}
```

单测：阈值累积与余量、修饰键优先级、换轴主导轴、拖拽方向、1:1 容差、角度归一化/吸附、AABB（0/45/90 三组解析值）、`quarter_steps` 幂等。

### 3.2 视图变换：**单一矩阵**作为唯一真相

折叠/镜像/旋转都是线性变换。不再靠 `draw` 的链式调用顺序，而是显式构造矩阵，绘制与命中检测共用同一个函数：

```rust
// gestures.rs，纯数学，可单测（notan::math::Mat3，无 GPU 依赖）
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ViewTransform { pub rot: ViewRotation, pub mirror_x: bool }

impl ViewTransform {
    pub fn is_identity(&self) -> bool;
    /// M = Mirror ∘ R(pivot) ∘ T(offset) ∘ S(scale)
    pub fn matrix(&self, scale: f32, offset: (f32, f32), pivot: (f32, f32)) -> Mat3;
    /// 逆映射：屏幕点 -> 未变换坐标系（供 cursor_relative / 缩放锚点）
    pub fn untransform_point(&self, p: (f32, f32), scale: f32, offset: (f32, f32), pivot: (f32, f32)) -> (f32, f32);
    /// 逆映射线性部分：屏幕位移 -> offset 位移（供平移）
    pub fn untransform_delta(&self, d: (f32, f32)) -> (f32, f32);
}
```

绘制侧（`texture_wrapper.rs`）只做一件事：`draw.image(tex).transform(m)`，`m` 由上面的函数给出。所有纹理瓦片用同一个 `m` → 大图拼接不会撕裂。

`utils.rs`：

```rust
pub fn zoom_at(delta: f32, anchor_screen: Vector2<f32>, state: &mut OculanteState) {
    let anchor = state.anchored(anchor_screen);           // 内部调用 untransform_point
    let new_scale = state.image_geometry.scale + delta;
    if new_scale > ZOOM_MIN && new_scale < ZOOM_MAX {
        state.image_geometry.offset -= scale_pt(
            state.image_geometry.offset, anchor, state.image_geometry.scale, delta);
        state.image_geometry.scale = new_scale;
    }
}
pub fn toggle_always_on_top(app: &mut App, state: &mut OculanteState);
```

`set_zoom`、`ZoomIn/ZoomOut`、平移（左/中键 + 键盘 Pan）、`cursor_relative` 全部改走这两个 helper —— 镜像/旋转下的平移方向、缩放锚点、像素坐标才不会错（见 §9.2 B1/B2、§9.7 G1/G2）。

> 好处：`matrix()` 与 `untransform_point()` 互为逆，可用一条断言锁死（§9.5 E2），避免"渲染对、命中错"的经典分叉。

### 3.2b 几何不变量（本轮的教训，改代码前先读）

实测与推导确认：`offset` 是**纯屏幕平移**，旋转/镜像都在“随 offset 移动的图像中心”处发生，因此：

| 操作 | 正确公式 | 说明 |
|---|---|---|
| 平移 | `offset += 屏幕位移`（原样） | 绝不能再逆旋位移，否则图片会沿旋转后的轴走（用户反馈的 bug） |
| 缩放锚点 | `offset -= (cursor - offset) * Δ / scale`（cursor 用**原始屏幕坐标**） | 相机/图片的旋转项会相互抵消，光标下的像素严格不动（单测已锁） |
| 命中检测 | 先 `M⁻¹`（只含旋转/镜像，不含 scale/offset）再除以 scale | 得到图像像素坐标 |
| 边界限制 | 在**屏幕空间**限制图像中心：`pivot ∈ [W - aabb/2, aabb/2]` | 否则限制框会随图片旋转，拖动到边缘时方向“被旋转影响” |

对应单测：`panning_is_a_pure_screen_translation`、`zooming_keeps_the_pixel_under_the_cursor`。

### 3.3 `MouseWheel` 分支（替换 `main.rs:576-617`）

```rust
Event::MouseWheel { delta_x, delta_y } => {
    // precise_scroll_remainder 逻辑保持原样
    if state.pointer_over_ui { return; }
    let mods = Mods {
        ctrl: app.keyboard.ctrl() || (cfg!(target_os = "macos") && app.keyboard.logo()),
        shift: app.keyboard.shift(),
        alt: app.keyboard.alt(),
    };
    match gestures::resolve_wheel(&mut state.wheel_acc, delta_x, delta_y, mods) {
        Some(WheelAction::PrevImage) => prev_image(state),
        Some(WheelAction::NextImage) => next_image(state),
        Some(WheelAction::Zoom(raw)) => {
            let d = zoomratio((raw / divisor) * mult, scale).clamp(-5.0, 5.0);
            zoom_at(d, state.cursor, state);
        }
        Some(WheelAction::RotateCw)  => rotate_view(state, 1),
        Some(WheelAction::RotateCcw) => rotate_view(state, -1),
        None => {}
    }
}
```

- 翻图额外加 **100ms 冷却**，避免加载线程被刷爆。
- 旋转 **不写盘**，只改 `state.view_rotation` + `app.window().request_frame()`。

### 3.4 右键按住上下拖拽 = 缩放

```rust
MouseDown { button: Right } → state.zoom_drag = true
MouseUp   { button: Right } → state.zoom_drag = false
// update() 中、mouse_delta 计算之后：
if state.zoom_drag && !state.pointer_over_ui && state.mouse_delta.y != 0.0 {
    let d = gestures::drag_zoom_delta(state.mouse_delta.y, state.image_geometry.scale);
    zoom_at(d, state.cursor, state);
}
```

不写 `drag_enabled`，与左/中键平移完全解耦。

### 3.5 左键双击 = 1:1 ↔ Reset view（替换 `main.rs:1059-1067`）

```rust
if !state.pointer_over_ui && !state.mouse_grab
    && ctx.input(|r| r.pointer.button_double_clicked(Primary)) {
    if gestures::is_actual_size(state.image_geometry.scale) {
        state.reset_image = true;                       // 自适应窗口，并清零视图旋转
        state.view_rotation = ViewRotation::default();
    } else {
        set_zoom(1.0, None, state);                     // View 1:1，锚点=光标
    }
}
```

全屏保留：`F` 键 + 顶栏按钮。

### 3.6 右键双击 = Always on top

```rust
if !state.pointer_over_ui && !state.mouse_grab
    && ctx.input(|r| r.pointer.button_double_clicked(Secondary)) {
    toggle_always_on_top(app, state);
}
```

### 3.7 Shift+滚轮 = 90° 步进旋转（仅显示、不写盘）

`state.view = state.view.quarter_steps(±1)` → **先把当前角度吸附到最近的 90° 倍数，再加减 90°**。这样"用中键调到 12.3° 精修"后再按 Shift+滚轮，会得到干净的 90°/180°，而不是 102.3°。

绘制侧（`texture_wrapper.rs`）改为接收一个完整矩阵：

```rust
pub fn draw_textures(&self, draw: &mut Draw, tx: f32, ty: f32, scale: f32, m: Mat3) {
    draw.image(tex).blend_mode(BlendMode::NORMAL).transform(m);
}
```

`main.rs` 绘制段（1200-1240）用 `state.view.matrix(scale, offset, pivot)` 产出 `m`；`show_frame` 边框、棋盘背景同样用 `m`。瓦片模式（`tiling>1`）每块复用同一 `m`，复合结果一致。

配套一致性处理：
- `limit_offset`、reset view 的自适应缩放用 `view.aabb(w, h)`（任意角度的外接矩形），不再假设 90° 倍数。
- 新图载入 / 双击回 fit / `V` 键时 `view.angle_deg` 归零（"临时"语义）。
- **编辑模式（`edit_enabled`）下禁用旋转**：编辑叠加层（crop/bbox/画笔）按未变换坐标绘制，旋转会错位；给一条 toast 提示。
- 信息面板放大镜（`draw_zoomed`）不跟随变换（次要差异，文档记录）。

### 3.8 禁用 `ClearImage`（功能禁用）

| # | 位置 | 改动 |
|---|---|---|
| 1 | `shortcuts.rs:133` | 删除默认绑定 `Shift+Delete` |
| 2 | `main.rs:505-507` | 删除 `key_pressed(...ClearImage)` 调用块（唯一触发路径消失） |
| 3 | `ui/top_bar.rs:226-237` | 删除顶栏 "Clear image" 按钮 |
| 4 | `ui/settings_ui.rs::keybinding_ui` | 列表过滤 `InputEvent::ClearImage` |

**保留**枚举变体（否则旧 `settings.json` 里 `Shortcuts` 反序列化整体失败）与 `utils::clear_image()`（`delete_file` 内部仍用，`utils.rs:592`）。

验证：Shift+Delete 无反应；Delete 仍弹确认并进废纸篓；`[`/`]` 键行为不变（仍是无损写盘旋转，未改动）。

### 3.9 新增状态（`appstate.rs`）

```rust
pub wheel_acc: gestures::Accumulator,
pub zoom_drag: bool,
pub view: gestures::ViewTransform,             // angle_deg + mirror_x
pub rotation_path: Option<PathBuf>,            // 复位钩子用
pub last_nav_at: Option<std::time::Instant>,    // 翻图冷却
pub middle: gestures::MiddleGesture,            // 见 §3.10 状态机
```

### 3.10 中键：**单击 = 镜像翻转 / 拖拽 = 精密旋转**

#### 3.10.1 可行性

| 功能 | 实现方式 | 成本 |
|---|---|---|
| 单击镜像 | 在 §3.2 矩阵外面乘一个反射矩阵 | 零拷贝、零新纹理 |
| 拖拽精密旋转 | 连续角度写入 `view.angle_deg`，走同一个矩阵 | 零拷贝；仅多几次三角函数 |

- **顺序**：矩阵 `M = Mirror ∘ R(θ) ∘ T ∘ S`。反射在最外层 → 翻转的是"你当前看到的画面左右"（先转后翻），与 90° 步进叠加符合直觉。
- **背面剔除**：负行列式翻转三角形绕序 → 已确认 notan 默认 `cull_mode: CullMode::None`（`notan_graphics/src/pipeline.rs:374`）且项目未覆盖，镜像不会被剔除。
- **UV**：镜像的是顶点位置，UV 跟随顶点 → 纹理呈镜像（文字反向），符合"翻转图片"预期。
- **瓦片大图**：所有瓦片共用同一 `m`，天然一致。

#### 3.10.2 中键状态机（替代原有"中键拖动平移"）

```rust
pub enum MiddleGesture { Idle, Pressed { start: (f32,f32), max_travel: f32 }, Rotating }
```

| 阶段 | 行为 |
|---|---|
| `MouseDown(Middle)`（画布内、非编辑态、有图像） | → `Pressed{start, 0}`；**不**设置平移 |
| 每帧 `is_down(Middle)` | `max_travel = max(max_travel, dist(cursor, start))`；一旦 `> 3px` → `Rotating` |
| `Rotating` 期间 | `view = view.add_angle(dx * gain)`，`gain` 默认 0.20°/px，按住 Shift → 0.04°/px（精密档）；每帧节流 toast 显示当前角度 |
| `MouseUp(Middle)` | 若仍是 `Pressed`（从未超过 3px）→ **切换镜像**；若 `Rotating` → 结束，不翻转；回到 `Idle` |
| 编辑模式 / 指针在 UI 上 / 无图像 / `settings_enabled` | 不进入状态机 |

关键取舍：
- **拖拽只取水平位移 `dx`**：竖直已被右键缩放占用，混用会产生"转着转着变缩放"的误操作；水平单调映射也更符合"精密"直觉（1px = 0.2°，Shift 档 1px = 0.04°）。
- **旋转围绕图像中心**（矩阵 pivot），与 90° 步进、镜像同一 pivot，视觉连续。
- **吸附**：结果落在 90° 倍数 ±1.5° 内时吸附到整数倍（`ROT_SNAP_DEG`），让"回正"可达成；`V` / 双击 fit / 切图仍是一键归零。
- **角度实时读数**：`Rotating` 期间用 egui 在画布角落显示 `θ = 12.3°`（1s 节流，避免每帧刷 toast），否则"精密"无从判断。

#### 3.10.3 行为变更（必须知情）

| 旧 | 新 |
|---|---|
| 中键按住拖动 = 平移 | 中键拖动 = 精密旋转 |
| 平移只剩左键拖拽 + 键盘 Shift+方向键 | 左键拖拽平移保留（方向经 `untransform_delta` 修正） |

用**最大位移**（而非抬手时的位移）判定，避免"拖出去再拖回来"被误判成单击。

"浏览下一张再回来就回到正常"：由 §9.1 A1 的路径钩子统一复位（与旋转共用同一处），返回时必然是正常方向。

---

## 4. 最终映射表

| 输入 | 动作 | 备注 |
|---|---|---|
| 滚轮 上/下 | 上一张 / 下一张 | 阈值 30 + 100ms 冷却 |
| Ctrl 或 ⌘ + 滚轮 | 缩放（光标锚点） | 与键盘 `=`/`-` 共用 `zoom_at` |
| **Shift + 滚轮** | **视图旋转 右/左 90°（仅显示）** | 不写盘；编辑模式下禁用 |
| 右键按住上下拖 | 缩放 | 上移=放大 |
| **中键单击** | **水平镜像翻转（仅显示）** | 再次单击取消；切图自动复位 |
| **中键按住拖动** | **精密旋转（连续角度）** | 0.2°/px，按住 Shift → 0.04°/px；±1.5° 内吸附 90° 倍数 |
| 左键双击 | 1:1 ↔ Reset view | Reset 时清除旋转（镜像由中键单独控制） |
| 右键双击 | Always on top 开关 | 等价 `T` 键 |
| 左键拖 | 平移 | 方向经逆变换修正 |
| ~~中键拖~~ | ~~平移~~ | **已改为旋转** |
| ← → Home End | 翻图/首末 | 不变 |
| `[` `]` | 无损旋转（写盘，JPEG） | 上游原有，未改动 |
| Delete | 废纸篓（有确认框） | 不变 |
| ~~Shift+Delete~~ | ~~ClearImage~~ | **移除** |
| `F` | 全屏 | 不再由双击触发 |

---

## 5. 执行步骤

| # | 步骤 | 验证 |
|---|---|---|
| 0 | `brew install cmake` | `cmake --version` |
| 1 | `git switch -c feat/input-remap` | 分支就绪 |
| 2 | 新增 `src/gestures.rs` + 单测，`lib.rs` 注册 | `cargo test --lib gestures` |
| 3 | `utils.rs` 加 `zoom_at`/`toggle_always_on_top`，重构既有调用 | 编译 + 行为等价 |
| 4 | 改滚轮/右键拖拽/双击 | 手动手感 |
| 5 | `ViewTransform`（连续角度+镜像）+ 绘制矩阵 + AABB 换算 + 复位钩子 | 旋转/镜像画面不写盘（`stat` 校验 mtime） |
| 5b | 中键状态机：单击镜像 / 拖动精密旋转（含 3px 消歧、角度读数） | 中键拖=旋转；单击=翻转 |
| 5c | 删除原"中键拖动平移"分支，左键平移改走逆变换 | 平移方向在旋转/镜像下正确 |
| 6 | 禁用 `ClearImage`（4 处） | Shift+Delete 失效 |
| 7 | `cargo test` + `cargo build --release` | `target/release/oculante` |
| 8 | 手动验收（§7）+ README 增 "Mouse" 段 + patch 导出 | 交付 |

磁盘：`target/` 约 6~9GB（release），完成可 `cargo clean` 回收。

---

## 6. 风险与对策

| 风险 | 对策 |
|---|---|
| 触控板连翻 | 累积阈值 + 100ms 冷却 |
| macOS Shift 换轴/方向反 | 主导轴 + `SHIFT_AXIS_SIGN` 一行翻转 |
| 旋转后平移边界/自适应缩放宽高不符 | `ViewRotation::dimensions()` 统一换算 |
| 旋转与编辑叠加层错位 | 编辑模式下禁用旋转并 toast |
| Ctrl+滚轮被 egui 当 UI 缩放 | 已验证 `Event::Zoom` 不改 `zoom_factor`；若实测有异，拦截 `Event::Zoom` |
| 瓦片大图旋转错位 | 所有瓦片共用同一 pivot，几何一致 |
| 镜像导致三角形绕序反转被剔除 | 已确认 `cull_mode=None`，不剔除；若日后上游改为剔除，改为镜像 UV 而非顶点 |
| 中键单击与中键平移互相误判 | 最大位移 < 3px 才算单击；拖出去再拖回也不会误判 |
| 中键拖拽=旋转后与原平移逻辑冲突 | 删除 `is_down(Middle)` 平移分支，左键平移保留 |
| 任意角度下宽高换算失真 | `aabb()` 解析式 + 单测（0/45/90/135） |
| 上游合并冲突 | 5 文件 + 1 模块，patch 独立 |

---

## 7. 验收清单

- [ ] 滚轮一格=翻一张；触控板轻扫 1~2 张不连翻
- [ ] ←/→/Home/End 仍正常
- [ ] Ctrl(⌘)+滚轮围绕光标缩放，界面字号不变
- [ ] 右键上拖放大/下拖缩小；横向拖不缩放
- [ ] 左键双击：1:1 ↔ 自适应；Reset 后旋转归零
- [ ] 右键双击：置顶开关，顶栏按钮状态同步
- [ ] Shift+滚轮：画面旋转 90°，**文件 mtime 不变**（`stat -f %m` 对比）
- [ ] 旋转 90° 后自适应缩放与平移边界正常
- [ ] 编辑模式下 Shift+滚轮不旋转 + 有提示
- [ ] **中键单击：画面左右翻转；再单击恢复**
- [ ] **中键拖动：画面连续旋转，角标读数同步；期间画面只转不飘（无平移）**
- [ ] **Shift+中键拖动：进入 0.04°/px 精密档，能精确对齐地平线**
- [ ] **旋转到 0.8° 附近自动吸附回 0°；12.3° 时 Shift+滚轮得到 90° 而非 102.3°**
- [ ] **翻转+旋转 90°+ 任意角度叠加后，平移/缩放锚点仍正确，信息面板坐标正确**
- [ ] **翻转或旋转后切下一张再切回，画面为正常方向**
- [ ] 45° 旋转后双击 fit 能完整显示（AABB 生效）
- [ ] `F` 全屏正常；左/中键平移正常
- [ ] Shift+Delete 无反应；Delete 进废纸篓；`[`/`]` 仍可无损旋转

---

## 8. 决策记录

| # | 决策 | 结果 |
|---|---|---|
| D1 | 构建依赖 | 装 cmake |
| D2 | 缩放修饰键 | Ctrl 与 ⌘ 都接受 |
| D3 | 旋转语义 | **仅显示旋转，禁止写盘** |
| D4 | 交付 | release 二进制 |
| D5 | 旋转持久性 | 切换图片 / Reset view 时归零 |
| D6 | 1:1 锚点 | 光标 |
| D7 | ClearImage | 彻底移除入口（保留枚举） |
| D8 | 中键单击语义 | 水平镜像翻转（仅显示），再次单击取消 |
| D9 | 镜像复用范围 | 与旋转共用同一 `ViewTransform` 矩阵与同一复位钩子 |
| D10 | 中键拖拽语义 | **精密旋转**（替代原平移），0.2°/px，Shift 精密档 0.04°/px |
| D11 | 旋转表示 | 连续角度 `angle_deg: f32`（非离散 4 档），±1.5° 吸附 90° 倍数 |
| D12 | 复用策略 | 平移只剩左键 + 键盘；`Shift+滚轮` 先吸附再 ±90° |

---

## 9. 安全性与健壮性审查（v2 增补）

前提：绘制变换链为 **R(pivot) ∘ T(offset) ∘ S(scale)**（notan 的 translate/scale/rotate_from 都是左乘到当前矩阵，即后设的先作用于顶点）。所有几何副作用都由此推出。

### 9.1 A 类：状态一致性

| # | 问题 | 触发 | 后果 | 对策 |
|---|---|---|---|---|
| A1 | 旋转复位点太多 | 翻图有 9 个入口：`next/prev/first/last`、scrubber 拖动、文件浏览器、拖放、粘贴、Delete、compare、stdin 管道 | 漏一个就出现"新图带着上一张的旋转" | **不在调用点复位**，在 `update()` 用单一钩子：`current_path != rotation_path` → 归零并记录新 path。覆盖全部入口，零遗漏 |
| A2 | 动画帧误清零 | GIF/WebP 每帧都走 `Frame::Animation`，不改 `current_path` | 动画播放中旋转被清 | 钩子比对路径而非 `new_image_loaded`，动画不影响 |
| A3 | `zoom_drag` 卡死 | 在窗口外松开右键 / 窗口失焦丢失 `MouseUp` | 之后鼠标一动就缩放 | 每帧同步 `if !app.mouse.is_down(Right) { zoom_drag = false }`，不依赖事件配对 |
| A4 | 与 `keep_view` 冲突 | 用户开了"保留视图" | 旋转仍归零，语义不一致 | 先按"临时"语义归零；若你要求跟随 `keep_view`，改一行条件 |
| A5 | EXIF 叠加 | 加载时 `rotate_dynimage` 已按 EXIF 转正 | 旋转叠加在 EXIF 之上 | 语义即"在当前显示方向上再转 90°"，符合直觉，无需处理 |

### 9.2 B 类：几何正确性（最易产生隐蔽 bug，必须集中处理）

| # | 问题 | 后果 | 对策 |
|---|---|---|---|
| B1 | **平移方向被旋转**：`offset` 是"旋转前"坐标，90° 时右拖图像会下移 | 手感完全错乱 | 屏幕位移先逆旋再累加：`offset += R(-θ)·mouse_delta`（左/中键拖拽 + 键盘 Pan 四处统一走 helper `unrotated_delta()`） |
| B2 | **缩放锚点漂移**：`zoom_at`/`set_zoom` 用屏幕坐标做锚点 | 旋转后缩放朝错误方向跑 | 锚点先逆旋到未旋转坐标系：`anchor' = pivot + R(-θ)(cursor − pivot)`；集中在 `zoom_at`/`set_zoom` 内，键盘 `=`/`-`/`1`~`5` 自动受益 |
| B3 | **大图瓦片 pivot 错**：`tiling>1` 时图像由 N×N 块拼成 | 各块绕不同点转 → 画面撕裂 | pivot 必须是合成图中心：`offset + dims·scale·tiling/2` |
| B4 | 边界与自适应用未旋转宽高（任意角度下 90° 互换公式失效） | Reset 后画面跑偏、滚不到角、1:1 判定失真 | 统一用 `view.aabb(w,h)`：`w' = |w·cosθ| + |h·sinθ|`，`h' = |w·sinθ| + |h·cosθ|`（`limit_offset`、fit、`set_zoom` 边界） |
| B5 | `cursor_relative` 未逆旋 | 信息面板坐标/取色/放大镜中心全错 → **给出错误数据**（比视觉错更糟） | `main.rs:706` 与 `ui/mod.rs:576` 的 `pos_from_coord` 入参先逆旋 |
| B6 | 棋盘背景与 `show_frame` 边框未旋转 | 透明图 90° 时背景与图像不重合，视觉"错切" | `draw.pattern` 与边框 rect 施加同一 pivot 旋转 |
| B7 | 放大镜 `draw_zoomed` 不旋转 | 放大区域方向与主画面差 90° | 记录为已知差异（改动需重写瓦片裁剪逻辑，收益低）；如需一致，二期做 |

### 9.3 C 类：输入判定

| # | 问题 | 后果 | 对策 |
|---|---|---|---|
| C1 | 右键拖拽后松手被 egui 记成 click，短时间再拖一次 → 误触发置顶 | 意外改窗口层级 | `zoom_drag_moved`（位移 >4px）标记；处理 Secondary 双击时要求它为 false；每次右键按下清零 |
| C2 | 单次滚轮事件 `steps()` 可能 \|n\|>1（触控板大 delta） | 一次跳 20 张 | **单帧最多消费 1 步**，其余丢弃 |
| C3 | 冷却期吞掉最后一格 | 快速滑动末尾无响应 | 冷却期间累计 `pending`，冷却结束补一次（每帧检查） |
| C4 | macOS Shift 换轴 + `from_angle` 方向（y 向下坐标系正角度=顺时针） | 旋转方向可能相反 | 两个常量：`SHIFT_AXIS_SIGN`、`ROT_DIR_SIGN`，实测一行翻转 |
| C5 | 修饰键优先级含糊 | shift+ctrl+wheel 行为不确定 | 固定 shift > ctrl > 无；alt 保留不用；单测锁定 |
| C6 | `pointer_over_ui` 有一帧延迟（egui 帧末才更新） | 从画布划到面板的瞬间可能误翻一张 | 再叠加 `settings_enabled`/`key_grab` 判断；一帧误差接受（最多误触发一次，不会连续） |
| C7 | 编辑态（painting / `block_panning`）中翻图 | 编辑结果 `result_pixel_op` 可能落到错的图片 / `.oculante` 状态错配 | 编辑态禁止翻图与旋转，仅允许缩放平移；给一次 toast |

### 9.4 D 类：数据安全（本次红线）

| # | 承诺 | 校验方式 |
|---|---|---|
| D1 | 新增手势**零磁盘写入**（翻图/缩放/双击/视图旋转全不碰文件） | 旋转 20 次后 `stat -f %m` 与旋转前一致 |
| D2 | 唯一写盘点仍是原有的 `[`/`]` 无损旋转与显式保存，均未改动 | 代码审查 diff |
| D3 | `Delete` 仍是 `trash` + 确认弹窗；`ClearImage` 四个入口全移除 | 手动测试 + grep `ClearImage` 无调用 |
| D4 | 不做静默删除、不做批量重命名、不动用户目录 | 除 `brew install cmake` 与仓库内文件外无系统变更 |

### 9.5 E 类：可测试性与回退

| # | 内容 |
|---|---|
| E1 | `gestures.rs` 单测：阈值/余量/优先级/旋转取模/单帧限步/pending 补发 |
| E2 | **几何不变性测试**：`untransform_point/untransform_delta` 与 `matrix()` 严格互逆（误差 < 1e-3，枚举 angle∈{0,12.3,45,90,135,180,270} × mirror∈{false,true}），另加 `aabb()` 与解析值比对 —— 把 B1/B2/B4/G1/G2/H2 变成自动化断言 |
| E3 | 分支 `feat/input-remap` + 原子提交 + `input-remap.patch`；`cargo test` 全绿才 release 构建 |
| E4 | 性能：每帧仅多几次矩阵乘法；旋转不复制像素缓冲（对比"旋转像素"方案省 100MP×4B 峰值内存） |

### 9.6 F 类：交互完整性（避免"改完发现没法回到原状"）

| # | 问题 | 对策 |
|---|---|---|
| F1 | 旋转后没有复位手段 | 左键双击回 fit 时归零；`V`（ResetView）同上；切图自动归零 |
| F2 | 用户不知道当前旋转了多少 | 旋转后 toast 一次 `视图旋转 90°（仅显示）`，1s 节流防刷屏 |
| F3 | 全屏入口 | `F` + 顶栏按钮保留，受影响的只是"双击"这一个入口 |
| F4 | 新手势无文档 | README 加 "Mouse" 表 + 设置页不改（键位表只列键盘，避免误导） |

### 9.7 G 类：镜像专项（中键单击）

| # | 问题 | 触发条件 | 后果 | 对策 |
|---|---|---|---|---|
| G1 | **平移方向在镜像下反向** | 翻转后拖拽/键盘 Pan 做 `offset += delta`，而 `offset` 在未变换坐标系 | 右拖图像往左走 | 平移统一走 `untransform_delta()`（线性部分 = `R⁻¹·M`，M 自逆） |
| G2 | **缩放锚点在镜像下反向** | 翻转后滚轮/双击 1:1 | 光标锚点跑到对称位置，缩放"往反方向跑" | `zoom_at` 内部用 `untransform_point()`，与绘制矩阵严格互逆 |
| G3 | **`cursor_relative` 未镜像** | 翻转后看信息面板/取色 | 坐标与颜色取到镜像位置 → 错误数据 | `pos_from_coord` 入参先 `untransform_point`（`main.rs:706`、`ui/mod.rs:576`） |
| G4 | 负行列式导致背面剔除 | 若 `cull_mode != None` | 镜像后整张图消失 | 已核实默认 `CullMode::None`（`notan_graphics/src/pipeline.rs:374`）且项目未覆盖；验收单列"翻转后图像仍在" |
| G5 | 中键单击 / 中键拖动互相误判 | 中键原本就是平移 | 想平移却翻转、或想翻转却微移 | 用**按下期间最大位移 < 3px** 判定单击；并加 `pointer_over_ui`/编辑态/无图像三重 guard |
| G6 | 拖出去再拖回被当成单击 | 位移回到 0 | 误翻转 | 用最大位移而非末位移（G5 同一条实现） |
| G7 | 镜像状态跨图残留 | 翻到下一张 | 画面方向"莫名颠倒" | 与旋转共用 §9.1 A1 路径钩子统一复位 |
| G8 | 镜像 + 旋转顺序歧义 | 同时开两种 | 用户预期与实际不一致（左右翻变上下翻） | 固定 **先旋转后镜像**（镜像在最外层 = 屏幕空间左右翻），单测锁定矩阵顺序 |
| G9 | 镜像后棋盘背景/边框不跟随 | 透明图 | 背景与图像错位 | 棋盘 pattern 与 `show_frame` 复用同一矩阵 |
| G10 | 镜像与 1:1 / fit 判定 | 双击 | 误以为 reset 也应取消镜像 | 明确语义：镜像只由中键切换 + 切图复位；`V`/双击只复位旋转与缩放 |
| G11 | 触摸板无中键 | 无外接鼠标 | 功能不可达 | 记录为已知限制；如需，二期增加可选键盘替代键（不默认占用） |
| G12 | toast 刷屏 | 连续点中键 | 消息堆积 | 状态类提示 1s 节流 |

### 9.8 H 类：精密旋转专项（中键拖拽）

| # | 问题 | 触发条件 | 后果 | 对策 |
|---|---|---|---|---|
| H1 | **残留的中键平移分支** | 忘了删 `main.rs:693` 的 `app.mouse.is_down(Middle)` | 拖拽时同时旋转+平移，画面"边转边飘" | 显式删除该条件，只保留左键 `drag_enabled`；验收单列"中键拖拽时画面只转不飘" |
| H2 | **任意角度下的 AABB 失效** | 45° 时按 90° 公式算宽高 | reset 后图片超出窗口 / 边角滚不到 | `aabb()` 解析式 + 单测（0/45/90/135/180 与解析值对比） |
| H3 | 角度漂移累积 | 反复拖拽 | 12.0001° 这种脏值，且"回正"不可达 | 每次 `add_angle` 归一化到 (-180,180]；±1.5° 内吸附 90° 倍数；`V`/双击/切图一键归零 |
| H4 | 90° 步进把角度搞脏 | 12.3° 时 Shift+滚轮 | 得到 102.3°，用户预期是 90° | `quarter_steps()`：**先吸附再加减 90°** |
| H5 | 旋转与右键缩放同时进行 | 两键一起按 | 缩放锚点与角度同时变，画面乱跳 | 状态机互斥：`Rotating` 期间忽略 `zoom_drag` 的累积，反之亦然 |
| H6 | 中键按下期间切图 | 滚轮+中键混用 | 角度按上一张的起点累积 → 新图带旋转 | 路径钩子复位时同时 `middle = Idle` |
| H7 | 旋转中指针移出画布 | 拖到面板/窗口外 | 旋转被 UI 打断或状态卡住 | 模式在按下时锁定；丢掉 `MouseUp` 时每帧用 `!is_down(Middle)` 兜底复位 |
| H8 | 精度不可感知 | 0.2°/px | 用户不知道转了多少度 | `Rotating` 期间画布角落显示 `θ = 12.3°`（1s 节流） |
| H9 | 旋转后瓦片接缝 | `tiling>1` + 非 90° | 相邻瓦片 1px 裂缝（现有代码对 offset 做 `trunc()`） | 变换非恒等时**不做 `trunc()`**，用浮点 offset；单列验收项 |
| H10 | 旋转后图像边缘锯齿/空白 | 任意角度 | 四角出现背景 | 属预期（不裁切）；缩放仍以图像中心为基准，不会突然跳位 |
| H11 | 性能 | 每帧一次三角函数 + 矩阵乘 | 可忽略 | 不重建纹理、不重算图像，只改矩阵 |
| H12 | 误触旋转 | 中键单击时手抖 4px | 画面歪 0.8°，且不再翻转 | 3px 阈值 + 吸附 90° 倍数（0° 在吸附范围内）→ 小抖动自动回正 |
