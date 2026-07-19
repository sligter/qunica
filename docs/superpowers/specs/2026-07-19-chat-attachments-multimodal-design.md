# Chat Attachments And Multimodal Images Design

## Goal

Allow users to select, drag into the composer, or paste image and file attachments. Every attachment is saved in the current conversation workspace and appears in the durable chat transcript. PNG, JPEG, WebP, and GIF images are sent as provider-native visual input only to built-in Agents whose configured model declares vision support. Every other Agent receives a controlled workspace attachment reference.

This design applies equally to group chats and direct chats.

## Scope

The first release supports the following input sources:

- File-picker selection of one or more files.
- Operating-system file drag and drop into the composer.
- Clipboard image paste.
- Existing internal workspace-file drag and drop.

Native visual input is restricted to files whose MIME type is `image/png`, `image/jpeg`, `image/webp`, or `image/gif`. SVG, HEIC, PDF, Office documents, archives, code, and arbitrary binary data are durable workspace attachments only. The release does not extract document text, perform OCR, transcode image formats, or add audio/video input.

## User Experience

The composer keeps draft text and a pending attachment collection. Adding a file uploads it to the conversation workspace before send and displays a compact attachment item. The item shows the filename, MIME type, size, upload state, and a remove action. Images additionally show a preview thumbnail.

The send action is enabled when the composer has non-whitespace text or at least one successfully uploaded attachment. An attachment-only message is valid. Files that fail to upload remain visible as errors and are not included when the message is sent; retry and removal remain available.

The composer handles `DataTransfer.files` for external drops, preserves the existing internal workspace path drop behaviour, and handles clipboard image files through `ClipboardEvent.clipboardData.files`. Pasted text retains standard textarea behaviour.

Persisted user messages render the text followed by attachment items. Images have a preview/open action. Other files provide a workspace/open action. The transcript never presents a file path as though the model had read the file.

## Attachment Data Model

Messages gain a structured content representation that retains the existing text field for compatibility and adds an ordered attachment collection. Each attachment records:

- Stable attachment identifier.
- Conversation ID and message ID after persistence.
- Workspace-relative path.
- Original filename.
- MIME type.
- Byte size.
- Attachment category: `image` only for the four supported image MIME types; otherwise `file`.

The API accepts a message body containing `content` and `attachments`. It validates that each referenced workspace file belongs to the active owned conversation, records immutable attachment metadata with the message, and rejects missing, duplicate, or invalid references. Uploads remain a separate multipart endpoint, so the message streaming endpoint continues to accept JSON rather than multipart bodies.

The message API and frontend types return attachment data for history, stream events, and optimistic rendering. Existing text-only messages remain valid and render unchanged.

## Runtime And Provider Routing

The neutral LLM message contract changes from a string-only body to ordered content parts. It supports text and an image part containing attachment metadata plus a provider-readable data source. Tool messages and assistant output remain text-only for this release.

Before each built-in Agent invocation, the runtime builds content from the persisted transcript:

- The normal human/peer identity envelope remains text and retains its current prompt-injection boundary.
- Each attachment is represented in the envelope with filename, MIME type, byte size, and workspace-relative path. It explicitly states that the attachment must be read through workspace tools when it is not supplied as native visual input.
- For a configured vision-capable Agent, each valid image attachment on a user or peer message also becomes a native image content part.
- For a non-vision Agent, all attachments remain reference-only text. It receives no image bytes.
- Non-image attachments remain reference-only for every built-in Agent.

The image source is read from the conversation workspace at invocation time after enforcing the configured count and size limits. The runtime encodes it only for the provider request; base64 image bytes are neither embedded in the durable message body nor rendered into transcript text.

External ACP Agents always receive the existing text prompt plus the controlled attachment references and access files through their configured workspace capability. The system does not claim that an ACP runtime can inspect images.

## Provider Mappings

Built-in providers map the neutral image part only when the Agent capability enables vision:

- OpenAI-compatible: user `content` becomes an array of text and `image_url` parts with a data URL. This is guarded by the vision capability because OpenAI-compatible endpoints vary widely.
- Anthropic: user `content` becomes text and base64 `image` source blocks.
- Gemini: user `parts` gains `inlineData` image parts alongside text parts.

System messages, assistant messages, tool results, and non-vision requests preserve their current wire shapes whenever no image parts are present. This avoids regressions for text-only models and compatibility endpoints.

## Capability Configuration

Agent model configuration gains an explicit `vision` boolean, defaulting to `false`. It is independent of provider kind. Provider/model discovery may later suggest a default capability, but user configuration remains authoritative.

When a conversation has agents with mixed capabilities, every Agent receives the attachment reference context. Only agents with `vision: true` receive native image bytes. The UI may communicate this capability state, but it must not suppress attachment sending based on the lowest-capability participant.

## Limits And Errors

The backend owns file policy. It enforces a maximum attachment count per message, a maximum individual image size, and a maximum combined native-image byte size per model request. These are explicit constants/configuration values with validation tests. The UI mirrors the limits for immediate feedback but the server remains authoritative.

If an image is deleted or changed after upload but before a turn is rendered, native input is omitted for that Agent and the runtime produces a bounded warning while retaining the durable attachment reference. A provider rejection for image input is surfaced as a turn failure/warning and does not silently retry as a text-only request, because that would hide a model configuration error.

Authorization and workspace path-safety checks are reused for all uploads, attachment references, previews, and runtime reads. Image preview/download endpoints return only files belonging to the authenticated conversation.

## Tests

Frontend tests cover file selection, external file drop, clipboard image paste, internal workspace path drop, attachment-only send, removal, upload failure state, and rendering of persisted attachments.

Backend/API tests cover attachment-reference validation, persistence, history response shape, authorization, text-only backward compatibility, message streaming with attachments, and missing-file handling.

Runtime and provider tests cover:

- Image classification for PNG, JPEG, WebP, and GIF only.
- Vision-enabled Agents receiving native image parts.
- Vision-disabled Agents receiving only controlled references.
- Non-image files never reaching native provider image fields.
- Correct OpenAI-compatible, Anthropic, and Gemini request mappings.
- Text-only messages preserving the existing provider request payloads.
- Count and size-limit enforcement.

## Out Of Scope

- PDF/document native input and text extraction.
- OCR or generated image descriptions.
- SVG/HEIC conversion.
- Audio/video input.
- Provider-hosted file upload APIs and remote file URLs.
- General-purpose binary data embedded in model requests.
