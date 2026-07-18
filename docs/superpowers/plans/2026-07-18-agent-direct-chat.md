# Agent Direct Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-class one-to-one Agent chat mode with multiple independent sessions per Agent, automatic first-message titles, manual renaming, and the existing durable streaming behavior.

**Architecture:** Direct chats are typed conversation containers stored in the existing `groups`, `threads`, `messages`, and `stream_events` graph. Dedicated direct-chat lifecycle/message APIs enforce type and ownership, while the existing runtime handles the sole bound Agent; the frontend generalizes message transport and chat presentation without changing message-store persistence semantics.

**Tech Stack:** Rust/Axum, SQLx/SQLite, ag-swarmer domain events, React 19, TypeScript 5.7, TanStack Query, Zustand, React Router, Vitest/Testing Library.

## Global Constraints

- Direct chats appear in a dedicated sidebar section above Groups and use `/chats/:chatId` routes.
- A user can create multiple independent direct chats with the same Agent.
- Each direct chat has exactly one bound Agent and one initial thread.
- Direct chats reuse message persistence, SSE replay, interruption, resume, Agent Tools, Skills, model, runtime, and Workspace behavior.
- The first non-empty user message generates a title from the first 32 Unicode scalar values after whitespace normalization.
- A manual rename is never overwritten by automatic title generation.
- Disabled/deleted Agents leave history readable but prevent new sends.
- Group lists and group-management endpoints exclude direct chats; direct-chat APIs exclude groups.
- Existing group-chat behavior must not regress.
- This plan assumes `0004_system_settings_language.sql` from the bilingual plan may exist; its own migration is version `0005` and remains valid if SQLx applies it without `0004` in an isolated implementation branch.
- Execute Tasks 1-3 of `2026-07-18-bilingual-i18n.md` before this plan's Task 6, because the direct-chat UI adds keys to the initialized bilingual resources and uses locale formatting.

## File Structure

- Create `backend-rs/crates/backend/src/db/migrations/0005_direct_chats.sql`: typed conversation metadata and indexes.
- Create `backend-rs/crates/backend/src/api/conversations.rs`: shared kind/ownership checks.
- Create `backend-rs/crates/backend/src/api/direct_chats.rs`: direct-chat lifecycle API and response mapping.
- Modify `backend-rs/crates/backend/src/api/groups.rs`: filter group CRUD/management to `group` containers.
- Modify `backend-rs/crates/backend/src/api/messages.rs`: kind-specific group/direct handlers over shared message functions.
- Modify `backend-rs/crates/backend/src/api/mod.rs`: direct-chat routes.
- Modify `backend-rs/crates/backend/src/runtime/group.rs`: touch direct-chat activity, generate the first title, and emit metadata updates.
- Modify `backend-rs/crates/domain/src/events.rs`: add durable `conversation_updated` event kind.
- Modify `backend-rs/crates/backend/src/api/sse_replay.rs`: recognize/replay the new event.
- Create `backend-rs/crates/backend/tests/direct_chats.rs`: lifecycle, authorization, title, availability, streaming, replay, and isolation tests.
- Modify `backend-rs/crates/backend/tests/groups.rs`, `group_stream.rs`, and `sqlite_bootstrap.rs`: regression/migration coverage.
- Modify `frontend/src/types/api.ts` and `frontend/src/lib/api-v2/types.ts`: direct-chat and stream-event contracts.
- Create `frontend/src/hooks/useDirectChats.ts` and tests: list/detail/create/rename/delete cache behavior.
- Create `frontend/src/components/direct-chats/DirectChatPickerDialog.tsx` and tests: searchable Agent selection.
- Create `frontend/src/components/direct-chats/EditableDirectChatTitle.tsx` and tests: accessible optimistic rename.
- Create `frontend/src/pages/chat/DirectChatPage.tsx` and tests: one-Agent chat route and unavailable state.
- Create `frontend/src/components/chat/ConversationChatView.tsx`: shared group/direct chat canvas.
- Modify `frontend/src/pages/group/GroupChatPage.tsx`: configure the shared chat canvas.
- Modify `frontend/src/hooks/useGroupMessages.ts` and `useSendMessageStream.ts`: scope-aware conversation endpoints/query keys.
- Modify `frontend/src/components/layout/AppSidebar.tsx`, `frontend/src/pages/home/ChatHomePage.tsx`, and `frontend/src/routes.tsx`: navigation and recent conversations.

---

### Task 1: Add typed conversation metadata and isolate existing group APIs

**Files:**
- Create: `backend-rs/crates/backend/src/db/migrations/0005_direct_chats.sql`
- Create: `backend-rs/crates/backend/src/api/conversations.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/src/api/groups.rs`
- Modify: `backend-rs/crates/backend/tests/groups.rs`
- Modify: `backend-rs/crates/backend/tests/sqlite_bootstrap.rs`

**Interfaces:**
- Produces: `ConversationKind::{Group, Direct}` with `as_str()`.
- Produces: `ensure_active_owned_conversation(pool, id, owner_id, expected_kind)` and `ensure_active_owned_workspace_conversation(pool, id, owner_id)`.
- Produces: schema columns `conversation_kind`, `direct_agent_id`, and `title_source`.
- Preserves: `GroupResponse` JSON shape; group consumers do not need a kind field.

- [ ] **Step 1: Write failing migration and group-isolation tests**

In `sqlite_bootstrap.rs`, assert `pragma_table_info('groups')` contains:

```rust
("conversation_kind", "TEXT", "'group'", 1),
("direct_agent_id", "TEXT", "NULL", 0),
("title_source", "TEXT", "'manual'", 1),
```

In `groups.rs`, insert a valid direct container directly through SQL and assert:

```rust
assert!(group_list.as_array().unwrap().iter().all(|group| group["id"] != direct_id));
assert_eq!(get_direct_through_group_api.status(), StatusCode::NOT_FOUND);
assert_eq!(patch_direct_through_group_api.status(), StatusCode::NOT_FOUND);
assert_eq!(delete_direct_through_group_api.status(), StatusCode::NOT_FOUND);
```

- [ ] **Step 2: Run focused backend tests to verify failure**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test sqlite_bootstrap -- --nocapture
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test groups direct_container -- --nocapture
```

Expected: FAIL because the columns and filters do not exist.

- [ ] **Step 3: Add the migration**

Create:

```sql
ALTER TABLE groups
ADD COLUMN conversation_kind TEXT NOT NULL DEFAULT 'group';

ALTER TABLE groups
ADD COLUMN direct_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL;

ALTER TABLE groups
ADD COLUMN title_source TEXT NOT NULL DEFAULT 'manual';

CREATE INDEX ix_groups_owner_kind_activity
ON groups(owner_id, conversation_kind, status, updated_at DESC);

CREATE INDEX ix_groups_direct_agent
ON groups(direct_agent_id)
WHERE conversation_kind = 'direct';
```

- [ ] **Step 4: Add shared conversation-kind access checks**

Create `api/conversations.rs`:

```rust
use sqlx::SqlitePool;

use crate::api::error::ApiError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationKind { Group, Direct }

impl ConversationKind {
    pub const fn as_str(self) -> &'static str {
        match self { Self::Group => "group", Self::Direct => "direct" }
    }
}

pub async fn ensure_active_owned_conversation(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
    expected: ConversationKind,
) -> Result<(), ApiError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM groups WHERE id = ? AND owner_id = ? AND status = 'active' AND conversation_kind = ? LIMIT 1",
    )
    .bind(id)
    .bind(owner_id)
    .bind(expected.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    found.map(|_| ()).ok_or_else(|| ApiError::not_found("conversation not found"))
}

pub async fn ensure_active_owned_workspace_conversation(
    pool: &SqlitePool,
    id: &str,
    owner_id: &str,
) -> Result<(), ApiError> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM groups WHERE id = ? AND owner_id = ? AND status = 'active' AND conversation_kind IN ('group', 'direct') LIMIT 1",
    )
    .bind(id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| ApiError::internal("database error"))?;
    found.map(|_| ()).ok_or_else(|| ApiError::not_found("conversation not found"))
}
```

Export the module from `api/mod.rs`. Change `groups::list` to add `conversation_kind = 'group'`; change `load_active_owned` and every direct group-row lookup used by CRUD, members, Agents, notes, scheduler, and turn management to require the same kind. Workspace file and Git handlers are shared conversation resources: change their preflight calls to `ensure_active_owned_workspace_conversation` so the existing `/groups/:id/workspace-*` routes continue serving both group and direct containers internally. They do not expose either kind in a list and remain ownership-scoped. Add one integration assertion that a direct container can read its copied Agent workspace while its member-management endpoint returns not found.

- [ ] **Step 5: Run migration and group regression tests**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test sqlite_bootstrap -- --nocapture
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test groups -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit typed conversation storage**

```powershell
git add backend-rs/crates/backend/src/db/migrations/0005_direct_chats.sql backend-rs/crates/backend/src/api/conversations.rs backend-rs/crates/backend/src/api/mod.rs backend-rs/crates/backend/src/api/groups.rs backend-rs/crates/backend/tests/groups.rs backend-rs/crates/backend/tests/sqlite_bootstrap.rs
git commit -m "feat(backend): add typed conversation containers"
```

---

### Task 2: Implement direct-chat lifecycle APIs

**Files:**
- Create: `backend-rs/crates/backend/src/api/direct_chats.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Create: `backend-rs/crates/backend/tests/direct_chats.rs`

**Interfaces:**
- Produces: `POST /api/v2/direct-chats` body `{ agent_id: string }`.
- Produces: `GET /api/v2/direct-chats`, `GET/PATCH/DELETE /api/v2/direct-chats/:chat_id`.
- Produces: `DirectChatResponse { id, title, title_source, agent_id, agent_name, agent_status, workspace_id, status, created_at, updated_at }`.
- PATCH consumes `{ title: string }` and always sets `title_source = 'manual'`.

- [ ] **Step 1: Write failing lifecycle tests**

Cover these concrete cases in `direct_chats.rs`:

- creating two chats for one owned active Agent returns two different IDs;
- both rows have one owner membership, one active matching `group_agents` row, and one thread;
- chat workspace equals the Agent workspace;
- list orders by `updated_at DESC` and does not include normal groups;
- another user's Agent returns not found;
- inactive Agent returns conflict;
- GET/PATCH/DELETE are owner-only and reject a normal group ID as not found;
- PATCH trims the title, rejects empty or over-120-scalar titles, and returns `title_source = manual`;
- DELETE sets status to deleted and subsequent GET returns not found.

- [ ] **Step 2: Run the direct-chat test to verify missing routes**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test direct_chats -- --nocapture
```

Expected: FAIL with 404 for `/api/v2/direct-chats`.

- [ ] **Step 3: Implement response/request types and queries**

Use these wire types:

```rust
#[derive(Debug, Deserialize)]
pub struct CreateRequest { agent_id: String }

#[derive(Debug, Deserialize)]
pub struct UpdateRequest { title: String }

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DirectChatResponse {
    id: String,
    title: String,
    title_source: String,
    agent_id: Option<String>,
    agent_name: Option<String>,
    agent_status: Option<String>,
    workspace_id: Option<String>,
    status: String,
    created_at: String,
    updated_at: String,
}
```

The shared SELECT left-joins `agents a ON a.id = g.direct_agent_id`, filters `g.conversation_kind = 'direct'`, and never filters the joined Agent by status so unavailable history remains readable.

- [ ] **Step 4: Implement atomic creation**

Validate the UUID and query the owned Agent's `name`, `workspace_id`, and `status`. Read account language from `system_settings` with fallback `en-US`; store `New chat with {agent}` for English or `与 {agent} 的新对话` for Chinese. In one SQL transaction insert:

1. `groups` with `conversation_kind='direct'`, `direct_agent_id`, `title_source='automatic'`, `free_speech=1`, `proactive_mode=0`, `scheduler_enabled=0`, and copied `workspace_id`;
2. the owner `group_members` row;
3. one `group_agents` row with `response_mode='default'` and `context_scope_json='{"share_group_workspace":true}'`;
4. one active `threads` row with `next_seq=1`.

Commit only after all inserts succeed. Do not create a new workspace directory.

- [ ] **Step 5: Register routes and run lifecycle tests**

Add:

```rust
.route("/api/v2/direct-chats", post(direct_chats::create).get(direct_chats::list))
.route(
    "/api/v2/direct-chats/:chat_id",
    get(direct_chats::get).patch(direct_chats::update).delete(direct_chats::delete),
)
```

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test direct_chats -- --nocapture
```

Expected: lifecycle tests PASS.

- [ ] **Step 6: Commit direct-chat lifecycle APIs**

```powershell
git add backend-rs/crates/backend/src/api/direct_chats.rs backend-rs/crates/backend/src/api/mod.rs backend-rs/crates/backend/tests/direct_chats.rs
git commit -m "feat(backend): add direct chat lifecycle"
```

---

### Task 3: Add kind-safe direct messaging and automatic activity/title events

**Files:**
- Modify: `backend-rs/crates/domain/src/events.rs`
- Modify: `backend-rs/crates/backend/src/api/sse_replay.rs`
- Modify: `backend-rs/crates/backend/src/api/messages.rs`
- Modify: `backend-rs/crates/backend/src/api/mod.rs`
- Modify: `backend-rs/crates/backend/src/runtime/group.rs`
- Modify: `backend-rs/crates/backend/tests/direct_chats.rs`
- Modify: `backend-rs/crates/backend/tests/group_stream.rs`

**Interfaces:**
- Produces: `StreamEventKind::ConversationUpdated` serialized as `conversation_updated`.
- Event payload: `{ conversation_id, title, title_source, updated_at }`.
- Produces: `GET/POST /api/v2/direct-chats/:chat_id/messages`, `POST /api/v2/direct-chats/:chat_id/messages/clear`, `DELETE /api/v2/direct-chats/:chat_id/messages/:message_id`, and `POST /api/v2/direct-chats/:chat_id/messages/stream`.
- Preserves: existing group message routes and wire payloads.

- [ ] **Step 1: Add failing direct stream, title, and replay tests**

Create an active direct chat through the API, send `"   Plan   the launch 🚀 now   "`, drain SSE, and assert:

```rust
assert_eq!(agent_message_count, 1);
assert_eq!(responding_agent_id, bound_agent_id);
assert_eq!(conversation_update["title"], "Plan the launch 🚀 now");
assert_eq!(conversation_update["title_source"], "automatic");
```

Send a 40-scalar Unicode message and assert exactly 32 scalar values. Rename the chat, send again, and assert the manual title is unchanged. Assert `updated_at` advances on every successful user message. Reconnect with `Last-Event-ID` before `conversation_updated` and assert replay includes it. Disable the Agent, verify history GET succeeds, and verify new send returns conflict before SSE opens. Verify direct endpoints reject group IDs and group message endpoints reject direct IDs.

- [ ] **Step 2: Run focused tests to verify missing message routes/event**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test direct_chats direct_message -- --nocapture
```

Expected: FAIL with missing routes/event kind.

- [ ] **Step 3: Generalize message handlers by expected conversation kind**

Keep Axum-facing wrappers small:

```rust
pub async fn list_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    list_for_kind(state, headers, group_id, query, ConversationKind::Group).await
}

pub async fn list_direct(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(group_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, ApiError> {
    list_for_kind(state, headers, group_id, query, ConversationKind::Direct).await
}
```

Apply the same wrapper/private-function pattern to `send`, `clear`, `delete`, and `stream`. Replace `ensure_active_owned_group` inside each private implementation with `ensure_active_owned_conversation(state.db.pool(), &conversation_id, &owner_id, expected_kind)`. For direct send/stream preflight, additionally require an active owned Agent matching `groups.direct_agent_id`; return conflict `direct chat agent is unavailable` when the chat remains valid but the Agent is missing/inactive. Rename routes in `api/mod.rs` to the group wrappers and register the four exact direct message routes listed in this task's Interfaces block.

- [ ] **Step 4: Persist direct activity and first-message title**

Add `ConversationUpdated` to the domain enum and SSE wire parser. In `runtime/group.rs`, immediately after `UserMessage` is durably persisted, acquire the shared write lock and run a transaction that:

1. counts all user messages for the conversation, including non-visible statuses;
2. normalizes whitespace with `split_whitespace().join(" ")`;
3. derives at most 32 `chars()` only when the count is exactly one, normalized content is non-empty, kind is direct, and `title_source='automatic'`;
4. updates `updated_at` on every direct-chat user message;
5. updates `name` only in the first-message case;
6. returns current `name`, `title_source`, and `updated_at`.

Emit and persist `ConversationUpdated` before Agent selection. Keep direct containers `free_speech=true`, so the existing selector returns their sole active Agent. Treat an invariant violation with zero or multiple direct candidates as a runtime error and test the zero-Agent case through preflight.

- [ ] **Step 5: Run direct and group stream tests**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test direct_chats -- --nocapture
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend --test group_stream -- --nocapture
```

Expected: PASS; existing group streams contain no `conversation_updated` event.

- [ ] **Step 6: Commit direct messaging**

```powershell
git add backend-rs/crates/domain/src/events.rs backend-rs/crates/backend/src/api/sse_replay.rs backend-rs/crates/backend/src/api/messages.rs backend-rs/crates/backend/src/api/mod.rs backend-rs/crates/backend/src/runtime/group.rs backend-rs/crates/backend/tests/direct_chats.rs backend-rs/crates/backend/tests/group_stream.rs
git commit -m "feat(chat): stream direct agent conversations"
```

---

### Task 4: Add frontend direct-chat contracts and server-state hooks

**Files:**
- Modify: `frontend/src/types/api.ts`
- Modify: `frontend/src/lib/api-v2/types.ts`
- Modify: `frontend/src/lib/api-v2/schemas.ts`
- Modify: `frontend/src/lib/api-v2/schemas.test.ts`
- Create: `frontend/src/hooks/useDirectChats.ts`
- Create: `frontend/src/hooks/useDirectChats.test.tsx`

**Interfaces:**
- Produces: `DirectChatRead`, `DirectChatCreate`, `DirectChatUpdate`, and `ConversationUpdatedPayload`.
- Produces hooks: `useDirectChats()`, `useDirectChat(chatId)`, `useCreateDirectChat()`, `useRenameDirectChat(chatId)`, `useDeleteDirectChat(chatId)`.
- Query keys: `['direct-chats']` and `['direct-chats', chatId]`.

- [ ] **Step 1: Write failing schema and hook tests**

Parse a strict `conversation_updated` event and reject one missing `updated_at`. Mock `fetchJson` to assert create POST body `{ agent_id }`, rename PATCH body `{ title }`, and delete method. For optimistic rename, seed list/detail cache, call mutate, assert both caches change before the promise resolves, reject it, and assert both caches restore.

- [ ] **Step 2: Run focused tests to verify missing types/hooks**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/lib/api-v2/schemas.test.ts src/hooks/useDirectChats.test.tsx
```

Expected: FAIL because direct-chat contracts do not exist.

- [ ] **Step 3: Add exact frontend contracts**

Add:

```ts
export type DirectChatTitleSource = 'automatic' | 'manual'

export interface DirectChatRead {
  id: string
  title: string
  title_source: DirectChatTitleSource
  agent_id: string | null
  agent_name: string | null
  agent_status: string | null
  workspace_id: string | null
  status: string
  created_at: string
  updated_at: string
}

export interface DirectChatCreate { agent_id: string }
export interface DirectChatUpdate { title: string }

export interface ConversationUpdatedPayload {
  conversation_id: string
  title: string
  title_source: DirectChatTitleSource
  updated_at: string
}
```

Add `conversation_updated` to `LegacyStreamEventKind` and a strict Zod payload parser exported as `parseConversationUpdatedEvent`.

- [ ] **Step 4: Implement direct-chat hooks and cache helpers**

Keep cache writes in named pure functions `replaceDirectChatInList` and `sortDirectChatsByActivity`. Create invalidates/navigates only at the component layer; hook create success inserts/sorts the list and seeds detail. Rename uses `onMutate/onError/onSuccess`; delete removes list/detail on success.

- [ ] **Step 5: Run focused tests and type-check**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/lib/api-v2/schemas.test.ts src/hooks/useDirectChats.test.tsx
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS.

- [ ] **Step 6: Commit frontend direct-chat contracts**

```powershell
git add frontend/src/types/api.ts frontend/src/lib/api-v2/types.ts frontend/src/lib/api-v2/schemas.ts frontend/src/lib/api-v2/schemas.test.ts frontend/src/hooks/useDirectChats.ts frontend/src/hooks/useDirectChats.test.tsx
git commit -m "feat(frontend): add direct chat data hooks"
```

---

### Task 5: Generalize message transport and extract the shared chat view

**Files:**
- Modify: `frontend/src/hooks/useGroupMessages.ts`
- Modify: `frontend/src/hooks/useSendMessageStream.ts`
- Modify: `frontend/src/hooks/useSendMessageStream.test.tsx`
- Create: `frontend/src/components/chat/ConversationChatView.tsx`
- Create: `frontend/src/components/chat/ConversationChatView.test.tsx`
- Modify: `frontend/src/pages/group/GroupChatPage.tsx`
- Modify: `frontend/src/components/chat/Composer.tsx`
- Modify: `frontend/src/components/chat/MessageList.tsx`

**Interfaces:**
- Produces: `type ConversationScope = 'groups' | 'direct-chats'`.
- Produces: `conversationMessagesKey(scope, id)` and `conversationApiPath(scope, id)`.
- Changes: `useConversationMessages(scope, id)`; keep `useGroupMessages(id)` as a wrapper.
- Changes: `useSendMessageStream(conversationId, schedulerEnabled, options?)`, where options contains `scope` and `onConversationUpdated`.
- Produces: `ConversationChatView` with explicit capabilities.

- [ ] **Step 1: Add failing scope and capability tests**

Extend stream-hook tests to assert a direct send opens `/direct-chats/chat-1/messages/stream`, consumes `conversation_updated`, and calls the supplied cache callback without adding a visible message. Test `ConversationChatView` with:

```ts
const directCapabilities = {
  showAnnouncement: false,
  showManage: false,
  showTurnTrace: false,
  showWorkspace: true,
  allowMentions: false,
}
```

Assert the direct canvas omits Manage Group and turn-trace actions while retaining composer, stop, resume, Markdown, and workspace toggle.

- [ ] **Step 2: Run focused tests to verify group-only assumptions fail**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/hooks/useSendMessageStream.test.tsx src/components/chat/ConversationChatView.test.tsx
```

Expected: FAIL because URLs/query keys and view capabilities are group-specific.

- [ ] **Step 3: Generalize history and stream URLs without renaming durable store fields**

Keep `Message.group_id` and Zustand's group-keyed maps unchanged as internal conversation IDs. Add:

```ts
export type ConversationScope = 'groups' | 'direct-chats'
export const conversationMessagesKey = (scope: ConversationScope, id: string) =>
  [scope, id, 'messages'] as const
export const conversationApiPath = (scope: ConversationScope, id: string) =>
  `/${scope}/${id}`
```

Make history, clear, delete, and stream hooks accept scope. On `conversation_updated`, parse the event and invoke `onConversationUpdated(payload)`; do not append it to the message store. Invalidate the scope-specific list/detail and message keys.

- [ ] **Step 4: Extract `ConversationChatView`**

Move the message list, stream error, composer, workspace toggle/panel, resize behavior, file-link open behavior, and optional trace drawer from `GroupChatPage`. Use this prop contract:

```ts
interface ConversationChatViewProps {
  conversationId: string
  scope: ConversationScope
  schedulerEnabled: boolean
  agents: GroupAgentRead[]
  title: React.ReactNode
  subtitle?: React.ReactNode
  announcement?: string | null
  headerActions?: React.ReactNode
  capabilities: {
    showAnnouncement: boolean
    showManage: boolean
    showTurnTrace: boolean
    showWorkspace: boolean
    allowMentions: boolean
  }
  onConversationUpdated?: (payload: ConversationUpdatedPayload) => void
  disabledComposerReason?: string
}
```

`GroupChatPage` loads group/group-agents as before and supplies all group capabilities as true. Direct mode later supplies one adapted `GroupAgentRead` and no management/trace. Keep workspace hooks on the existing internal conversation ID in this task; Task 7 connects the direct page.

- [ ] **Step 5: Run group chat tests and type-check**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/hooks/useSendMessageStream.test.tsx src/components/chat src/pages/group
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS with unchanged group behavior.

- [ ] **Step 6: Commit shared conversation transport/view**

```powershell
git add frontend/src/hooks/useGroupMessages.ts frontend/src/hooks/useSendMessageStream.ts frontend/src/hooks/useSendMessageStream.test.tsx frontend/src/components/chat frontend/src/pages/group/GroupChatPage.tsx
git commit -m "refactor(frontend): share chat view across conversation modes"
```

---

### Task 6: Add the Agent picker and direct-chat sidebar section

**Files:**
- Create: `frontend/src/components/direct-chats/DirectChatPickerDialog.tsx`
- Create: `frontend/src/components/direct-chats/DirectChatPickerDialog.test.tsx`
- Modify: `frontend/src/components/layout/AppSidebar.tsx`
- Modify: `frontend/src/components/layout/AppLayout.test.tsx`
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`

**Interfaces:**
- Consumes: `useAgents()`, `useCreateDirectChat()`, `useDirectChats()`, `formatRelativeTime()`.
- Produces: picker prop `{ open, onOpenChange }`; successful selection navigates to `/chats/:id`.
- Produces: sidebar sections Direct Chats then Groups, with shared query filtering but separate headings.

- [ ] **Step 1: Write failing picker/sidebar tests**

Test searchable active-Agent rows, empty/loading/error states, disabled submit while creating, success navigation, and API error retention in the open dialog. In `AppLayout.test.tsx`, assert Direct Chats precedes Groups, direct rows link to `/chats/:id`, both sections filter from one query, and the collapsed add control has a localized tooltip.

- [ ] **Step 2: Run focused tests to verify missing UI**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/direct-chats/DirectChatPickerDialog.test.tsx src/components/layout/AppLayout.test.tsx
```

Expected: FAIL because the picker/section do not exist.

- [ ] **Step 3: Add direct-chat translation keys**

Add under `navigation.directChats` and `chat.direct`:

```ts
navigation: {
  directChats: 'Direct Chats', newDirectChat: 'New direct chat', searchConversations: 'Search conversations',
},
chat: {
  direct: {
    pickerTitle: 'Start a direct chat', pickerDescription: 'Choose an Agent for a new independent conversation.',
    searchAgents: 'Search Agents', noAgents: 'No active Agents available.', creating: 'Creating chat…',
  },
},
```

Chinese values are `私聊`, `新建私聊`, `搜索会话`, `开始私聊`, `选择一个 Agent 创建独立会话。`, `搜索 Agent`, `没有可用的 Agent。`, and `正在创建会话…`.

- [ ] **Step 4: Implement picker and sidebar sections**

Filter Agents by `status === 'active'` and case-insensitive name/description. Create immediately on row selection and navigate after success. In the sidebar, replace the group-only query label with the shared localized label, render direct chats sorted by hook data, then render filtered groups. Use `updated_at` for direct relative time and existing group timestamps for groups. Preserve collapsed layout width and keyboard-accessible controls.

- [ ] **Step 5: Run picker/sidebar tests**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/direct-chats/DirectChatPickerDialog.test.tsx src/components/layout/AppLayout.test.tsx src/i18n/index.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit direct-chat navigation**

```powershell
git add frontend/src/components/direct-chats frontend/src/components/layout/AppSidebar.tsx frontend/src/components/layout/AppLayout.test.tsx frontend/src/i18n
git commit -m "feat(frontend): add direct chat navigation"
```

---

### Task 7: Build the direct-chat page, editable title, and home integration

**Files:**
- Create: `frontend/src/components/direct-chats/EditableDirectChatTitle.tsx`
- Create: `frontend/src/components/direct-chats/EditableDirectChatTitle.test.tsx`
- Create: `frontend/src/pages/chat/DirectChatPage.tsx`
- Create: `frontend/src/pages/chat/DirectChatPage.test.tsx`
- Modify: `frontend/src/pages/home/ChatHomePage.tsx`
- Create: `frontend/src/pages/home/ChatHomePage.test.tsx`
- Modify: `frontend/src/routes.tsx`
- Modify: `frontend/src/i18n/resources/en-US.ts`
- Modify: `frontend/src/i18n/resources/zh-CN.ts`

**Interfaces:**
- Consumes: direct-chat hooks and `ConversationChatView`.
- Produces: `/chats/:chatId` route.
- Produces: inline title edit with Enter save, Escape cancel, blur save, max 120 Unicode scalars.
- Produces: recent-conversation union sorted by activity with `direct | group` type labels.

- [ ] **Step 1: Write failing title/page/home tests**

Test keyboard title editing and optimistic rollback. Page tests cover loading, not found, one adapted Agent, missing/deleted Agent history with disabled composer, `conversation_updated` cache refresh, and omission of group-only controls. Home tests assert both New Direct Chat and New Group actions and a mixed recent list ordered by activity with localized badges.

- [ ] **Step 2: Run focused tests to verify missing route/page**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/direct-chats/EditableDirectChatTitle.test.tsx src/pages/chat/DirectChatPage.test.tsx src/pages/home/ChatHomePage.test.tsx
```

Expected: FAIL because the title/page tests reference missing components; create `ChatHomePage.test.tsx` if it does not already exist.

- [ ] **Step 3: Implement accessible inline rename**

Render a button-like heading in view mode and an input in edit mode. Trim on save, keep edit mode for empty/over-120-scalar titles, call `useRenameDirectChat`, and expose localized validation/error text through `aria-describedby`. On rejected mutation, the hook restores cache and the component announces the localized failure in a `role="alert"` region.

- [ ] **Step 4: Implement `DirectChatPage` and route**

Load `useDirectChat(chatId)`. Adapt the response to the minimum `GroupAgentRead` required by Composer:

```ts
const agent = chat.agent_id && chat.agent_name ? [{
  id: `${chat.id}:${chat.agent_id}`,
  group_id: chat.id,
  agent_id: chat.agent_id,
  display_name: chat.agent_name,
  role: null,
  topology_role: null,
  speaking_order: 1,
  response_mode: 'default',
  share_group_workspace: true,
  context_usage: null,
  status: chat.agent_status ?? 'deleted',
  joined_at: chat.created_at,
}] : []
```

Pass `scope='direct-chats'`, `schedulerEnabled={false}`, direct capabilities, editable title, Agent subtitle, and a localized unavailable reason. On `conversation_updated`, patch both direct detail/list caches through the helper from Task 4. Register `{ path: '/chats/:chatId', element: <DirectChatPage /> }`.

- [ ] **Step 5: Integrate mixed recents on home**

Use `useGroups()` and `useDirectChats()`, normalize to `{ id, kind, title, subtitle, updatedAt, to }`, sort descending, and take five. `GroupRead` must expose backend `updated_at`; add it to the frontend type and backend response if absent. Use `created_at` only as a fallback for legacy group responses.

- [ ] **Step 6: Run page/home tests and full frontend tests**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- src/components/direct-chats src/pages/chat src/pages/home
pnpm --filter @ag-swarmer/frontend test
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: PASS.

- [ ] **Step 7: Commit the direct-chat experience**

```powershell
git add frontend/src/components/direct-chats frontend/src/pages/chat frontend/src/pages/home frontend/src/routes.tsx frontend/src/types/api.ts frontend/src/i18n
git commit -m "feat(frontend): add one-to-one agent chat"
```

---

### Task 8: Verify unavailable Agents, isolation, replay, and complete regression suite

**Files:**
- Modify: `backend-rs/crates/backend/tests/direct_chats.rs`
- Modify: `frontend/src/pages/chat/DirectChatPage.test.tsx`
- Modify: documentation only if an implemented endpoint differs from this plan's exact route contract.

**Interfaces:**
- Verifies all earlier contracts; produces no new public interface.

- [ ] **Step 1: Complete end-to-end API invariants in integration tests**

Ensure the integration suite explicitly proves: two sessions for one Agent have different threads and context histories; a direct chat cannot acquire a second Agent through group endpoints; group scheduler/member/note endpoints reject the direct ID; direct lifecycle/message endpoints reject a group ID; deleted Agent history remains readable; stopped streams resume through the existing thread route; and `Last-Event-ID` replays `conversation_updated` plus later events exactly once.

- [ ] **Step 2: Run backend formatting and complete backend tests**

Run:

```powershell
cargo fmt --manifest-path backend-rs/Cargo.toml --check
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-domain
cargo test --manifest-path backend-rs/Cargo.toml --package ag-swarmer-backend
```

Expected: PASS.

- [ ] **Step 3: Run frontend quality gates**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test
pnpm --filter @ag-swarmer/frontend lint
pnpm --filter @ag-swarmer/frontend type-check
pnpm --filter @ag-swarmer/frontend build
```

Expected: PASS.

- [ ] **Step 4: Manually smoke-test the two key flows**

Run the app with `pnpm dev`, then verify:

1. Create two direct chats with one Agent; send different first messages; confirm separate titles and histories after reload.
2. Rename one chat; send again; confirm the manual title stays fixed and the row moves to the top by activity.
3. Disable the Agent; reload; confirm history is readable and composer is disabled.
4. Open an existing group; confirm members, settings, mentions, workspace, streaming, stop, and resume still work.

- [ ] **Step 5: Commit final regression additions**

```powershell
git add backend-rs/crates/backend/tests/direct_chats.rs frontend/src/pages/chat/DirectChatPage.test.tsx
git commit -m "test: cover direct chat regression boundaries"
```
