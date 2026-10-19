CREATE TABLE computer_proxy_grants (
    token_hash TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK(scope IN ('model','notes')),
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES computers(id),
    generation TEXT NOT NULL,
    agent_id TEXT NOT NULL REFERENCES agents(id),
    turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
    job_id TEXT NOT NULL REFERENCES execution_jobs(id) ON DELETE CASCADE,
    provider_id TEXT REFERENCES llm_providers(id),
    model TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ix_computer_grants_turn ON computer_proxy_grants(turn_id,revoked);
CREATE TABLE computer_model_calls (
    id TEXT PRIMARY KEY,
    token_hash TEXT NOT NULL REFERENCES computer_proxy_grants(token_hash),
    turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
    request_hash TEXT NOT NULL,
    status TEXT NOT NULL,
    reserved_tokens INTEGER NOT NULL,
    input_tokens INTEGER,
    output_tokens INTEGER,
    UNIQUE(token_hash,request_hash)
);
CREATE INDEX ix_computer_calls_turn ON computer_model_calls(turn_id,status);
CREATE TABLE computer_artifacts (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    computer_id TEXT NOT NULL REFERENCES computers(id),
    turn_id TEXT NOT NULL REFERENCES group_turns(id) ON DELETE CASCADE,
    generation TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    UNIQUE(turn_id,computer_id,path)
);
CREATE TABLE computer_acp_sessions (
    computer_id TEXT NOT NULL REFERENCES computers(id),
    agent_id TEXT NOT NULL REFERENCES agents(id),
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    context_hash TEXT NOT NULL,
    session_id TEXT NOT NULL,
    PRIMARY KEY(computer_id,agent_id,thread_id)
);
