-- Live replies are split whenever reasoning or a tool interrupts visible text.
-- Rebuild those boundaries once for messages written before the durable mirror
-- gained `response_segments`. Only exact content matches are backfilled.
WITH target_messages AS (
  SELECT id
  FROM messages
  WHERE sender_type = 'agent'
    AND CASE
      WHEN json_valid(content_json)
        THEN coalesce(json_type(content_json, '$.response_segments'), 'null') = 'null'
      ELSE 1
    END
),
target_streams AS (
  SELECT DISTINCT stream_id
  FROM stream_events
  JOIN target_messages
    ON target_messages.id = json_extract(stream_events.payload_json, '$.message_id')
  WHERE kind = 'agent_message'
),
relevant AS (
  SELECT stream_id,
         seq,
         kind,
         json_extract(payload_json, '$.agent_id') AS agent_id,
         json_extract(payload_json, '$.text') AS text,
         json_extract(payload_json, '$.message_id') AS message_id
  FROM stream_events
  WHERE stream_id IN (SELECT stream_id FROM target_streams)
    AND (
      kind IN (
        'agent_start',
        'token',
        'reasoning',
        'tool_call_start',
        'agent_message',
        'waiting_for_user',
        'warning',
        'agent_silent'
      )
      OR (
        kind = 'acp_agent_run'
        AND json_extract(payload_json, '$.status') = 'running'
      )
    )
),
numbered AS (
  SELECT *,
         lag(kind) OVER (
           PARTITION BY stream_id
           ORDER BY seq
         ) AS previous_kind,
         lag(agent_id) OVER (
           PARTITION BY stream_id
           ORDER BY seq
         ) AS previous_agent_id,
         coalesce(sum(kind = 'agent_message') OVER (
           PARTITION BY stream_id, agent_id
           ORDER BY seq
           ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING
         ), 0) AS response_number
  FROM relevant
),
token_starts AS (
  SELECT *,
         CASE
           WHEN previous_kind = 'token' AND previous_agent_id = agent_id THEN 0
           ELSE 1
         END AS starts_segment
  FROM numbered
  WHERE kind = 'token'
    AND coalesce(text, '') <> ''
),
segmented_tokens AS (
  SELECT *,
         sum(starts_segment) OVER (
           PARTITION BY stream_id, agent_id, response_number
           ORDER BY seq
         ) AS segment_number
  FROM token_starts
),
ordered_tokens AS (
  SELECT *
  FROM segmented_tokens
  ORDER BY stream_id, agent_id, response_number, seq
),
segments AS (
  SELECT stream_id,
         agent_id,
         response_number,
         segment_number,
         group_concat(text, '') AS segment_text,
         min(seq) AS first_seq
  FROM ordered_tokens
  GROUP BY stream_id, agent_id, response_number, segment_number
),
ordered_segments AS (
  SELECT *
  FROM segments
  ORDER BY stream_id, agent_id, response_number, first_seq
),
response_segments AS (
  SELECT stream_id,
         agent_id,
         response_number,
         json_group_array(segment_text) AS segments_json,
         group_concat(segment_text, '') AS full_content
  FROM ordered_segments
  GROUP BY stream_id, agent_id, response_number
),
message_events AS (
  SELECT stream_id, agent_id, response_number, message_id
  FROM numbered
  WHERE kind = 'agent_message'
    AND message_id IS NOT NULL
)
UPDATE messages AS message
SET content_json = json_set(
  CASE WHEN json_valid(message.content_json) THEN message.content_json ELSE '{}' END,
  '$.schema_version',
  CASE
    WHEN json_valid(message.content_json)
      THEN coalesce(json_extract(message.content_json, '$.schema_version'), 1)
    ELSE 1
  END,
  '$.response_segments',
  json(response_segments.segments_json)
)
FROM message_events
JOIN response_segments USING (stream_id, agent_id, response_number)
WHERE message.id = message_events.message_id
  AND coalesce(message.content, '') = response_segments.full_content
  AND CASE
    WHEN json_valid(message.content_json)
      THEN coalesce(json_type(message.content_json, '$.response_segments'), 'null') = 'null'
    ELSE 1
  END;
