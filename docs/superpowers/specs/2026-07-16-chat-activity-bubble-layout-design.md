# Chat Activity Bubble and Message Layout Design

## Objective

Fix two chat presentation defects without changing scheduler or message persistence contracts:

1. User messages must remain inside the centered chat column at every supported viewport width.
2. Each agent response must present all reasoning segments and tool calls through one collapsed activity bubble instead of a long stack of separate bubbles.

## Scope

This is a frontend-only presentation change. It applies to both live stream events and persisted message details after reload. It does not change the backend `content_json` schema, stream event schema, scheduler trace, or tool execution behavior.

## Message Layout

The message list, message row, sender content column, and bubble form one continuous shrinkable flex chain. Each relevant flex child must use `min-width: 0`, and the row content column must have an explicit responsive width cap. The message list must not acquire page-level horizontal scrolling.

Normal prose and long unbroken text wrap inside the bubble. Wide content that is meaningful as a horizontal surface, including code blocks and Markdown tables, keeps its own local horizontal scrolling. This prevents one message from expanding the entire chat canvas.

## Agent Activity Bubble

A shared activity container renders the process metadata associated with one agent response.

- The outer container is one `details` disclosure and is collapsed by default for both active and historical responses.
- Its summary displays an Activity label plus live counts, for example `5 reasoning - 12 tools`.
- While streaming, the same summary updates in place and shows an active status without expanding automatically.
- Expanding the container reveals one combined reasoning section and one compact tool-call list.
- Reasoning segments are combined in their existing order with clear separators. They are not rendered as separate top-level pills.
- Tool calls remain individually inspectable inside the activity container so arguments, results, and statuses are still available.
- Empty categories are omitted. If only reasoning or only tools exist, the summary and body show only that category.

Live and persisted renderers share the same activity shell and visual contract. Live events retain their stream order internally where available. Persisted data keeps the order available in its existing reasoning and tool arrays; exact cross-category interleaving is not invented after reload.

## Accessibility

The activity summary is keyboard operable through native `details`/`summary` behavior. The summary has a stable accessible label that includes counts and streaming state. Nested tool disclosures remain keyboard accessible. Focus indicators use the existing application focus treatment.

## Testing

Frontend tests cover:

- the user message content chain is shrinkable and the scroll viewport suppresses page-level horizontal overflow;
- live reasoning and tool events render one collapsed activity disclosure with correct counts;
- expanding the disclosure reveals combined reasoning and tool details;
- persisted reasoning and tool data follows the same collapsed interaction;
- reasoning-only and tool-only activity summaries omit empty categories.

Verification runs the focused component tests, the complete frontend test suite, lint, type-check, production build, and a desktop release build for manual testing.

## Non-Goals

- Changing provider reasoning generation or passback.
- Changing tool-call execution, scheduler dispatches, or trace privacy.
- Migrating historical message data to a unified event timeline.
- Redesigning message colors, typography, or the overall chat shell.
