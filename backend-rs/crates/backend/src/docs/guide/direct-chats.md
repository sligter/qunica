# Direct chats

A direct chat is a one-on-one conversation with a single agent. Use it when there is nothing to coordinate.

## Starting one

Pick an agent from the chat picker. When your first message arrives, the agent's own model names the chat from it and the new title appears on the spot; you can rename by hand at any time, which stops regeneration. If the agent has no LLM provider yet (or generation fails), the title falls back to the first message's opening words.

## How it differs from a group

- Exactly one agent, so there is nothing to `@mention`. It uses the same scheduler as groups with a one-candidate, one-step profile.
- No announcement or member management. Turns are still persisted and cancellable, although the direct-chat UI does not show the trace panel.
- The workspace is the agent's own. It is not a separate binding: rebinding the agent's workspace moves its direct chats with it.

## Context

**Reset context** starts a new thread while leaving the history on screen. The agent stops seeing the earlier messages, but nothing is deleted. Use it when a long conversation has drifted.

**Clear messages** deletes the history. Workspace files are untouched.

When a conversation has several task threads, each task can also be archived, restored, deleted, or have only its own messages cleared from the task header.

## Files

The workspace panel lists the agent's workspace. Dragging a file into the composer attaches a reference to it — the file is not copied and no duplicate is made. Dragging a directory inserts its relative path as text instead.

## Notes

- Deleting the agent leaves the chat readable, marked unavailable.
- The built-in Assistant's own chat does not appear in this list; it is reached from the floating dock.
