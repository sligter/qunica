# Workspaces

A workspace is the directory an agent's file and shell tools operate in. It is the boundary: path resolution rejects anything that escapes the root.

## Backend types

- **local** — a real directory on the backend machine. `local_path` can be an existing absolute directory or one directory name to create below the configured workspace root; the canonical absolute path is stored. This is the only kind whose files agents can actually read and write.
- **cloud_sandbox** — a placeholder for a remote sandbox. File and shell tools are not available against it; agents bound to one report that no workspace is configured.

## Creating one

The easiest way is `auto_create`: set `backend_type` to `local`, set `auto_create` to true, and leave `local_path` empty. Qunica creates the folder under the workspace root from Settings, so nobody has to find or type a path. It requires that root to be configured, and refuses if `local_path` is also given.

Otherwise provide an existing absolute `local_path`, or a single directory name such as `my-project`. A relative name is created below the configured workspace root. Separators, drive prefixes, and `..` are rejected, so it cannot escape that root.

On the desktop app, the folder picker is the native OS dialog and stores a real filesystem path.
In a browser, a folder picker cannot expose or mount the client filesystem. Qunica uses only the selected directory name and creates it on the backend; it never uploads the selected files.

## What a workspace bounds

- `Read`, `Write`, `Edit`, `Glob`, and `Grep` resolve every path against the root. Parent-directory segments, absolute paths, drive prefixes, and UNC paths are rejected.
- `Bash` runs in the primary root only. Its command guard is built around a single root, so named mounts are not reachable from a shell.
- The built-in terminal is **not** bounded by the workspace. It runs a full host shell with the account's own permissions.

## Project conventions

Before each built-in agent turn, Qunica reads `AGENTS.md` from the primary workspace root, falling back to `CLAUDE.md` when it is absent. Up to 6,000 characters are added to the system prompt immediately after the Workspace section, so edits take effect on the next turn and survive context compaction.

The file is workspace-controlled guidance. It has lower priority than Qunica's operating rules, approval gates, and workspace constraints. Only the primary root is checked: named mounts and nested directories are not scanned.

ACP agents do not receive this host-injected section. External CLI agents such as Codex CLI and Claude Code discover project instructions in their working directory themselves, so injecting it again would duplicate the same rules.

## Sharing between agents

An agent in a group chooses which roots it can address:

- `group` — the conversation's workspace only.
- `group_and_self` — the conversation's workspace as primary, with the agent's own mounted at `~self/`.
- `self` — the agent's own workspace only; the conversation's is out of reach, including its attachments.

## Notes

- A direct chat has no workspace of its own; it follows the agent it is with. Rebinding the agent moves its existing direct chats too.
- Deleting a workspace unbinds it from agents rather than deleting files.
