# LLM providers

A provider holds the credentials and model list for one LLM vendor. Every agent that chats needs one bound to it.

## Fields

- **name** — how the provider appears in pickers.
- **kind** — which API dialect to speak. One of `openai-compatible`, `anthropic`, or `gemini`.
- **base_url** — the API endpoint. Required for `openai-compatible`; the Anthropic and Gemini defaults are built in.
- **api_key** — the secret. Stored locally and never returned by the API; reads report only whether one is set, and the UI shows a masked form.
- **default_model** — the model an agent uses when it does not name one itself.
- **models** — the list offered in model pickers, each with an optional context window and output reserve.
- **reasoning_passback** — configured per model; whether to send that model's own reasoning back on the next turn. Off by default.

## Discovering models

**Test / discover models** asks the provider for its catalog and fills the model list. The request is made by the backend so the key never leaves the machine. If discovery fails, models can still be typed in by hand.

## Kinds

| Kind | Use for |
| --- | --- |
| `openai-compatible` | OpenAI, and any server exposing the same `/v1/chat/completions` shape — most local and third-party gateways |
| `anthropic` | Claude models over the Anthropic Messages API |
| `gemini` | Google Gemini models |

## Notes

- The API key is write-only over the API. Sending an update without an `api_key` field keeps the stored one; it does not clear it.
- Deleting a provider does not delete the agents bound to it. Those agents stop being able to reply until they are rebound.
