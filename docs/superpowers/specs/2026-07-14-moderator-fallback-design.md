# Independent Moderator Fallback Design

**Status:** Design approved; implementation awaits written-spec review.

## Goal

Add an optional, bounded moderator decision to scheduled group turns. The moderator
chooses only among candidates that the deterministic scheduler has already proven legal.
It has no visible chat path and cannot expand a turn's authority, topology, or budget.

## Configuration Contract

The existing group API contract remains unchanged. When `moderator_enabled` is true,
both `moderator_provider_id` and `moderator_model` are required. The runtime does not
fall back to a provider default model.

The configured provider must still belong to the group owner and be active when a turn
runs. Missing or unavailable runtime configuration is a controlled moderator failure,
not a chat-facing error.

## Selection Flow

1. Preserve deterministic priority for user mentions and explicit agent actions.
2. Build the legal candidate set after applying active, muted, topology, hop, per-agent,
   step, failure, and token constraints.
3. Dispatch a single legal candidate deterministically without a model call.
4. With two or more legal candidates, call the moderator only when it is enabled and its
   call budget remains available.
5. Accept a valid selection as `moderator`, then repeat legal-candidate validation before
   dispatching it.
6. On timeout, provider error, invalid response, out-of-set response, failed post-response
   validation, or unavailable moderator budget, select the first legal deterministic
   candidate as `moderator_fallback`.
7. Finish the turn only when no legal candidate remains or a turn budget requires a
   terminal outcome.

## Moderator Boundary

`group_scheduler::moderator` is a dedicated component with a narrow request and response
contract. It receives:

- the triggering visible user message as the objective, capped at 2,000 Unicode scalar
  values;
- at most four visible text messages, each capped at 1,000 Unicode scalar values;
- candidate `agent_id`, display name, and scheduler reason; and
- remaining agent steps.

It does not receive full chat history, tool results, workspace data, skills, agent persona,
reasoning content, or a visible message/stream path. Provider calls use `tools: []` and
`temperature: 0.0`.

The response must be exactly a JSON object containing one string field:

```json
{"agent_id":"candidate-id"}
```

Any other shape, extra field, non-string value, or unknown candidate is rejected.

## Accounting, Persistence, and Observability

Every actual provider call increments `group_turns.moderator_calls` and aggregates reported
token usage into `group_turns.total_tokens`, subject to the shared budget. The turn's
non-secret scheduler configuration and budget limits remain in its configuration snapshot.

The selected dispatch persists the existing `selection_reason` as `moderator` or
`moderator_fallback`. This task does not add a visible chat message or an SSE event; the
event protocol extension belongs to the later scheduler event task.

Logs contain only stable identifiers, counts, and controlled failure categories. They do
not contain prompts, completions, secrets, tool contents, or reasoning.

## Error Handling

Moderator failures are recoverable and lead to deterministic fallback. Persistence errors
continue to propagate through the runtime's typed error path. `StepErr::Cancelled` remains
normal control flow and must preserve the existing interrupted-turn lifecycle rather than
being converted into fallback.

## Validation

Tests cover disabled and missing configuration, a single candidate with no model call,
valid selection through a fake provider, invalid and out-of-set JSON, timeout/provider
fallback, post-response topology revalidation, the four-call maximum, token aggregation,
and unchanged scheduler-disabled behavior.
