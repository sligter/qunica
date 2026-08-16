-- The ACP session an agent is part-way through in a conversation.
--
-- Reusable ACP sessions otherwise live only in the process-global map, so
-- closing the app threw away everything the agent knew that the host
-- transcript cannot carry: the files it read, its tool results, its own
-- reasoning. Re-sending the rendered transcript into a brand new session reads
-- to the user as the agent forgetting the work it just did.
--
-- The row is only the agent's `sessionId` plus the signature of the session it
-- belongs to. It is offered back on the next launch only when that signature
-- still matches — same owner, workspace, runtime config, and host context —
-- and only to a runtime that advertises `loadSession`. Anything else starts a
-- fresh session exactly as before.
CREATE TABLE IF NOT EXISTS acp_sessions (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL,
    -- The id the ACP agent handed out for its own session store.
    session_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    cwd TEXT NOT NULL,
    -- Hash of the normalized runtime config the session was opened with.
    config_hash TEXT NOT NULL,
    -- Hash of the host-side context (the agent brief); null when untracked.
    context_hash TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (thread_id, agent_id)
);
