# Direct Workspace Parity and Drag Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give direct chats the same rename, delete, and Git workspace controls as groups, and make right-panel file/folder drops work when the desktop WebView exposes only `text/plain`.

**Architecture:** Keep the existing conversation-scoped read/preview client and the existing group-compatible mutation/Git endpoints. Reuse one Files/Git panel for both conversation scopes, pass the scope into cache invalidation, and add `text/plain` as a verified fallback after structured and operating-system file drops.

**Tech Stack:** React 19, TypeScript, TanStack Query, Vitest/Testing Library, Rust, Axum, SQLx.

## Global Constraints

- Do not add direct-chat external file upload.
- Do not recursively attach directory contents.
- Do not add a drag-and-drop dependency, new state-management layer, or workspace-ID API.
- Preserve current ownership, active-conversation, local-workspace, path, symlink, attachment-count, and metadata validation.
- A directory drop inserts its relative path at the current textarea selection; without focus it appends.
- A drop never creates, moves, copies, renames, or deletes disk content.

---

### Task 1: Share Files/Git and file mutations with direct chats

**Files:**

- Modify: `frontend/src/components/chat/GroupWorkspacePanel.tsx`
- Modify: `frontend/src/components/chat/ConversationChatView.tsx`
- Modify: `frontend/src/components/chat/ConversationWorkspacePanel.test.tsx`
- Modify: `frontend/src/components/chat/ConversationChatView.test.tsx`
- Modify: `frontend/src/components/chat/WorkspaceFilesTab.tsx`
- Modify: `frontend/src/components/chat/WorkspaceFilesTab.test.tsx`
- Modify: `frontend/src/hooks/useGroupFiles.ts`

**Interfaces:**

- Consumes: `ConversationScope = 'groups' | 'direct-chats'`, `conversationWorkspaceFilesQueryKey(scope, conversationId)`, existing `/groups/:id/workspace-files` mutation routes, and existing `/groups/:id/workspace-git/*` routes.
- Produces: `GroupWorkspacePanel` accepting `scope?: ConversationScope`; direct-chat rename/delete actions; scope-correct file-query invalidation.

- [ ] **Step 1: Change the panel tests to require Files/Git for a direct chat**

Replace the direct-only-files assertion in `ConversationWorkspacePanel.test.tsx` with:

```tsx
it('renders Files and Git for direct chats with direct file scope', () => {
  renderWithClient(
    <GroupWorkspacePanel
      scope="direct-chats"
      groupId="chat-1"
      workspaceId="workspace-1"
      width={320}
    />,
  )

  expect(screen.getByRole('tab', { name: 'Files' })).toBeVisible()
  expect(screen.getByRole('tab', { name: 'Git' })).toBeVisible()
  expect(screen.getByText('files:direct-chats:chat-1:workspace-1')).toBeVisible()
  expect(panelMocks.files).toHaveBeenCalledWith(expect.objectContaining({
    scope: 'direct-chats',
    conversationId: 'chat-1',
    workspaceId: 'workspace-1',
  }))
})
```

Remove the now-unused direct `ConversationWorkspacePanel` import from this test file; keep its component mock because `GroupWorkspacePanel` renders it internally.

Update the `ConversationChatView.test.tsx` workspace mock to keep only `GroupWorkspacePanel`, accept `scope`, and assert direct chats call it:

```tsx
const workspacePanelMocks = vi.hoisted(() => ({ group: vi.fn() }))

vi.mock('@/components/chat/GroupWorkspacePanel', () => ({
  GroupWorkspacePanel: (props: {
    groupId: string
    scope?: 'groups' | 'direct-chats'
    workspaceId: string | null
  }) => {
    workspacePanelMocks.group(props)
    return <div>workspace panel</div>
  },
}))
```

Remove the `ConversationWorkspacePanel` mock and every `workspacePanelMocks.conversation` reset/assertion from this test file.

The direct-chat expectation becomes:

```tsx
expect(workspacePanelMocks.group).toHaveBeenCalledWith(expect.objectContaining({
  groupId: 'chat-1',
  scope: 'direct-chats',
  workspaceId: 'workspace-1',
}))
```

- [ ] **Step 2: Run the focused panel tests and confirm they fail**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- ConversationWorkspacePanel ConversationChatView
```

Expected: FAIL because direct chats still render `ConversationWorkspacePanel` without Git and `GroupWorkspacePanel` does not accept or forward `scope`.

- [ ] **Step 3: Make the existing Files/Git panel scope-aware and use it for every conversation**

In `GroupWorkspacePanel.tsx`, add the type and prop:

```tsx
import type { ConversationScope } from '@/types/api'

interface GroupWorkspacePanelProps {
  groupId: string | undefined
  scope?: ConversationScope
  workspaceId?: string | null
  width?: number
  className?: string
  onInsertPaths?: (paths: string[]) => void
}
```

Default and forward it:

```tsx
export function GroupWorkspacePanel({
  groupId,
  scope = 'groups',
  workspaceId = null,
  width,
  className,
  onInsertPaths,
}: GroupWorkspacePanelProps) {
```

Use the scope for Files invalidation and rendering:

```tsx
void queryClient.invalidateQueries({
  queryKey: conversationWorkspaceFilesQueryKey(scope, groupId),
})
```

```tsx
<ConversationWorkspacePanel
  embedded
  scope={scope}
  conversationId={groupId}
  workspaceId={workspaceId}
  onInsertPaths={onInsertPaths}
/>
```

Include `scope` in the navigation effect dependency list.

In `ConversationChatView.tsx`, remove the `ConversationWorkspacePanel` import and replace the scope conditional with:

```tsx
<GroupWorkspacePanel
  scope={scope}
  groupId={conversationId}
  workspaceId={workspaceId}
  width={workspaceFilesPane.width}
  onInsertPaths={insertWorkspacePaths}
/>
```

- [ ] **Step 4: Change file mutation tests to require direct-chat rename/delete while keeping upload hidden**

Replace the direct read-only test in `WorkspaceFilesTab.test.tsx` with:

```tsx
it('allows direct-chat rename and delete while keeping upload unavailable', async () => {
  const user = userEvent.setup()
  renderTab({ scope: 'direct-chats', conversationId: 'chat-1' })

  expect(screen.queryByRole('button', {
    name: 'Upload file to workspace uploads',
  })).toBeNull()
  expect(screen.getByLabelText('Rename README_RAW_原文.md')).toBeVisible()
  expect(screen.getByLabelText('Delete README_RAW_原文.md')).toBeVisible()
  expect(screen.getByLabelText('Download README_RAW_原文.md')).toBeVisible()

  await user.click(screen.getByText('README_RAW_原文.md'))
  expect(screen.getByText('preview:direct-chats:raw dir/README_RAW_原文.md')).toBeVisible()
})
```

- [ ] **Step 5: Run the file-panel test and confirm it fails**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- WorkspaceFilesTab
```

Expected: FAIL because `canMutate` is still restricted to `scope === 'groups'`.

- [ ] **Step 6: Enable direct mutations and invalidate the correct scope**

In `WorkspaceFilesTab.tsx`, keep upload group-only while enabling mutations for any active conversation:

```tsx
const activeConversationId = workspaceId ? conversationId : undefined
const groupId = scope === 'groups' ? activeConversationId : undefined
const hasConversation = Boolean(activeConversationId)
const canUpload = scope === 'groups'
const canMutate = hasConversation
const rename = useRenameGroupWorkspaceFile(activeConversationId, scope)
const del = useDeleteGroupWorkspaceFile(activeConversationId, scope)
```

Change the upload-only render/error conditions from `canMutate` to `canUpload`. Change rename submission to:

```tsx
if (!activeConversationId || !renaming || !renameValue.trim()) return
```

In `useGroupFiles.ts`, import:

```tsx
import { conversationWorkspaceFilesQueryKey } from '@/hooks/useConversationWorkspaceFiles'
import type { ConversationScope } from '@/types/api'
```

Give both mutation hooks a defaulted scope:

```tsx
export function useRenameGroupWorkspaceFile(
  conversationId: string | undefined,
  scope: ConversationScope = 'groups',
) {
```

```tsx
export function useDeleteGroupWorkspaceFile(
  conversationId: string | undefined,
  scope: ConversationScope = 'groups',
) {
```

Use `conversationId` in the URLs and replace their file invalidation with:

```tsx
void qc.invalidateQueries({
  queryKey: conversationWorkspaceFilesQueryKey(scope, conversationId),
})
void qc.invalidateQueries({ queryKey: workspaceGitQueryKey(conversationId) })
```

- [ ] **Step 7: Run focused frontend checks**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- ConversationWorkspacePanel ConversationChatView WorkspaceFilesTab
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: all selected tests PASS and type-check exits `0`.

- [ ] **Step 8: Commit the shared private-chat workspace behavior**

```powershell
git add frontend/src/components/chat/GroupWorkspacePanel.tsx frontend/src/components/chat/ConversationChatView.tsx frontend/src/components/chat/ConversationWorkspacePanel.test.tsx frontend/src/components/chat/ConversationChatView.test.tsx frontend/src/components/chat/WorkspaceFilesTab.tsx frontend/src/components/chat/WorkspaceFilesTab.test.tsx frontend/src/hooks/useGroupFiles.ts
git commit -m "feat(workspace): add direct chat file and git controls"
```

---

### Task 2: Add the WebView `text/plain` drag fallback

**Files:**

- Modify: `frontend/src/components/chat/Composer.tsx`
- Modify: `frontend/src/components/chat/Composer.test.tsx`
- Modify: `frontend/src/components/chat/WorkspaceFilesTab.test.tsx`

**Interfaces:**

- Consumes: `workspacePathsFromDataTransfer(DataTransfer): string[]`, the existing structured workspace item decoder, and `insertWorkspacePaths(paths)`.
- Produces: drop acceptance when `DataTransfer.types` contains only `text/plain`, with server-confirmed file attachment or directory insertion.

- [ ] **Step 1: Add tests reproducing the desktop WebView data shape**

Import `workspacePathsFromDataTransfer` only in production code. In `Composer.test.tsx`, add:

```tsx
function webViewWorkspaceDataTransfer(paths: string[]) {
  const text = paths.join('\n')
  return {
    files: [],
    types: ['text/plain'],
    dropEffect: 'none',
    getData: (type: string) => type === 'text/plain' ? text : '',
  }
}
```

Add a file fallback test:

```tsx
it('attaches a server-confirmed file from a text/plain-only WebView drop', async () => {
  mocks.getFile.mockResolvedValueOnce(workspaceFile('docs/guide.md'))
  mocks.getMetadata.mockResolvedValueOnce(workspaceMetadata('docs/guide.md'))
  render(
    <Composer
      conversationId="chat-1"
      workspaceId="workspace-1"
      scope="direct-chats"
      onSend={vi.fn()}
    />,
  )
  const dropZone = screen.getByRole('group', {
    name: 'Message composer file drop area',
  })
  const dataTransfer = webViewWorkspaceDataTransfer(['docs/guide.md'])

  fireEvent.dragOver(dropZone, { dataTransfer })
  fireEvent.drop(dropZone, { dataTransfer })

  expect(await screen.findByText('guide.md')).toBeVisible()
  expect(mocks.getFile).toHaveBeenCalledWith(
    'direct-chats',
    'chat-1',
    'docs/guide.md',
    null,
  )
})
```

Add a directory fallback test:

```tsx
it('inserts a server-confirmed directory from a text/plain-only WebView drop', async () => {
  const user = userEvent.setup()
  mocks.getFile.mockResolvedValueOnce(workspaceFile('docs', true))
  render(
    <Composer
      conversationId="group-1"
      workspaceId="workspace-1"
      scope="groups"
      onSend={vi.fn()}
    />,
  )
  const textarea = screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement
  await user.type(textarea, 'open OLD now')
  textarea.setSelectionRange(5, 8)

  fireEvent.drop(
    screen.getByRole('group', { name: 'Message composer file drop area' }),
    { dataTransfer: webViewWorkspaceDataTransfer(['docs']) },
  )

  await waitFor(() => expect(textarea).toHaveValue('open docs now'))
})
```

Extend the drag-source test in `WorkspaceFilesTab.test.tsx`:

```tsx
expect(setData).toHaveBeenCalledWith('text/plain', rawFile.path)
```

- [ ] **Step 2: Run the focused tests and confirm the WebView cases fail**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- Composer WorkspaceFilesTab
```

Expected: the new Composer tests FAIL because `text/plain` is neither recognized during `dragover` nor consumed during `drop`.

- [ ] **Step 3: Reuse the existing path decoder after safer drop forms**

In `Composer.tsx`, import:

```tsx
import {
  WORKSPACE_ITEM_MIME,
  workspaceItemsFromDataTransfer,
  workspacePathsFromDataTransfer,
} from '@/lib/workspaceDrag'
```

Allow the target to opt into the WebView fallback:

```tsx
function isRecognizedDrop(dataTransfer: DataTransfer): boolean {
  const types = Array.from(dataTransfer.types)
  return (dataTransfer.files?.length ?? 0) > 0
    || types.includes('Files')
    || types.includes(WORKSPACE_ITEM_MIME)
    || types.includes('text/plain')
}
```

In `handleDrop`, keep structured workspace items first and operating-system files second. Before the unsupported notice, add:

```tsx
const fallbackPaths = workspacePathsFromDataTransfer(event.dataTransfer)
if (fallbackPaths.length > 0) {
  insertWorkspacePaths(fallbackPaths)
  return
}
```

This routes every fallback path through the existing current-conversation lookup, so the client does not trust the text value as file metadata or a directory type.

- [ ] **Step 4: Run focused frontend checks**

Run:

```powershell
pnpm --filter @ag-swarmer/frontend test -- Composer WorkspaceFilesTab workspaceDrag
pnpm --filter @ag-swarmer/frontend type-check
```

Expected: all selected tests PASS and type-check exits `0`.

- [ ] **Step 5: Commit the fallback**

```powershell
git add frontend/src/components/chat/Composer.tsx frontend/src/components/chat/Composer.test.tsx frontend/src/components/chat/WorkspaceFilesTab.test.tsx
git commit -m "fix(chat): accept WebView workspace path drops"
```

---

### Task 3: Lock the direct-chat backend compatibility and run delivery gates

**Files:**

- Modify: `backend-rs/crates/backend/tests/direct_chats.rs`

**Interfaces:**

- Consumes: group-compatible file mutation and Git routes with a direct-chat conversation ID.
- Produces: an end-to-end regression proving direct-chat rename, delete, Git initialization, and owner checks.

- [ ] **Step 1: Add one direct-chat mutation/Git regression test**

Append to `backend-rs/crates/backend/tests/direct_chats.rs`:

```rust
#[tokio::test]
async fn direct_workspace_supports_file_mutations_and_git_through_shared_routes() {
    let (app, _state) = router_with_state_for_tests().await;
    let token = register(&app, "direct-workspace-mutations@example.com").await;
    let (root, workspace_id) =
        create_local_workspace(&app, &token, "Direct Mutations Workspace").await;
    std::fs::write(root.path().join("before.txt"), b"before").unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let agent_id = create_agent(&app, &token, &workspace_id, "Local Agent").await;
    let chat = create_chat(&app, &token, &agent_id).await;
    let chat_id = chat["id"].as_str().unwrap();

    let (status, renamed) = send(
        &app,
        request(
            "PATCH",
            &format!(
                "/api/v2/groups/{chat_id}/workspace-files/rename?path=before.txt"
            ),
            Some(&token),
            json!({"new_path": "after.txt"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {renamed:?}");
    assert_eq!(renamed["path"], "after.txt");
    assert!(root.path().join("after.txt").is_file());

    let (status, body) = send(
        &app,
        authed(
            "DELETE",
            &format!("/api/v2/groups/{chat_id}/workspace-files?path=empty"),
            &token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "body: {body:?}");
    assert!(!root.path().join("empty").exists());

    let (status, initialized) = send(
        &app,
        request(
            "POST",
            &format!("/api/v2/groups/{chat_id}/workspace-git/init"),
            Some(&token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {initialized:?}");
    assert_eq!(initialized["available"], true);

    let foreign_token = register(&app, "direct-workspace-foreign@example.com").await;
    let (status, body) = send(
        &app,
        authed(
            "GET",
            &format!("/api/v2/groups/{chat_id}/workspace-git/status"),
            &foreign_token,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body:?}");
    assert_eq!(body["error"]["code"], "permission_denied");
}
```

- [ ] **Step 2: Run the backend regression**

Run:

```powershell
cargo test --manifest-path backend-rs/Cargo.toml --test direct_chats direct_workspace_supports_file_mutations_and_git_through_shared_routes
```

Expected: PASS without production backend changes. If it fails, fix the shared ownership/workspace resolver only; do not add duplicate direct-chat routes.

- [ ] **Step 3: Run formatting and complete focused delivery gates**

Run:

```powershell
cargo fmt --manifest-path backend-rs/Cargo.toml -- --check
pnpm --filter @ag-swarmer/frontend test -- ConversationWorkspacePanel ConversationChatView WorkspaceFilesTab Composer workspaceDrag
pnpm --filter @ag-swarmer/frontend type-check
cargo test --manifest-path backend-rs/Cargo.toml --test direct_chats
git diff --check
```

Expected: every command exits `0`.

- [ ] **Step 4: Commit the backend contract test**

```powershell
git add backend-rs/crates/backend/tests/direct_chats.rs
git commit -m "test(workspace): cover direct file mutations and git"
```

## Plan Self-Review

- Spec coverage: Files/Git parity is Task 1; rename/delete and scope-correct invalidation are Task 1; WebView file/folder drag fallback is Task 2; backend ownership and shared-route compatibility are Task 3.
- Scope: private external upload, recursive directory attachments, new APIs, and dependencies remain excluded.
- Type consistency: every panel uses `ConversationScope`; file queries and invalidation share `conversationWorkspaceFilesQueryKey(scope, conversationId)`; Git continues to use the conversation ID through existing group-compatible routes.
- Safety: text fallback supplies only candidate paths; the current conversation API still determines existence, kind, metadata, and authorization.
