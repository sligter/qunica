-- Every conversation now uses the bounded scheduler. Preserve the old
-- scheduler-off behavior as a one-pass deterministic budget profile.
UPDATE groups
SET scheduler_enabled = 1,
    agent_mention_policy = 'display_only',
    max_agent_steps = NULL,
    max_steps_per_agent = 1,
    max_scheduler_hops = 0,
    max_moderator_calls = 0,
    max_consecutive_failures = 1,
    max_total_failures = 1,
    moderator_enabled = 0,
    allow_agent_free_mention = 0,
    agent_free_mention_max_dispatches = 0
WHERE scheduler_enabled = 0;

-- Keep the legacy columns for SQLite/backward compatibility, but make their
-- persisted values truthful for older readers. New code no longer exposes or
-- consults them.
UPDATE groups SET scheduler_enabled = 1;
