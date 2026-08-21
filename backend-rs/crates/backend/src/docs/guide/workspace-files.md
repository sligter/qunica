# Workspace files

The workspace panel browses the folder bound to the current conversation, and edits text files in place.

## Attaching files to a message

Dragging a file from the panel into the composer attaches a **reference** to it: the path is recorded, not a copy of the contents, so there is no duplicate to keep in sync. The server re-checks on send that the path is inside the conversation's workspace.

Dragging a *directory* inserts its relative path as text at the cursor instead. Directories are not expanded, and their contents are not attached.

## Editing

In-app editing works on UTF-8 text the server has confirmed as text.

Saving carries a digest of the contents as they were when the file was opened. If the file changed on disk in the meantime, the save is refused, the local draft is kept, and a confirmation is required before reloading — an external edit is never silently overwritten.

## Previewing

- **HTML** renders in a sandboxed iframe with no script, same-origin, or navigation permission.
- **Images** and **PDFs** get a size-limited preview.
- **Office documents and unknown formats** are not converted or edited. Metadata is shown with a download link.

## Git

When the workspace is a git repository, the git tab covers status, staging, diffs, history, branches, stash, and remote fetch/pull/push. Commit messages can be generated from the staged diff with an optional custom instruction.

## Attachments and images

Image attachments are sent to the model natively when the agent's provider and `vision` setting allow it. A file that cannot be sent natively is attached as a reference instead, with a warning.
