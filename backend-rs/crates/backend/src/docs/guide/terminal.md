# Terminal

Qunica has a tabbed terminal docked at the bottom of a conversation, opened
with `Ctrl`/`Cmd` + `` ` ``. The desktop app owns its PTY directly; the browser
uses an authenticated PTY owned by the backend.

## Scope

**The terminal is not sandboxed.** It runs a full shell with the Qunica
process's permissions and can reach files and processes outside the workspace.
It starts in the bound workspace directory, but nothing confines it there. In
Docker that shell runs inside the container, so mount only directories it may
reach.

It is also independent of agent tool execution: the `Bash` tool runs its own guarded process inside the workspace root and shares nothing with these tabs.

## Lifetime

- Switching conversations does not stop a running command.
- Hiding the app to the system tray does not stop it either.
- Quitting the app terminates the PTY and its descendants.
- Restarting restores tab names and starting directories only. Previous processes, input, and output are gone.

## Availability

The terminal appears for a conversation with a bound local workspace. Desktop
sessions run on the desktop host; browser sessions run on the backend host.
