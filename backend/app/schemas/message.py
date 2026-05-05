from datetime import datetime
from typing import Any
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field


class MessageCreate(BaseModel):
    content: str = Field(min_length=1)


class MessageRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: UUID
    group_id: UUID
    thread_id: UUID | None
    sender_type: str
    sender_id: UUID | None
    message_type: str
    content: str | None
    status: str
    refs: dict[str, Any] | None
    reply_to_message_id: UUID | None
    created_at: datetime


class SilentAgentTurnRead(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    agent_id: UUID
    display_name: str


class MessageSendResponse(BaseModel):
    user_message: MessageRead
    agent_replies: list[MessageRead]
    warnings: list[str]
    silent_turns: list[SilentAgentTurnRead] = []
    all_silent: bool = False
    waiting_for_user: bool = False
