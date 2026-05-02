"""GroupState — LangGraph state schema for an agent invocation.

Two fields, no reducers:
- `input_messages`: the message list assembled fresh from DB before each
  invocation (system_prompt + announcement + last 20 group messages). It is
  REPLACED on every invocation; it does not accumulate across them.
- `last_response`: set by `agent_node` to the LLM's reply. The caller persists
  this to the `messages` table.

LangGraph's PostgresSaver checkpoints both fields per (thread_id, checkpoint_id).
The thread_id corresponds 1:1 to a row in our `threads` table — this is the
hook for future interrupt/resume work in Phase 1 Week 6.
"""

from typing import TypedDict

from langchain_core.messages import AIMessage, BaseMessage


class GroupState(TypedDict, total=False):
    input_messages: list[BaseMessage]
    last_response: AIMessage | None
