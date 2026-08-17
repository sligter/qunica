//! Keeping a thread inside its context window.
//!
//! The runtime previously sent the complete thread on every request: the loader
//! reads every visible message with no `LIMIT`, and a tool-calling turn appends
//! to that list without ever removing anything. A long session therefore grew
//! until the provider rejected it, and the rejection was not recoverable —
//! `is_transient_provider_error` did not recognise a context overflow, so the
//! turn failed outright and every retry failed the same way.
//!
//! Two reductions run here, cheapest first:
//!
//! 1. **Tool-result pruning** — deterministic, no model call. An old tool result
//!    keeps its head and tail with the middle elided. A single 12 000-character
//!    build log otherwise occupies every later request in the thread forever.
//! 2. **Summary compaction** — the oldest span of messages is replaced by one
//!    host-authored summary.
//!
//! Both preserve the invariant providers actually enforce: a `tool` message must
//! follow the assistant message that called it. A span boundary is never allowed
//! to leave an orphan tool result at the head of the retained tail.
//!
//! One piece of state is carried across rather than dropped: the agent's
//! `TodoWrite` checklist. It is the only tool result that says what the agent is
//! still supposed to do, so losing it to a summary is how a compacted agent
//! forgets the rest of its own plan.

use async_trait::async_trait;

use crate::llm::ChatMessage;
use crate::tools::todo::{self, TodoItem, TodoStatus};

/// Messages at the end of the thread that are never pruned or summarized.
///
/// Recent turns are what the model is actually working from; compacting them is
/// how an agent forgets the instruction it was given two messages ago.
pub const RETAINED_TAIL_MESSAGES: usize = 8;

/// A tool result longer than this is pruned once it leaves the retained tail.
pub const MAX_RETAINED_TOOL_RESULT_CHARS: usize = 4_000;

/// Characters of a pruned tool result kept from the start.
const PRUNE_HEAD_CHARS: usize = 1_200;
/// Characters of a pruned tool result kept from the end. Weighted towards the
/// tail for the same reason shell output is: errors come last.
const PRUNE_TAIL_CHARS: usize = 2_000;

/// Fraction of the usable window above which pressure compaction runs.
pub const PRESSURE_RATIO: f64 = 0.75;
/// Fraction of the usable window a compaction aims to land under.
const TARGET_RATIO: f64 = 0.55;

/// Why compaction is being considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTrigger {
    /// The estimated request is approaching the window.
    Pressure,
    /// The provider rejected the request as too long. Compaction runs
    /// regardless of the estimate, because the estimate was evidently wrong.
    Overflow,
}

/// What one compaction pass changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactionOutcome {
    /// Tool results whose middles were elided.
    pub pruned_results: usize,
    /// Characters removed by pruning.
    pub pruned_chars: usize,
    /// Messages replaced by the summary.
    pub summarized_messages: usize,
    /// Estimated tokens before and after.
    pub tokens_before: i64,
    pub tokens_after: i64,
}

impl CompactionOutcome {
    /// Whether the pass actually reduced anything.
    ///
    /// The retry path keys off this: re-issuing a request that compaction could
    /// not shrink would fail identically, so a pass that makes no progress
    /// surfaces the original provider error instead of looping.
    pub fn made_progress(&self) -> bool {
        self.pruned_chars > 0 || self.summarized_messages > 0
    }

    pub fn describe(&self) -> String {
        format!(
            "pruned {} tool result(s) (-{} chars), summarized {} message(s); estimated {} -> {} tokens",
            self.pruned_results,
            self.pruned_chars,
            self.summarized_messages,
            self.tokens_before,
            self.tokens_after
        )
    }
}

/// The window a thread has to fit into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompactionLimits {
    /// Tokens available for input after the provider's output reserve.
    pub usable_tokens: i64,
}

impl CompactionLimits {
    /// Derive limits from a provider's declared window, or `None` when it
    /// declares none.
    ///
    /// Pressure compaction does not run against a guessed window. Discarding
    /// history because of an invented number is worse than sending a request
    /// that fits, and the overflow path still recovers if it does not.
    pub fn from_provider(
        context_window_tokens: Option<i64>,
        output_reserve_ratio: Option<f64>,
    ) -> Option<Self> {
        let window = context_window_tokens.filter(|tokens| *tokens > 0)?;
        let reserve_ratio = output_reserve_ratio
            .filter(|ratio| ratio.is_finite() && *ratio >= 0.0 && *ratio < 1.0)
            .unwrap_or(0.0);
        let reserve = ((window as f64) * reserve_ratio).round() as i64;
        Some(Self {
            usable_tokens: (window - reserve).max(1),
        })
    }

    fn pressure_threshold(&self) -> i64 {
        ((self.usable_tokens as f64) * PRESSURE_RATIO) as i64
    }

    fn target(&self) -> i64 {
        ((self.usable_tokens as f64) * TARGET_RATIO) as i64
    }
}

/// Produces the replacement text for a compacted span.
///
/// A trait rather than a provider handle so the selection and splicing rules can
/// be tested without a model, and so a failed summary can fall back to a
/// host-authored placeholder without special-casing the caller.
#[async_trait]
pub trait Summarizer: Send + Sync {
    async fn summarize(&self, transcript: &str) -> anyhow::Result<String>;
}

/// Reduce `messages` in place.
///
/// Returns `None` when nothing needed to be done. `Some(outcome)` may still
/// report no progress — check [`CompactionOutcome::made_progress`] before
/// retrying a request on the strength of it.
pub async fn compact(
    messages: &mut Vec<ChatMessage>,
    limits: CompactionLimits,
    trigger: CompactionTrigger,
    summarizer: &dyn Summarizer,
) -> Option<CompactionOutcome> {
    let tokens_before = estimate_tokens(messages);
    if trigger == CompactionTrigger::Pressure && tokens_before <= limits.pressure_threshold() {
        return None;
    }

    let mut outcome = CompactionOutcome {
        tokens_before,
        tokens_after: tokens_before,
        ..Default::default()
    };

    let (pruned_results, pruned_chars) = prune_tool_results(messages);
    outcome.pruned_results = pruned_results;
    outcome.pruned_chars = pruned_chars;
    outcome.tokens_after = estimate_tokens(messages);

    // Pruning alone is often enough, and it costs no model call. This exit is
    // only sound under pressure: on overflow the provider has already proved the
    // request did not fit, so an estimate saying it now does is evidence of
    // nothing. Trusting it there would return "no progress" and turn a
    // recoverable overflow back into a failed turn.
    if trigger == CompactionTrigger::Pressure && outcome.tokens_after <= limits.target() {
        return Some(outcome);
    }

    if let Some(span) = select_span(messages) {
        let transcript = render_transcript(&messages[span.clone()]);
        // Read before the splice: after it, the messages holding the checklist
        // are gone.
        let carried = carried_checklist(messages, span.end);
        let summary = match summarizer.summarize(&transcript).await {
            Ok(summary) if !summary.trim().is_empty() => summary,
            // A failed or empty summary must not block the reduction: an
            // overflowing request that cannot be shrunk is a dead turn. Say
            // plainly what was dropped instead of inventing content.
            Ok(_) | Err(_) => format!(
                "{} earlier messages were dropped to stay inside the context window, and could \
                 not be summarized. Ask the user to restate anything you need from them.",
                span.end - span.start
            ),
        };
        outcome.summarized_messages = span.end - span.start;
        messages.splice(
            span,
            [summary_message(
                &summary,
                outcome.summarized_messages,
                carried.as_deref(),
            )],
        );
        outcome.tokens_after = estimate_tokens(messages);
    }

    Some(outcome)
}

/// Elide the middle of every oversized tool result outside the retained tail.
fn prune_tool_results(messages: &mut [ChatMessage]) -> (usize, usize) {
    let prunable = messages.len().saturating_sub(RETAINED_TAIL_MESSAGES);
    let mut count = 0;
    let mut removed = 0;
    for message in messages.iter_mut().take(prunable) {
        if message.role != "tool" {
            continue;
        }
        let Some(pruned) = prune_text(&message.content) else {
            continue;
        };
        // A checklist is bounded by construction and is the one tool result
        // whose middle cannot be recovered by running the tool again. Eliding
        // it would drop the items in the middle of the agent's own plan.
        if todo::todos_from_output(&message.content).is_some() {
            continue;
        }
        removed += message.content.chars().count() - pruned.chars().count();
        message.content = pruned;
        count += 1;
    }
    (count, removed)
}

/// Marks a checklist restated inside a summary message.
///
/// Written and read back: a long thread compacts more than once, and the second
/// pass has to find the list its predecessor carried, or the checklist survives
/// exactly one compaction.
const CHECKLIST_OPEN: &str = "<ag-swarmer-todo-checklist>";
const CHECKLIST_CLOSE: &str = "</ag-swarmer-todo-checklist>";

/// The checklist a summary should restate, if any.
///
/// `None` when the retained tail still carries one: that copy is current, and
/// restating an older list above it would tell the model two different things
/// about the same work. An empty list counts as a current one — an agent that
/// cleared its checklist has not lost it.
fn carried_checklist(messages: &[ChatMessage], tail_start: usize) -> Option<Vec<TodoItem>> {
    if latest_checklist(&messages[tail_start..]).is_some() {
        return None;
    }
    latest_checklist(&messages[..tail_start]).filter(|todos| !todos.is_empty())
}

/// The most recent checklist visible in `messages`.
///
/// Both forms one can take are read: the `TodoWrite` result the model produced,
/// and the block an earlier compaction restated it as.
fn latest_checklist(messages: &[ChatMessage]) -> Option<Vec<TodoItem>> {
    messages.iter().rev().find_map(|message| {
        if message.role == "tool" {
            return todo::todos_from_output(&message.content);
        }
        parse_checklist(&message.content)
    })
}

/// Render a checklist as the block a model reads instead of a tool result.
///
/// The status labels are the ones `TodoWrite`'s schema asks for, so the model
/// can send the list straight back with one of them changed.
fn render_checklist(todos: &[TodoItem]) -> String {
    let lines = todos
        .iter()
        .map(|todo| format!("- [{}] {}", todo.status.as_str(), todo.content))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{CHECKLIST_OPEN}\nThe checklist you were keeping when the messages above were \
         compacted away. It is still current and still yours; call TodoWrite to change \
         it.\n\n{lines}\n{CHECKLIST_CLOSE}"
    )
}

/// Read back a checklist [`render_checklist`] wrote.
fn parse_checklist(text: &str) -> Option<Vec<TodoItem>> {
    let block = text
        .split_once(CHECKLIST_OPEN)?
        .1
        .split_once(CHECKLIST_CLOSE)?
        .0;
    let todos: Vec<TodoItem> = block
        .lines()
        .filter_map(|line| {
            let (status, content) = line.trim().strip_prefix("- [")?.split_once("] ")?;
            let content = content.trim();
            (!content.is_empty()).then(|| TodoItem {
                content: content.to_string(),
                status: TodoStatus::parse(status),
            })
        })
        .collect();
    (!todos.is_empty()).then_some(todos)
}

/// Keep the head and tail of `text`, replacing the middle with a marker.
fn prune_text(text: &str) -> Option<String> {
    let total = text.chars().count();
    if total <= MAX_RETAINED_TOOL_RESULT_CHARS {
        return None;
    }
    let head: String = text.chars().take(PRUNE_HEAD_CHARS).collect();
    let tail: String = text.chars().skip(total - PRUNE_TAIL_CHARS).collect();
    let elided = total - PRUNE_HEAD_CHARS - PRUNE_TAIL_CHARS;
    Some(format!(
        "{head}\n[... {elided} characters elided from this tool result to stay inside the \
         context window; re-run the tool if you need the middle ...]\n{tail}"
    ))
}

/// The span of messages to replace with a summary.
///
/// Starts at 1 so the system prompt survives, and stops short of the retained
/// tail. The end is advanced past any `tool` message so the retained tail never
/// begins with a result whose call has been summarized away — providers reject
/// that outright.
fn select_span(messages: &[ChatMessage]) -> Option<std::ops::Range<usize>> {
    let start = 1;
    let mut end = messages.len().checked_sub(RETAINED_TAIL_MESSAGES)?;
    while end < messages.len() && messages[end].role == "tool" {
        end += 1;
    }
    // A span worth a model call and a splice.
    (end > start + 1).then_some(start..end)
}

/// Render a span as plain text for the summarizer.
fn render_transcript(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let mut line = format!("[{}] {}", message.role, message.content.trim());
            if !message.tool_calls.is_empty() {
                let calls = message
                    .tool_calls
                    .iter()
                    .map(|call| format!("{}({})", call.name, call.args))
                    .collect::<Vec<_>>()
                    .join(", ");
                line.push_str(&format!("\n[tool calls] {calls}"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Wrap `summary` in a host envelope, with the checklist appended when one had
/// to be carried across the compacted span.
///
/// Carried on a `user` message rather than an assistant one: the summary is
/// host-authored context, and presenting it as the model's own words would
/// invite it to treat invented detail as something it had already said.
fn summary_message(summary: &str, replaced: usize, checklist: Option<&[TodoItem]>) -> ChatMessage {
    let mut content = format!(
        "<ag-swarmer-context-summary replaced_messages=\"{replaced}\">\nThe earlier part of \
         this conversation was replaced by this summary to stay inside the context window. \
         Treat it as an accurate record of what happened, and say so if you need detail it \
         does not carry.\n\n{summary}\n</ag-swarmer-context-summary>"
    );
    // Outside the summary envelope: the checklist is not a record of what was
    // said, it is state the agent is still acting on, and a model told to treat
    // the summary as history would be right to treat it as finished business.
    if let Some(todos) = checklist {
        content.push_str("\n\n");
        content.push_str(&render_checklist(todos));
    }
    ChatMessage::text("user", content)
}

/// The prompt used to summarize a compacted span.
pub const SUMMARY_INSTRUCTION: &str = "You are compacting the earlier part of a working \
    conversation so it can be dropped from an agent's context window. Write a dense summary that \
    lets the agent continue without re-reading it. Cover, in this order and only where they \
    apply: what was asked for; decisions made and the reasons given; files, commands, and \
    identifiers touched, by exact name; results and errors that still matter; and anything left \
    open. Preserve exact names, paths, and numbers. Do not add anything that is not in the \
    transcript, do not editorialize, and do not address the reader.";

/// Estimate the token cost of `messages`.
///
/// A heuristic, not a tokenizer: no provider ships one for every model this app
/// can be pointed at. ASCII runs about four characters per token, while CJK
/// runs close to one token per character — a flat `chars / 4` underestimates a
/// Chinese thread by roughly 4x, which is precisely the case that overflows. The
/// two are counted separately for that reason. The overflow path does not depend
/// on this being right; it exists so pressure compaction has something to read.
pub fn estimate_tokens(messages: &[ChatMessage]) -> i64 {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &ChatMessage) -> i64 {
    /// Role, delimiters, and per-message framing the provider adds.
    const PER_MESSAGE_OVERHEAD: i64 = 8;

    let mut tokens = PER_MESSAGE_OVERHEAD + estimate_text_tokens(&message.content);
    for part in &message.parts {
        if let ag_swarmer_domain::runtime::ChatContentPart::Text { text } = part {
            tokens += estimate_text_tokens(text);
        }
    }
    for call in &message.tool_calls {
        tokens += estimate_text_tokens(&call.name) + estimate_text_tokens(&call.args.to_string());
    }
    if let Some(name) = message.tool_name.as_deref() {
        tokens += estimate_text_tokens(name);
    }
    tokens
}

/// Estimate the token cost of one run of text, with the same ASCII/CJK split
/// [`estimate_tokens`] uses per message.
pub fn estimate_text_tokens(text: &str) -> i64 {
    let mut ascii = 0i64;
    let mut wide = 0i64;
    for character in text.chars() {
        if character.is_ascii() {
            ascii += 1;
        } else {
            wide += 1;
        }
    }
    ascii / 4 + wide
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolCall;
    use serde_json::{json, Value};

    struct FixedSummarizer(&'static str);

    #[async_trait]
    impl Summarizer for FixedSummarizer {
        async fn summarize(&self, _transcript: &str) -> anyhow::Result<String> {
            Ok(self.0.to_string())
        }
    }

    struct FailingSummarizer;

    #[async_trait]
    impl Summarizer for FailingSummarizer {
        async fn summarize(&self, _transcript: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("summarizer unavailable"))
        }
    }

    /// `ChatMessage` has no `PartialEq`, so tests compare the fields that
    /// matter here: who said what, in what order.
    fn shape(messages: &[ChatMessage]) -> Vec<(String, String)> {
        messages
            .iter()
            .map(|message| (message.role.clone(), message.content.clone()))
            .collect()
    }

    fn thread(length: usize) -> Vec<ChatMessage> {
        let mut messages = vec![ChatMessage::text("system", "You are a test agent.")];
        for index in 0..length {
            messages.push(ChatMessage::text("user", format!("question {index}")));
            messages.push(ChatMessage::text("assistant", format!("answer {index}")));
        }
        messages
    }

    fn tool_exchange(output: &str) -> Vec<ChatMessage> {
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            args: json!({ "command": "cargo build" }),
            provider_metadata: None,
        };
        vec![
            ChatMessage::assistant_tool_calls("", vec![call.clone()]),
            ChatMessage::tool_result(call.id, call.name, output.to_string()),
        ]
    }

    /// A `TodoWrite` exchange exactly as the runtime appends it, framing and
    /// all: the whole point is that compaction reads what actually reaches the
    /// model, not a tidied-up copy.
    fn todo_exchange(todos: &[(&str, &str)]) -> Vec<ChatMessage> {
        let items: Vec<Value> = todos
            .iter()
            .map(|(content, status)| json!({ "content": content, "status": status }))
            .collect();
        let call = ToolCall {
            id: "call-todo".to_string(),
            name: "TodoWrite".to_string(),
            args: json!({ "todos": items }),
            provider_metadata: None,
        };
        let output = json!({ "tool": "TodoWrite", "status": "COMPLETED", "todos": items });
        vec![
            ChatMessage::assistant_tool_calls("", vec![call.clone()]),
            ChatMessage::tool_result(call.id, call.name, format!("status: Completed\n{output}")),
        ]
    }

    fn compacted_text(messages: &[ChatMessage]) -> String {
        messages
            .iter()
            .find(|message| message.content.contains("ag-swarmer-context-summary"))
            .expect("a summary should have been spliced in")
            .content
            .clone()
    }

    #[test]
    fn cjk_text_is_not_estimated_as_a_quarter_token_per_character() {
        let ascii = estimate_text_tokens(&"a".repeat(400));
        let chinese = estimate_text_tokens(&"文".repeat(400));
        assert_eq!(ascii, 100);
        assert_eq!(chinese, 400);
    }

    #[test]
    fn limits_are_only_derived_from_a_declared_window() {
        assert_eq!(CompactionLimits::from_provider(None, Some(0.2)), None);
        assert_eq!(CompactionLimits::from_provider(Some(0), None), None);
        assert_eq!(
            CompactionLimits::from_provider(Some(100_000), Some(0.2))
                .unwrap()
                .usable_tokens,
            80_000
        );
        assert_eq!(
            CompactionLimits::from_provider(Some(100_000), None)
                .unwrap()
                .usable_tokens,
            100_000
        );
    }

    #[tokio::test]
    async fn pressure_below_the_threshold_does_nothing() {
        let mut messages = thread(3);
        let before = shape(&messages);
        let outcome = compact(
            &mut messages,
            CompactionLimits {
                usable_tokens: 100_000,
            },
            CompactionTrigger::Pressure,
            &FixedSummarizer("unused"),
        )
        .await;
        assert!(outcome.is_none());
        assert_eq!(shape(&messages), before);
    }

    #[tokio::test]
    async fn pruning_alone_is_preferred_over_a_model_call() {
        let mut messages = thread(1);
        messages.extend(tool_exchange(&"x".repeat(30_000)));
        messages.extend(thread(4).into_iter().skip(1));

        let outcome = compact(
            &mut messages,
            CompactionLimits {
                usable_tokens: 9_000,
            },
            CompactionTrigger::Pressure,
            &FailingSummarizer,
        )
        .await
        .expect("pressure should trigger");

        assert_eq!(outcome.pruned_results, 1);
        assert!(outcome.pruned_chars > 25_000, "{outcome:?}");
        assert_eq!(
            outcome.summarized_messages, 0,
            "pruning was enough, so no span should have been summarized"
        );
        assert!(outcome.tokens_after < outcome.tokens_before);
        assert!(messages
            .iter()
            .any(|m| m.content.contains("characters elided")));
    }

    #[tokio::test]
    async fn a_span_is_replaced_by_one_summary_message() {
        let mut messages = thread(20);
        let original = messages.len();

        let outcome = compact(
            &mut messages,
            CompactionLimits { usable_tokens: 100 },
            CompactionTrigger::Pressure,
            &FixedSummarizer("The agent answered twenty questions."),
        )
        .await
        .expect("pressure should trigger");

        assert!(outcome.summarized_messages > 0);
        assert_eq!(messages.len(), original - outcome.summarized_messages + 1);
        assert_eq!(messages[0].role, "system", "the system prompt must survive");
        assert!(messages[1]
            .content
            .contains("The agent answered twenty questions."));
        assert!(messages[1].content.contains("ag-swarmer-context-summary"));
        assert!(outcome.made_progress());
    }

    #[tokio::test]
    async fn a_failed_summary_still_reduces_and_says_so() {
        let mut messages = thread(20);
        let outcome = compact(
            &mut messages,
            CompactionLimits { usable_tokens: 100 },
            CompactionTrigger::Overflow,
            &FailingSummarizer,
        )
        .await
        .expect("overflow always runs");

        assert!(outcome.made_progress());
        assert!(messages[1].content.contains("could not be summarized"));
    }

    #[tokio::test]
    async fn overflow_compacts_even_when_the_estimate_looks_fine() {
        let mut messages = thread(20);
        let outcome = compact(
            &mut messages,
            CompactionLimits {
                usable_tokens: 1_000_000,
            },
            CompactionTrigger::Overflow,
            &FixedSummarizer("summary"),
        )
        .await
        .expect("overflow ignores the estimate");
        assert!(outcome.made_progress());
    }

    #[test]
    fn a_span_never_leaves_an_orphan_tool_result_at_the_head_of_the_tail() {
        // The tail boundary lands mid tool exchange; the span must extend past
        // the result so the retained tail starts with a complete message.
        let mut messages = thread(2);
        messages.extend(tool_exchange("build output"));
        messages.extend(thread(3).into_iter().skip(1));

        let span = select_span(&messages).expect("a span should be selectable");
        assert_eq!(span.start, 1);
        assert_ne!(
            messages[span.end].role, "tool",
            "the retained tail must not begin with a tool result"
        );
    }

    #[test]
    fn a_thread_shorter_than_the_retained_tail_has_no_span() {
        assert!(select_span(&thread(1)).is_none());
        assert!(select_span(&[]).is_none());
    }

    #[test]
    fn pruning_keeps_both_ends_of_a_tool_result() {
        let text = format!("HEAD{}TAIL", "m".repeat(20_000));
        let pruned = prune_text(&text).unwrap();
        assert!(pruned.starts_with("HEAD"));
        assert!(pruned.ends_with("TAIL"));
        assert!(pruned.chars().count() < text.chars().count());
        assert!(prune_text("short").is_none());
    }

    #[test]
    fn the_retained_tail_is_never_pruned() {
        let mut messages = vec![ChatMessage::text("system", "s")];
        messages.extend(tool_exchange(&"x".repeat(30_000)));
        let (count, removed) = prune_tool_results(&mut messages);
        assert_eq!((count, removed), (0, 0));
    }

    #[tokio::test]
    async fn a_summarized_checklist_is_carried_across_verbatim() {
        let mut messages = thread(2);
        messages.extend(todo_exchange(&[
            ("read the code", "completed"),
            ("write the fix", "in_progress"),
            ("run the tests", "pending"),
        ]));
        messages.extend(thread(20).into_iter().skip(1));

        let outcome = compact(
            &mut messages,
            CompactionLimits { usable_tokens: 100 },
            CompactionTrigger::Pressure,
            // The summarizer is told nothing about todos on purpose: carrying
            // the checklist must not depend on a model noticing it.
            &FixedSummarizer("The agent did some work."),
        )
        .await
        .expect("pressure should trigger");
        assert!(outcome.summarized_messages > 0);

        let summary = compacted_text(&messages);
        assert!(summary.contains("- [completed] read the code"), "{summary}");
        assert!(
            summary.contains("- [in_progress] write the fix"),
            "{summary}"
        );
        assert!(summary.contains("- [pending] run the tests"), "{summary}");
        // Outside the summary envelope: the checklist is live state, not a
        // record of what was already said.
        assert!(
            summary.find(CHECKLIST_OPEN) > summary.find("</ag-swarmer-context-summary>"),
            "{summary}"
        );
    }

    #[tokio::test]
    async fn a_checklist_still_in_the_retained_tail_is_not_restated() {
        let mut messages = thread(20);
        messages.extend(todo_exchange(&[("write the fix", "in_progress")]));

        compact(
            &mut messages,
            CompactionLimits { usable_tokens: 100 },
            CompactionTrigger::Pressure,
            &FixedSummarizer("summary"),
        )
        .await
        .expect("pressure should trigger");

        // The tool result the model can already read is the current one. A
        // second copy above it would be one more thing to reconcile.
        assert!(!compacted_text(&messages).contains(CHECKLIST_OPEN));
    }

    #[tokio::test]
    async fn a_carried_checklist_survives_the_next_compaction_too() {
        let mut messages = thread(2);
        messages.extend(todo_exchange(&[("run the tests", "in_progress")]));
        messages.extend(thread(20).into_iter().skip(1));
        let limits = CompactionLimits { usable_tokens: 100 };

        compact(
            &mut messages,
            limits,
            CompactionTrigger::Pressure,
            &FixedSummarizer("first pass"),
        )
        .await
        .expect("the first pass should compact");
        // Grow the thread again so there is a fresh span to summarize, one that
        // now contains the carried block rather than the original tool result.
        messages.extend(thread(20).into_iter().skip(1));
        compact(
            &mut messages,
            limits,
            CompactionTrigger::Pressure,
            &FixedSummarizer("second pass"),
        )
        .await
        .expect("the second pass should compact");

        let summary = compacted_text(&messages);
        assert!(
            summary.contains("- [in_progress] run the tests"),
            "a checklist must not survive exactly one compaction: {summary}"
        );
    }

    #[test]
    fn a_cleared_checklist_is_not_replaced_by_an_older_one() {
        let mut messages = thread(1);
        messages.extend(todo_exchange(&[("write the fix", "in_progress")]));
        messages.extend(todo_exchange(&[]));
        let tail_start = messages.len();
        assert_eq!(carried_checklist(&messages, tail_start), None);
    }

    #[test]
    fn a_checklist_is_never_pruned_down_the_middle() {
        let long: Vec<(String, &str)> = (0..20)
            .map(|index| (format!("step {index} {}", "x".repeat(200)), "pending"))
            .collect();
        let borrowed: Vec<(&str, &str)> = long
            .iter()
            .map(|(content, status)| (content.as_str(), *status))
            .collect();
        let mut messages = vec![ChatMessage::text("system", "s")];
        messages.extend(todo_exchange(&borrowed));
        messages.extend(thread(6).into_iter().skip(1));

        let (count, removed) = prune_tool_results(&mut messages);
        assert_eq!(
            (count, removed),
            (0, 0),
            "an oversized checklist is still a checklist"
        );
        assert_eq!(latest_checklist(&messages).unwrap().len(), 20);
    }

    #[test]
    fn a_rendered_checklist_reads_back_as_the_same_items() {
        let todos = vec![
            TodoItem {
                content: "read the code".to_string(),
                status: TodoStatus::Completed,
            },
            TodoItem {
                content: "write the fix".to_string(),
                status: TodoStatus::InProgress,
            },
        ];
        assert_eq!(parse_checklist(&render_checklist(&todos)).unwrap(), todos);
        assert_eq!(parse_checklist("no checklist here"), None);
    }
}
