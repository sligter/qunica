-- External topics (a GitHub issue or pull request, a caller-chosen key) that
-- route trigger events to threads. A dispatched task thread subscribed to a
-- topic receives its events directly; any other thread is the topic's home.
CREATE TABLE thread_topics (
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  topic TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (thread_id, topic)
);

CREATE INDEX ix_thread_topics_group_topic ON thread_topics(group_id, topic);
