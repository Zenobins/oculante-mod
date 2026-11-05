# 文档索引

> **状态：已完成并通过验收（2026-11-06）**
> 分支 `feat/input-remap`（基于 `master` @ `fdc05eb`，19 个功能与修复提交 + 若干文档提交（实时数量见 compare 链接））
> 交付：`dist/Oculante.app` / `dist/Oculante-0.9.6-mod-20261106-0229.zip`
> 补丁：`input-remap.patch`（220 KB）；release 二进制 sha256 前缀 `0a2f6efb`
> 远端：`origin` = 上游 `woelper/oculante`，`fork` = <https://github.com/Zenobins/oculante-mod>（默认分支已切到 `feat/input-remap`）

| 文件 | 用途 | 什么时候看 |
|---|---|---|
| [`../HANDOFF.md`](../HANDOFF.md) | **运维手册**：GitHub 账号/远端、Git 规范、发布流程、排障、文档维护规则 | 新会话第一站、要推代码/发版/排障时 |
| `INPUT_REMAP_PLAN.md` | 需求规格（最终映射表、几何不变量、风险清单、决策记录） | 想改行为/加手势前 |
| `IMPLEMENTATION_SCHEDULE.md` | 执行台账（任务卡 + 硬门禁 + 阻塞记录 + 交付物） | 接手任务、恢复上下文时 |
| `MOD_CHANGELOG.md` | 本 fork 相对上游的全部改动 + **上游升级流程** | 上游更新、需要 rebase 时 |
| `RELEASE_NOTES.md` | 面向使用者的发布说明（下载 / 安装 / 与上游差异 / 校验值） | 发版、回答"怎么装/有什么不同" |
| `MOD_WORKFLOW.md` | 方法论、工具链、几何模型、Rust/输入栈的坑 | 第一次接触这个仓库 |

推荐阅读顺序：`HANDOFF.md` → `MOD_WORKFLOW.md` → `MOD_CHANGELOG.md` → `INPUT_REMAP_PLAN.md`。
