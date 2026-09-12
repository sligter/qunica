# Agent 行为规范

## 架构决策留痕（Agent Notes）

`.agents/` 是本地 Agent 工作目录，由 `.gitignore` 忽略，不提交或上传仓库。新克隆没有该目录是正常情况；需要记录时按下述格式创建目录和笔记，不依赖已有本地模板。需要跨克隆共享的决策理由应写入受版本控制的项目文档或代码注释。

在进行任何非平凡变更（行为、架构、协议、跨模块约定、测试策略）前：

1. 先在本地 `.agents/notes/` 下查找相关笔记，读完决策边界再动手；目录或相关笔记不存在时，查阅项目文档和代码中的决策依据。
2. 有新构想先在本地写 `proposed/`；落地并验证后在本地移到 `implemented/` 并改为现在时，笔记不随代码提交。
3. 路径格式：`.agents/notes/{proposed|implemented|rejected|archived}/{class}/yyyy-mm-dd-topic.md`。
   class 只允许：`feature` `bug-fix` `architecture` `process` `testing` `simplification`。
4. 正文固定四段：`## Problem` `## Decision` `## Alternatives considered` `## Consequences`，约 200 字。
5. `Alternatives considered` 必须先写对方最强论据再否定，且必含「不做 / 复用现有方案」。
6. `Consequences` 同时写收益、代价，以及「出现什么信号时必须重访」。
7. `implemented` 状态禁止未来时与会话残留（「后续将」「经讨论」「本次 PR」）。
8. 代码改名、移路径、改默认值时，原地修改对应笔记，不追加流水账。
9. 代码核心入口用注释直接说明决策理由，例如 `// Note: <理由>`；需要进一步说明时，链接受版本控制的项目文档，不引用仅本地存在的 `.agents/notes/` 文件。

黄金判据：半年后看这段代码若会问「为什么不用更简单的方案」，就必须写。

## 群笔记（宿主 `Notes/`）约定

群笔记文件名由宿主生成，路径不表达状态。每篇：

- 第一行 `# 标题`，第二行 `Status: proposed | implemented | rejected — <原因> | archived`，第三行 `Since: yyyy-mm-dd`，第四行 `Category: 决策 | 约定 | 踩坑`。
- 正文同样用上述四段骨架，见 [群组功能文档](backend-rs/crates/backend/src/docs/guide/groups.md#shared-notes)。
- 单个 Agent 的建议不得直接写成群共识；同意方案不等于已实施。
- 涉及代码的决策若已有受版本控制的项目文档，群笔记只放链接，避免重复维护正文；`.agents/notes/` 仅供本地记录，不作为群笔记的唯一依据。尚无共享文档时，群笔记须直接写明必要的决策依据。
- 不要手改 `Notes/index.md`，由宿主维护。
