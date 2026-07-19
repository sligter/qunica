# Task 1 Report: Attachment Message Contract

## Status

Completed and committed. The message API now accepts workspace attachment path references, validates them authoritatively, persists durable metadata in `messages.content_json`, returns it in message history, and includes it in `user_message` stream payloads.

## RED Evidence

Command run before implementation:

```text
cargo test -p ag-swarmer-backend --test groups attachment -- --nocapture
```

Result: 1 passed, 1 failed.

`attachment_message_persists_workspace_image_metadata` failed because the pre-change API rejected attachment-only input with HTTP 400 where the test expected HTTP 201. This demonstrated that messages required nonblank text and did not yet support attachment metadata.

## GREEN Evidence

Focused backend contract tests:

```text
cargo test -p ag-swarmer-backend --test groups attachment -- --nocapture
```

Result: 2 passed, 0 failed, 68 filtered out.

Broader backend integration suite:

```text
cargo test -p ag-swarmer-backend --test groups
```

Result: 70 passed, 0 failed.

Frontend type-check:

```text
pnpm --dir frontend type-check
```

Result: passed (`tsc -b --noEmit`).

Diff validation:

```text
git diff --check
```

Result: passed.

## Changed Files

- `backend-rs/crates/backend/src/api/messages.rs`
  - Added `MessageInput` and path-only `MessageAttachmentInput` request contract.
  - Validates attachment count, owned active local workspace, safe canonical workspace paths, files, and duplicate canonical paths.
  - Computes durable attachment metadata and image/file classification.
  - Returns attachments from history and accepts attachment-only messages.
- `backend-rs/crates/backend/src/runtime/group.rs`
  - Added lower-layer `MessageAttachment` and `AttachmentKind` types.
  - Added attachments to `TurnRequest`.
  - Persists user attachment metadata as `{"version":1,"attachments":[...]}` and emits it in user stream payloads.
- `backend-rs/crates/backend/tests/groups.rs`
  - Added authenticated API coverage for attachment-only PNG metadata persistence/history/event payload and unsafe/duplicate rejection without insertion.
- `frontend/src/types/api.ts`
  - Added `MessageAttachmentKind`, `MessageAttachment`, `MessageSendInput`, and required `Message.attachments`.
- `frontend/src/hooks/useSendMessageStream.ts`
  - Added Zod validation/hydration for attachment stream payloads and typed request posting.
- Direct frontend constructors and test fixtures
  - Added the required empty attachment array where messages are constructed locally.
  - Preserved attachment metadata through resumed agent message replacement.

## Self-Review

- Text-only messages retain the existing behavior: attachments are empty in payloads/responses and `content_json` remains `NULL`.
- No schema migration or message-table change was introduced.
- Binary data is never read into an LLM request by this task. Attachments are durable workspace references and metadata only.
- Image classification is limited to `image/png`, `image/jpeg`, `image/webp`, and `image/gif`; all other MIME types classify as `file`.
- Path validation uses the existing lower-level safe workspace resolver followed by canonicalization and containment/file checks.

## Concerns

- `cargo fmt --check` for the whole backend workspace reports an unrelated existing formatting delta in `backend-rs/crates/backend/tests/providers_settings.rs`. The three Rust files changed for this task were formatted with `rustfmt`, and no task-file formatting issue remains.
- The task intentionally stops at the durable contract. Provider vision behavior and Composer attachment UI are deferred to later tasks.
