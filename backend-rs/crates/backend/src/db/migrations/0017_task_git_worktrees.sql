ALTER TABLE threads
ADD COLUMN git_branch TEXT CHECK (
  git_branch IS NULL OR length(trim(git_branch)) BETWEEN 1 AND 255
);

ALTER TABLE threads
ADD COLUMN worktree_path TEXT CHECK (
  worktree_path IS NULL OR length(trim(worktree_path)) > 0
);

CREATE UNIQUE INDEX ux_threads_group_git_branch
ON threads(group_id, git_branch)
WHERE agent_id IS NULL AND git_branch IS NOT NULL;
