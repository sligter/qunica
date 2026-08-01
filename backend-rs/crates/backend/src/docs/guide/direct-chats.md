# Direct chats

A direct chat is a one-on-one conversation with a single agent. Use it when there is nothing to coordinate.

## Starting one

Pick an agent from the chat picker. The chat's title is generated from the first exchange, and can be renamed by hand at any time; renaming stops it being regenerated.

## How it differs from a group

- Exactly one agent, so there is nothing to `@mention` and no scheduler.
- No announcement, no member management, no turn trace.
- The workspace is the agent's own. It is not a separate binding: rebinding the agent's workspace moves its direct chats with it.

## Context

**Reset context** starts a new thread while leaving the history on screen. The agent stops seeing the earlier messages, but nothing is deleted. Use it when a long conversation has drifted.

**Clear messages** deletes the history. Workspace files are untouched.

## Files

The workspace panel lists the agent's workspace. Dragging a file into the composer attaches a reference to it — the file is not copied and no duplicate is made. Dragging a directory inserts its relative path as text instead.

## Notes

- Deleting the agent leaves the chat readable, marked unavailable.
- The built-in Assistant's own chat does not appear in this list; it is reached from the floating dock.
