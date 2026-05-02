class AgentChatError(Exception):
    """Base exception for AgentChat domain errors."""


class NotFoundError(AgentChatError):
    """Resource not found."""


class PermissionDeniedError(AgentChatError):
    """User lacks permission to perform this action."""


class ConflictError(AgentChatError):
    """Resource already exists or state conflict."""


class LLMProviderError(AgentChatError):
    """Upstream LLM provider failure."""
