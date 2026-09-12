# Agent Note: 为什么引入 Agent Notes 决策留痕规范

Status: implemented

## Problem

本群由多个 Agent 轮流在同一仓库工作，每个会话都从零开始。已被否决的方案会被反复重提，妥协设计会被当成疏漏「顺手重构」。仓库此前没有任何决策记录，`AGENTS.md` 为空文件。

## Decision

采用 write-notes-like-deepseek 的正文纪律：路径即状态、四段骨架、反稻草人备选、现在时。规则写在 `AGENTS.md`，笔记存放于 `.agents/notes/`，模板见 `.agents/notes/TEMPLATE.md`。不引入其 CI 脚本与 HTML 看板。

## Alternatives considered

- **不做，继续依赖群聊与宿主群笔记**：零成本，且群笔记已能跨会话共享。但群笔记文件名为 UUID 且不随代码提交，无法在改代码时被就近发现，也不能与代码同一提交演进。
- **完整引入四件套（含 CI 门禁）**：机械校验能挡住格式漂移。但本仓库 CI 只有前端 lint，Node 脚本需额外维护；先靠模板与规范约束，格式漂移成为实际问题时再加。
- **传统 ADR 目录 `docs/adr/`**：更常见。但 ADR 无状态路径与反稻草人要求，历史上容易腐烂为摆设。

## Consequences

- **收益**：新 Agent 改动前有明确可查的决策边界；被否决方案有据可依。
- **代价与上限**：每次非平凡变更多写约 200 字。若笔记超过约 50 篇仍无人维护，或格式频繁漂移，需重访是否引入校验脚本。
