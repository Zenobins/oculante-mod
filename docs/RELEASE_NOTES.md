# 发布说明

> 本仓库只发布 **macOS** 构建。Windows / Linux 未做测试，也没有预编译产物，请使用上游的官方发布。
> 完整改动清单见 [`docs/MOD_CHANGELOG.md`](MOD_CHANGELOG.md)，这里是给使用者的简短版本。

---

## v0.9.6-mod.20261106-0229 — 2026-11-06（仅 macOS）

基于上游 `woelper/oculante` v0.9.6（`master` @ `fdc05eb`）。

### 下载

| 文件 | 用途 |
|---|---|
| `Oculante-0.9.6-mod-20261106-0229.zip` | macOS 应用（`Oculante.app`，17 MB，arm64，macOS 11+） |
| `input-remap.patch` | 相对上游的完整补丁（`git diff master..HEAD`），供审阅或自行编译 |

校验（zip）：

```
sha256  fef01ea54fd9614d34800f8f96cf7e02c37b424ded232fb63a381c798a0ff944
```

### 安装

1. 解压，把 `Oculante.app` 拖进 `/Applications`；
2. 首次打开：应用只做了 ad-hoc 签名（没有开发者证书、未公证），若被 Gatekeeper 拦下，
   右键 →「打开」，或执行：

```bash
xattr -dr com.apple.quarantine /Applications/Oculante.app
```

3. 版本号可在 Finder →「显示简介」里核对：`0.9.6-mod.20261106-0229`。

### 与上游的用法差异

| 操作 | 上游 v0.9.6 | 本 mod |
|---|---|---|
| 滚轮 | 缩放 | **上一张 / 下一张**（经典滚轮一格一张；触控板/平滑鼠标每 30px 一张，可在设置里调灵敏度） |
| Ctrl (⌘) + 滚轮 | 上一张 / 下一张 | **缩放**（锚点严格跟随光标，旋转/镜像后也准） |
| 左键双击 | 全屏 | **1:1 ↔ 自适应窗口**（全屏改为只用 `F` 键或顶栏按钮） |
| 左键拖拽 | 平移 | 不变（旋转后仍按窗口方向跟手） |
| 右键拖拽 | 无 | **缩放**（上拖放大，下拖缩小） |
| 右键双击 | 无 | **Always on top 开关** |
| 中键单击 | 无 | **水平镜像**（仅显示，再点取消） |
| 中键拖拽 | 平移 | **精密旋转**：0.2°/像素，按住 Shift 变 0.04°/px，左上角显示当前角度 |
| Shift + 滚轮 | 无 | **90° 步进旋转**（仅显示；`[`/`]` 仍是上游的无损写盘旋转） |
| Shift + Delete | 清空图片（ClearImage） | **已禁用**；`Delete` 仍然只把文件移到废纸篓（有确认框） |
| 菜单 View 1:1 | 以窗口中心缩放 | 以**图像中心**缩放（双击则以光标为锚点） |
| 多窗口 | 单窗口 | **支持多开**，每个窗口独立图片/缩放/置顶；`⌘N` 新开一个 |
| 应用运行中打开另一张图 | 报错 "cannot open files in the Image format" | 正常打开新窗口 |
| 再次双击 .app | 只激活旧窗口 | 打开一个新窗口 |
| 一次打开多个文件 | — | 合并到**一个窗口**（多张图进浏览序列） |

> 旋转与镜像是**仅显示**的：不写盘，切换图片即复位，编辑模式下自动禁用。别指望它们改文件——要改文件请用 `[`/`]`（无损 JPEG 旋转）或编辑面板。

### 新增文件

`src/gestures.rs`（手势/变换纯逻辑 + 单测）、`packaging/macos/Info.plist`、`scripts/make_macos_app.sh`、`docs/`（4 份文档）、`HANDOFF.md`。

### 已知限制

- 只提供 Apple Silicon (arm64) 构建；Intel Mac 需要自行编译（`cargo build --release`）。
- 未签名/未公证，跨机器分发需要绕过 Gatekeeper（见上）。
- 多窗口 = 多进程，Dock 里会有多个图标；窗口存在期间有一个无窗口的助手进程（不占 Dock）。
- 编辑模式（`E`）下禁用旋转/镜像，因为编辑叠加层按未变换坐标绘制。

### 回退

直接用上游官方 release 覆盖 `/Applications/Oculante.app` 即可；本 mod 没有改配置文件格式，设置可被上游读取（多出的 `wheel_nav_sensitivity` 字段上游会忽略）。
