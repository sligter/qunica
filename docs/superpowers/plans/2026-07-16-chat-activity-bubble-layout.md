# Chat Activity Bubble and Message Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep user messages inside the chat column and collapse every agent response's reasoning and tool activity into one default-closed bubble.

**Architecture:** Add one normalized presentational component for activity metadata. Live stream and persisted message renderers map their existing data into that component while keeping scheduler and persistence contracts unchanged. Repair the message flex shrink chain and keep horizontal overflow local to code blocks and tables.

**Tech Stack:** React 19, TypeScript, Tailwind CSS, Vitest, Testing Library, Tauri 2.

## Global Constraints

- Frontend-only change; do not modify backend stream events or `content_json`.
- The outer Activity disclosure is collapsed by default, including while streaming.
- Reasoning is one combined section; tool calls remain individually inspectable inside the outer disclosure.
- Normal text wraps; code blocks and Markdown tables retain local horizontal scrolling.
- Keep existing chat colors, typography, human-input forms, tool statuses, and trace privacy.
- Implement first and add regression tests after implementation, per the user's explicit no-TDD instruction.

---

### Task 1: Shared Agent Activity Bubble

**Files:**
- Create: `frontend/src/components/chat/AgentActivityBubble.tsx`
- Create: `frontend/src/components/chat/AgentActivityBubble.test.tsx`

**Interfaces:**
- Produces: `ActivityReasoningSegment`, `ActivityToolItem`, and `AgentActivityBubble`.
- Consumes: existing `cn`, Lucide icons, and application color tokens.

- [ ] **Step 1: Create normalized activity types and the collapsed shell**

Implement these public contracts:

```tsx
export interface ActivityReasoningSegment {
  id: string
  content: string
  streaming?: boolean
}

export interface ActivityToolItem {
  id: string
  name: string
  status: string | null
  argsSummary?: string | null
  resultSummary?: string | null
  details?: ReactNode
}

interface AgentActivityBubbleProps {
  reasoning: ActivityReasoningSegment[]
  tools: ActivityToolItem[]
  active?: boolean
}
```

Return `null` when both arrays are empty. Otherwise render one native `details` without `open` or `defaultOpen`. Its summary uses the accessible label `Activity: {reasoningCount} reasoning, {toolCount} tools` and only includes non-empty visual count labels. Add a pulse indicator when `active=true`.

- [ ] **Step 2: Render one combined reasoning section and compact tool rows**

Inside the outer disclosure, render all non-empty reasoning segments in one section. Separate segments visually without creating nested top-level bubbles. Render tools as a compact vertical list; each tool uses a nested `details` row and keeps status, arguments, result, or the supplied `details` node accessible.

- [ ] **Step 3: Add component interaction tests**

Cover default-closed behavior, count labels, expansion, combined reasoning, tool detail expansion, active state, reasoning-only, tool-only, and empty input. Use `getByRole('group', { name: /activity:/i })` or a stable equivalent rather than styling selectors.

- [ ] **Step 4: Run focused tests and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- AgentActivityBubble.test.tsx
```

Expected: all activity bubble tests pass.

Commit:

```powershell
git add frontend/src/components/chat/AgentActivityBubble.tsx frontend/src/components/chat/AgentActivityBubble.test.tsx
git commit -m "feat(chat): consolidate agent activity"
```

---

### Task 2: Persisted Message Activity Integration

**Files:**
- Modify: `frontend/src/components/chat/PersistedTurnDetails.tsx`
- Modify: `frontend/src/components/chat/MessageItem.tsx`
- Create: `frontend/src/components/chat/MessageItem.test.tsx`

**Interfaces:**
- Consumes: `AgentActivityBubble`, `ActivityReasoningSegment`, `ActivityToolItem` from Task 1.
- Preserves: `PersistedTurnDetailsProps { reasoning?: string[] | null; toolCalls?: MessageToolCall[] | null }`.

- [ ] **Step 1: Replace persisted per-item bubbles with normalized activity data**

Map reasoning to stable IDs such as `persisted-reasoning-${index}` and map every `MessageToolCall` field to `ActivityToolItem`. Remove the old top-level `ReasoningBlock` and `ToolCard` stack. `PersistedTurnDetails` must return exactly one `AgentActivityBubble` when data exists.

- [ ] **Step 2: Keep the message content column shrinkable**

Change the `MessageItem` sender column to include `min-w-0` and an explicit responsive width contract, while retaining right alignment for user messages. Ensure both the persisted Activity bubble and final text bubble use `max-w-full` and cannot establish a wider intrinsic flex size.

- [ ] **Step 3: Add persisted integration tests**

Render an agent message containing multiple reasoning segments and tool calls. Assert that one Activity disclosure is present, it is closed initially, and expansion reveals all persisted details. Render a long user message and assert that the message row, sender column, and bubble expose the shrink/wrap classes required by the layout contract.

- [ ] **Step 4: Run focused tests and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- AgentActivityBubble.test.tsx MessageItem.test.tsx
```

Expected: both test files pass.

Commit:

```powershell
git add frontend/src/components/chat/PersistedTurnDetails.tsx frontend/src/components/chat/MessageItem.tsx frontend/src/components/chat/MessageItem.test.tsx
git commit -m "fix(chat): constrain persisted message layout"
```

---

### Task 3: Live Stream Activity Integration

**Files:**
- Modify: `frontend/src/components/chat/StreamTimeline.tsx`
- Create: `frontend/src/components/chat/StreamTimeline.test.tsx`

**Interfaces:**
- Consumes: `AgentActivityBubble`, `ActivityReasoningSegment`, `ActivityToolItem` from Task 1.
- Preserves: `StreamTimelineProps` and all current human-input submission behavior.

- [ ] **Step 1: Normalize live reasoning and tool events per agent block**

Add pure helpers that partition one `AgentBlock.events` array into activity and visible events. Map `reasoning` events to reasoning segments. Map `tool` and `external_run` events to tool items. Treat reasoning with `status='streaming'`, tools with `status='started'`, and running external executions as active activity.

- [ ] **Step 2: Preserve tool input requests inside the aggregate**

For live tool events, normalize human input as today and pass the existing compact `HumanInputRequestForm` as the tool item's `details` node only when `shouldRenderInputRequest` accepts it. Otherwise pass argument and result summaries. Do not duplicate input forms outside the Activity bubble.

- [ ] **Step 3: Render one Activity bubble before visible response content**

Remove `ReasoningBlock`, `ToolHistoryGroup`, `compactToolHistory`, and top-level tool/external cards. In `AgentBlockView`, render one `AgentActivityBubble` after the waiting hint and before non-activity events. Continue rendering response drafts, final agent messages, waits, warnings, and agent notices through the existing paths.

- [ ] **Step 4: Add live integration tests**

Build a `StreamRun` with multiple reasoning and tool events for one agent. Assert exactly one collapsed Activity disclosure, live count updates, expansion reveals every reasoning segment and tool, and the final response remains a separate visible message bubble. Include a running tool and verify the Activity summary reports an active state without opening automatically.

- [ ] **Step 5: Run focused tests and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- AgentActivityBubble.test.tsx StreamTimeline.test.tsx
```

Expected: activity and stream integration tests pass.

Commit:

```powershell
git add frontend/src/components/chat/StreamTimeline.tsx frontend/src/components/chat/StreamTimeline.test.tsx
git commit -m "fix(chat): fold live agent activity"
```

---

### Task 4: Horizontal Overflow Hardening

**Files:**
- Modify: `frontend/src/components/chat/MessageList.tsx`
- Modify: `frontend/src/components/chat/MarkdownMessage.tsx`
- Modify: `frontend/src/components/chat/MessageList.test.tsx`

**Interfaces:**
- Preserves: all `MessageListProps` and `MarkdownMessageProps`.

- [ ] **Step 1: Bound the message scroll viewport and centered column**

Add `min-w-0 overflow-x-hidden` to the message scroll viewport and `min-w-0` to the centered message column. Do not remove vertical scrolling or scroll-position restoration.

- [ ] **Step 2: Make Markdown prose wrap without breaking local wide surfaces**

Add an anywhere-wrap fallback to the Markdown root and safe breaking to links and inline code. Keep `overflow-x-auto` on fenced code blocks and table wrappers; add `max-w-full` to those wrappers so they scroll inside the bubble instead of widening ancestors.

- [ ] **Step 3: Extend message list regression coverage**

Stop mocking the layout contract away in the relevant test, or add a focused test that verifies the viewport and centered column shrink/overflow classes. Confirm scheduler summaries and stream timelines still anchor below their trigger message.

- [ ] **Step 4: Run chat tests and commit**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- AgentActivityBubble.test.tsx MessageItem.test.tsx StreamTimeline.test.tsx MessageList.test.tsx
```

Expected: all chat component tests pass.

Commit:

```powershell
git add frontend/src/components/chat/MessageList.tsx frontend/src/components/chat/MarkdownMessage.tsx frontend/src/components/chat/MessageList.test.tsx
git commit -m "fix(chat): prevent message overflow"
```

---

### Task 5: Quality Gate and Desktop Test Build

**Files:**
- Modify only if verification exposes a defect in files already listed above.

**Interfaces:**
- Produces: a verified Windows portable executable and NSIS installer from the feature branch.

- [ ] **Step 1: Run the complete frontend quality gate**

Run from `frontend/`:

```powershell
pnpm test
pnpm lint
pnpm type-check
pnpm build
```

Expected: tests, type-check, and build exit 0; lint has no new warnings beyond the three existing Fast Refresh warnings.

- [ ] **Step 2: Review the complete diff**

Run:

```powershell
git diff --check master...HEAD
git status --short
```

Confirm no backend files, generated `dist`, target files, or `.superpowers` content are staged.

- [ ] **Step 3: Build the desktop test artifacts**

Run from the repository root:

```powershell
pnpm desktop:build
```

If Tauri creates the binary and installer but exits only because `TAURI_SIGNING_PRIVATE_KEY` is unavailable, run:

```powershell
pnpm desktop:portable
```

Expected artifacts:

```text
frontend/src-tauri/target/release/bundle/portable/AG Swarmer_0.1.1-alpha_x64-portable.exe
frontend/src-tauri/target/release/bundle/nsis/AG Swarmer_0.1.1-alpha_x64-setup.exe
```

- [ ] **Step 4: Commit any verification fix**

Only when Step 1-3 required a source fix:

```powershell
git add frontend/src/components/chat
git commit -m "fix(chat): polish activity bubble layout"
```
