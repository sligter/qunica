# LLM providers

A provider holds the credentials and model list for one LLM vendor. Every agent that chats needs one bound to it.

## Fields

- **name** — how the provider appears in pickers.
- **kind** — which API dialect to speak. One of `openai-compatible`, `openai-responses`, `anthropic`, `anthropic-compatible`, or `gemini`.
- **base_url** — the API endpoint. Required for `openai-compatible`; the OpenAI Responses, Anthropic and Gemini defaults are built in.
- **api_key** — the secret. Stored locally and never returned by the API; reads report only whether one is set, and the UI shows a masked form.
- **default_model** — the model an agent uses when it does not name one itself.
- **models** — the list offered in model pickers, each with an optional context window and output reserve. The request path infers sampling and thinking support from the model id — OpenAI reasoning families (`o1`/`o3`/`o4`/`gpt-5`+/`gpt-6`+) reject `temperature` and `top_p`, while classic chat models reject `reasoning_effort`. API clients can pin either answer per model with `supports_temperature` / `supports_reasoning_effort`.
- **reasoning_passback** — configured per model; whether to send that model's own reasoning back on the next turn. Off by default. DeepSeek's thinking mode requires `reasoning_content` on every previous assistant message when `tools` is present, so a rejected turn is retried once with passback on automatically. Turn it on explicitly for a gateway that needs it on the first attempt; leave it off for plain chat models. Applies to the `openai-compatible` kind.

Thinking is requested through `reasoning_effort` alone, which OpenAI and DeepSeek understand. A gateway that instead requires a separate `thinking` / `enable_thinking` toggle is not detected automatically; the level is ignored there rather than translated.

## Discovering models

**Test / discover models** asks the provider for its catalog and fills the model list. The request is made by the backend so the key never leaves the machine. If discovery fails, models can still be typed in by hand.

## Kinds

| Kind | Use for |
| --- | --- |
| `openai-responses` | OpenAI Responses API (`/v1/responses`) and compatible gateways; accepts an API root or full `/responses` endpoint, defaults to `https://api.openai.com/v1` |
| `openai-compatible` | OpenAI, and any server exposing the same `/v1/chat/completions` shape — most local and third-party gateways |
| `anthropic` | Claude models over the Anthropic Messages API |
| `gemini` | Google Gemini models |

## Notes

- The API key is write-only over the API. Sending an update without an `api_key` field keeps the stored one; it does not clear it.
- Deleting a provider does not delete the agents bound to it. Those agents stop being able to reply until they are rebound.

### OpenAI Responses

Choose **OpenAI Responses** when the server exposes `/responses`. Model discovery uses `/models` on the same API root. Text and image input, streamed text and reasoning summaries, function calls and results, and token usage are supported. Requests use `store: false`; encrypted reasoning is retained with tool calls for stateless tool-loop replay. The Chat Completions `reasoning_passback` switch does not apply. Failed, incomplete, or interrupted responses fail the turn without executing buffered tool calls.

Protocol reference: [OpenAI Responses streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events).
