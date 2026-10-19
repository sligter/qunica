CREATE TABLE run_replay_floors (
    stream_id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    retained_after INTEGER NOT NULL
);
