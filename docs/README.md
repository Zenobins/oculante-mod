# 文档索引

> **状态：已完成并通过验收（2026-11-06）**
> 分支 `feat/input-remap`（基于 `master` @ `fdc05eb`，14 个附加提交）
> 交付：`dist/Oculante.app` / `dist/Oculante-0.9.6-mod-20261106-0229.zip`
> 补丁：`input-remap.patch`（193 KB）；release 二进制 sha256 前缀 `0a2f6efb`

| 文件 | 用途 | 什么时候看 |
|---|---|---|
| `INPUT_REMAP_PLAN.md` | 需求规格（最终映射表、几何不变量、风险清单、决策记录） | 想改行为/加手势前 |
| `IMPLEMENTATION_SCHEDULE.md` | 执行台账（任务卡 + 硬门禁 + 阻塞记录 + 交付物） | 接手任务、恢复上下文时 |
| `MOD_CHANGELOG.md` | 本 fork 相对上游的全部改动 + **上游升级流程** | 上游更新、需要 rebase 时 |
| `MOD_WORKFLOW.md` | 方法论、工具链、几何模型、Rust/输入栈的坑 | 第一次接触这个仓库 |

推荐阅读顺序：`MOD_WORKFLOW.md` → `MOD_CHANGELOG.md` → `INPUT_REMAP_PLAN.md`。
