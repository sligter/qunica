# Agent 行为规范

## 架构决策留痕（Agent Notes）

方法来源：[write-notes-like-deepseek](https://github.com/czm15053/write-notes-like-deepseek)。只借用正文纪律，不引入其 CI 脚本与看板。

在进行任何非平凡变更（行为、架构、协议、跨模块约定、测试策略）前：

1. 先在 `.agents/notes/` 下查找相关笔记，读完决策边界再动手。
2. 有新构想先写 `proposed/`；落地时随同一次提交移到 `implemented/` 并改为现在时。
3. 路径格式：`.agents/notes/{proposed|implemented|rejected|archived}/{class}/yyyy-mm-dd-topic.md`。
   class 只允许：`feature` `bug-fix` `architecture` `process` `testing` `simplification`。
4. 正文固定四段：`## Problem` `## Decision` `## Alternatives considered` `## Consequences`，约 200 字。
5. `Alternatives considered` 必须先写对方最强论据再否定，且必含「不做 / 复用现有方案」。
6. `Consequences` 同时写收益、代价，以及「出现什么信号时必须重访」。
7. `implemented` 状态禁止未来时与会话残留（「后续将」「经讨论」「本次 PR」）。
8. 代码改名、移路径、改默认值时，原地修改对应笔记，不追加流水账。
9. 代码核心入口留反向注释：`// Note: <理由> — 见 .agents/notes/...`。

黄金判据：半年后看这段代码若会问「为什么不用更简单的方案」，就必须写。

## 群笔记（宿主 `Notes/`）约定

群笔记文件名由宿主生成，路径不表达状态。每篇：

- 第一行 `# 标题`，第二行 `Status: proposed | implemented | rejected — <原因> | archived`，第三行 `Since: yyyy-mm-dd`，第四行 `Category: 决策 | 约定 | 踩坑`。
- 正文同样用上述四段骨架。此格式已内置到 Qunica 群笔记（新建模板、状态/类别选择、Agent 提示），见 `.agents/notes/implemented/feature/2026-09-12-built-in-group-note-method.md`。
- 单个 Agent 的建议不得直接写成群共识；同意方案不等于已实施。
- 涉及代码的决策以 `.agents/notes/` 为正文，群笔记只放一行链接。
- 不要手改 `Notes/index.md`，由宿主维护。
