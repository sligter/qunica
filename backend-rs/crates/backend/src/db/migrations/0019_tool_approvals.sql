-- Approvals a user has granted for a thread's tool calls.
--
-- Keyed on the policy rule the tool asked about (`delete-files`), not on the
-- command text: a user who approves deleting a build directory has authorised
-- the capability for that thread, and re-asking for the next `rm` would be
-- nagging rather than consent. Scoping to the thread is what keeps the grant
-- from leaking into unrelated work.
--
-- Declined answers are recorded too. They are not consulted when deciding
-- whether to ask again — a "no" to one command should not silently refuse the
-- next one — but the row is the audit trail for what was asked and answered.
CREATE TABLE IF NOT EXISTS tool_approvals (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    agent_id TEXT,
    tool_name TEXT NOT NULL,
    rule TEXT NOT NULL,
    -- The exact call that was answered, for the audit trail.
    tool_call_id TEXT,
    subject TEXT,
    approved INTEGER NOT NULL,
    -- 1 when the user chose to apply this answer to the rest of the thread.
    remembered INTEGER NOT NULL DEFAULT 0,
    note TEXT,
    created_at TEXT NOT NULL
);

-- The lookup the shell policy makes before every gated command: which rules are
-- remembered as approved for this thread.
CREATE INDEX IF NOT EXISTS idx_tool_approvals_thread_rule
    ON tool_approvals (thread_id, rule, remembered, approved);
