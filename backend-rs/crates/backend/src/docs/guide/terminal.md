# Terminal

The desktop app has a tabbed terminal docked at the bottom of a conversation, opened with `Ctrl`/`Cmd` + `` ` ``.

## Scope

**The terminal is not sandboxed.** It runs a full host shell with the account's permissions and can reach files and processes outside the workspace. It starts in the bound workspace directory, but nothing confines it there.

It is also independent of agent tool execution: the `Bash` tool runs its own guarded process inside the workspace root and shares nothing with these tabs.

## Lifetime

- Switching conversations does not stop a running command.
- Hiding the app to the system tray does not stop it either.
- Quitting the app terminates the PTY and its descendants.
- Restarting restores tab names and starting directories only. Previous processes, input, and output are gone.

## Availability

Desktop only. The browser build has no terminal, and it appears only for a conversation with a bound local workspace.
