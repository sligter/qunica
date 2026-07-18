# Internationalization and Direct Chat Design

## Summary

AG Swarmer will support Chinese (`zh-CN`) and English (`en-US`) throughout the frontend and add a first-class one-to-one chat mode. Language is an account-level preference. Direct chats appear separately from groups, allow multiple independent conversations with the same Agent, and reuse the existing thread, message, SSE, interruption, and recovery infrastructure.

## Goals

- Translate all frontend interface copy, accessibility labels, frontend validation messages, known error messages, page titles, empty states, dates, relative times, and number formatting into Chinese and English.
- Persist the selected language in account-level system settings and apply it across browser and desktop clients.
- Add a dedicated direct-chat section to the sidebar with an Agent picker.
- Allow a user to create multiple independent direct chats with the same Agent.
- Generate a useful title after the first message and allow the user to rename it permanently.
- Preserve the existing group-chat behavior and reuse its stable streaming and persistence paths.

## Non-goals

- Translating Agent-generated content or forcing an Agent to reply in the interface language.
- Machine-translating arbitrary backend diagnostic text.
- Supporting locales other than `zh-CN` and `en-US` in this release.
- Adding group collaboration controls, group notes, member management, or scheduler configuration to direct chats.
- Adding multiple users to a direct chat.

## Architecture

### Typed conversation containers

The existing `groups -> threads -> messages -> stream_events` path remains the durable conversation backbone. The `groups` table gains:

- `conversation_kind TEXT NOT NULL DEFAULT 'group'`, constrained by application validation to `group` or `direct`.
- `direct_agent_id TEXT NULL REFERENCES agents(id) ON DELETE SET NULL`.
- `title_source TEXT NOT NULL DEFAULT 'manual'`, constrained by application validation to `automatic` or `manual`.

Existing records migrate as `conversation_kind = 'group'`, `direct_agent_id = NULL`, and `title_source = 'manual'`. A direct-chat container has `conversation_kind = 'direct'`, exactly one active `group_agents` row matching `direct_agent_id`, and an initial thread. It inherits the selected Agent's workspace association at creation time. The Agent's configured model, runtime, Skills, Tools, and Agent workspace behavior remain authoritative.

The backend exposes a dedicated `/api/v2/direct-chats` API for listing, creating, reading, renaming, and soft-deleting direct chats. Direct-chat message and SSE endpoints delegate to the existing conversation runtime after verifying ownership, active status, conversation kind, and the bound Agent. Group administration endpoints reject direct-chat IDs as not found; direct-chat endpoints likewise reject group IDs as not found.

This typed-container approach avoids duplicating message persistence, streaming, replay, pause, resume, and tool execution while keeping product-facing APIs and navigation explicit.

### Language infrastructure

The frontend uses `i18next` and `react-i18next`. Resources are split into these namespaces:

- `common`
- `auth`
- `navigation`
- `chat`
- `agents`
- `groups`
- `providers`
- `skills`
- `workspaces`
- `settings`

Components use semantic keys rather than visible English strings as identifiers. The two locale trees must have identical keys. Development builds warn about missing keys; production falls back to `en-US` and never displays raw translation keys.

The `system_settings` table gains `language TEXT NOT NULL DEFAULT 'en-US'`. The settings API accepts only `zh-CN` and `en-US`. The server value is the source of truth after authentication. A local mirror is updated after a successful settings read or write and is used only during bootstrap and on unauthenticated screens to prevent a visible language flash. If neither value exists, the app selects `zh-CN` for a Chinese browser locale and `en-US` otherwise.

Date, relative-time, and number formatting live behind locale-aware utility functions. Agent output is rendered unchanged.

## Direct-chat lifecycle

### Creation

The user clicks the add control in the sidebar's Direct Chats section and chooses an active Agent from a searchable picker. In one database transaction, the backend:

1. Confirms the Agent is active and owned by the current user.
2. Creates a `direct` conversation container with `title_source = 'automatic'`.
3. Copies the Agent's current `workspace_id` into the conversation container.
4. Adds the current user as the owner member.
5. Adds exactly the selected Agent with a response mode that always targets it.
6. Creates the initial thread.

The initial localized display title is derived in the frontend as "New chat with {Agent}" or "与 {Agent} 的新对话" while the stored title contains the same default in the user's current account language. The API returns the created chat and the frontend navigates to `/chats/:chatId`.

### Messages and title generation

Messages are sent through the direct-chat SSE endpoint. Before invoking the shared runtime, the handler verifies that the chat is direct, belongs to the caller, and still has its bound active Agent. The runtime schedules only that Agent.

Every successfully persisted user message updates the conversation's `updated_at`, which drives sidebar ordering. When the first user message is persisted and `title_source` is still `automatic`, the backend trims and collapses whitespace and takes the first 32 Unicode scalar values as the stored title. If the normalized message is empty, the default title is retained. The title update is emitted to the client so the header and sidebar update without a reload.

Renaming a chat sets `title_source = 'manual'`. Automatic title generation never overwrites a manual title. Rename uses an optimistic frontend update and restores the prior title if the request fails.

### Agent availability and deletion

If the bound Agent is disabled or deleted, the direct chat and its history remain readable. The UI marks the Agent unavailable and disables message submission. Since `direct_agent_id` uses `ON DELETE SET NULL`, historical conversations remain intact even if the Agent row is removed.

Deleting a direct chat uses the existing soft-delete convention. The chat is removed from user-facing lists and cannot be read or messaged through normal APIs, while its durable records remain available for operational recovery.

## User experience

### Sidebar and navigation

The expanded sidebar contains separate Direct Chats and Groups sections. Direct Chats appears above Groups and has an add button in its section header. A direct-chat row shows the Agent avatar, conversation title, and localized recent-activity time. Rows are ordered by `updated_at` descending.

The sidebar search input filters both sections while retaining their headings. The collapsed sidebar exposes a localized tooltip for starting a direct chat. The route `/chats/:chatId` renders the direct-chat page; `/groups/:groupId` remains unchanged for group chats.

### Direct-chat page

The page reuses existing message bubbles, Markdown rendering, streaming indicators, stop/resume behavior, composer, file references, and relevant workspace tools. Its header shows the Agent identity and editable conversation title. It does not show group members, group notes, scheduler settings, group announcements, collaborative dispatch DAGs, or other multi-Agent controls.

Shared chat components receive an explicit capabilities object rather than inferring mode from scattered route checks. Group chat enables group-only panels; direct chat disables them and fixes the response target to its bound Agent.

### Home page

The home page provides separate New Direct Chat and New Group buttons. Recent conversations combine both kinds, sort by activity, and show a localized type indicator so users can distinguish direct chats from groups.

### Language control

System Settings places a Chinese / English segmented control in the Appearance section. Selecting a language updates the visible interface immediately and persists the account setting. If persistence fails, the UI returns to the last server-confirmed locale and shows a localized retryable error.

The translated surface includes authenticated and unauthenticated routes, navigation, dialogs, forms, validation, tooltips, accessibility text, page titles, empty/loading/error states, and known frontend error mappings. Raw Agent messages and unknown backend diagnostics remain unchanged; an unknown backend error is accompanied by a localized generic explanation and retry action.

## Data flow and consistency

- Direct-chat creation is atomic; a partial container, member, Agent binding, or thread is never visible.
- The backend owns conversation-kind validation and bound-Agent invariants.
- Query-cache entries for lists, detail, title, and recent conversations are updated or invalidated after create, rename, delete, message activity, and Agent availability changes.
- SSE replay and thread resume use the existing durable event cursor and thread ownership checks, with an added conversation-kind guard at the direct-chat boundary.
- Language changes optimistically update i18next. A failed save restores both i18next and the local mirror to the prior server-confirmed language.
- The local language mirror never overwrites a successfully loaded account preference.

## Error handling

- Ownership failures and conversation-kind mismatches return not found to avoid leaking object existence.
- Invalid direct-chat titles, unsupported languages, empty messages, and invalid Agent IDs return structured validation errors that the frontend maps to localized copy.
- An unavailable Agent produces a conflict response and a persistent disabled-composer state after refresh.
- Network and SSE failures use the existing retry and replay behavior. Known cases receive localized user-facing messages; technical detail remains available to logs.
- Missing translation keys warn in development and fall back to `en-US` in production.

## Module boundaries

### Backend

- Database migrations add conversation metadata and account language.
- The direct-chat API owns lifecycle, authorization, title generation, and Agent availability rules.
- Shared conversation runtime owns message processing, persistence, streaming, replay, and interruption recovery.
- Group APIs continue to own group collaboration settings and exclude direct chats.

### Frontend

- i18n bootstrap and resources own locale selection and translations.
- Locale-format utilities own dates, relative time, and numbers.
- Direct-chat hooks own server-state access and cache synchronization.
- The Agent picker and direct-chat sidebar own creation and navigation.
- Shared chat presentation receives explicit capabilities; direct-chat and group pages configure those capabilities.

## Testing and acceptance criteria

### Backend tests

- Migration tests prove existing groups remain groups and all new defaults are valid.
- Settings tests accept only `zh-CN` and `en-US` and prove preferences are isolated per account.
- Direct-chat API tests cover multiple chats for one Agent, creation atomicity, ownership, type mismatch, rename, soft delete, and ordering by message activity.
- Title tests cover whitespace normalization, empty content, 32-character Unicode truncation, and protection of manual titles.
- Agent lifecycle tests prove history remains readable and sending is rejected after the Agent becomes unavailable.
- Runtime tests prove only the bound Agent responds and SSE replay, interruption, and resume continue to work.
- Existing group-chat tests continue to pass without changed behavior.

### Frontend tests

- A resource-parity test recursively compares Chinese and English translation keys.
- Locale initialization tests cover authenticated server preference, local bootstrap mirror, browser fallback, and English production fallback.
- Settings tests cover immediate switching, successful persistence, and rollback after failure.
- Component tests cover localized sidebar sections, Agent selection, creating multiple chats for one Agent, initial title, first-message title update, manual rename, recent-activity sorting, and unavailable-Agent state.
- Shared-chat tests cover capabilities in both direct and group modes.
- Existing lint, type-check, unit-test, and production-build commands pass.

### User-visible acceptance criteria

- A user can switch the complete interface between Chinese and English, sign in on another client, and receive the same preference.
- A user can create two direct chats with the same Agent and each retains independent message context.
- A first message replaces an automatic title; a manual rename is never overwritten.
- A direct chat invokes exactly its selected Agent and retains existing streaming, stop, replay, and resume behavior.
- Direct chats never appear in group lists or group-management screens, and groups never appear in direct-chat APIs.
- Existing group chats remain functional and visually unchanged except for localized copy and the reorganized sidebar.
