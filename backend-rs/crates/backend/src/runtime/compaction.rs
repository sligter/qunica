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
use qunica_domain::runtime::ToolDefinition;

/// Turns at the end of the thread that are never pruned or summarized under
/// normal pressure.
///
/// A turn is one interaction unit — a user message, a plain assistant reply, or
/// a whole `assistant(calls) -> tool* -> assistant(text)` exchange — so tool
/// results never inflate the count (see [`is_turn_head`]). Recent turns are what
/// the model is actually working from; compacting them is how an agent forgets
/// the instruction it was given two turns ago.
pub const RETAINED_TAIL_TURNS: usize = 8;

/// The retained tail in emergency and overflow mode: only the most recent
/// couple of turns survive intact.
///
/// This is the "fire drill" tier — the space to land under has to come from
/// somewhere, and squeezing the tail instead of repeating the summary is what
/// distinguishes it from a normal pass. It mirrors the article's last two
/// layers: near history stays whole under pressure, but at 92% only a skeleton
/// does.
const EMERGENCY_RETAINED_TAIL_TURNS: usize = 2;

/// A tool result longer than this is pruned once it leaves the retained tail.
///
/// This is the strict threshold for the prefix being summarized away. The tail,
/// once it alone exceeds the ceiling, is pruned at the laxer
/// [`TAIL_RETAINED_TOOL_RESULT_CHARS`].
pub const MAX_RETAINED_TOOL_RESULT_CHARS: usize = 4_000;

/// A tool result in the retained tail longer than this is pruned once the tail
/// exceeds its token ceiling. Laxer than the prefix: the recent context is what
/// the model works from, so more of it survives.
const TAIL_RETAINED_TOOL_RESULT_CHARS: usize = 8_000;

/// Fraction of the usable window the retained tail may occupy before its tool
/// results are pruned. Leaves room for the preserved prefix and the summary
/// inside [`TARGET_RATIO`].
const TAIL_CEILING_RATIO: f64 = 0.40;

/// Characters of a pruned tool result kept from the start.
const PRUNE_HEAD_CHARS: usize = 1_200;
/// Characters of a pruned tool result kept from the end. Weighted towards the
/// tail for the same reason shell output is: errors come last.
const PRUNE_TAIL_CHARS: usize = 2_000;

/// Fraction of the usable window above which pressure compaction runs.
pub const PRESSURE_RATIO: f64 = 0.75;
/// Fraction of the usable window above which the emergency tier runs.
///
/// Above this the thread is one request away from being rejected, so a normal
/// pass that lands under the target is no longer enough: the tail is squeezed
/// so the summary genuinely shrinks the request.
pub const EMERGENCY_RATIO: f64 = 0.92;
/// Fraction of the usable window a compaction aims to land under.
const TARGET_RATIO: f64 = 0.55;

/// Why compaction is being considered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTrigger {
    /// The estimated request is approaching the window.
    Pressure,
    /// The estimated request has blown past the emergency threshold; compress
    /// harder than pressure so it lands well under before the provider refuses.
    Emergency,
    /// The provider rejected the request as too long. Compaction runs
    /// regardless of the estimate, because the estimate was evidently wrong.
    Overflow,
}

/// What one compaction pass changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactionOutcome {
    /// Silent empty messages dropped by the rule pass.
    pub snipped_messages: usize,
    /// Characters removed by the rule pass (collapsed repeated lines).
    pub snipped_chars: usize,
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
        self.snipped_chars > 0
            || self.snipped_messages > 0
            || self.pruned_chars > 0
            || self.summarized_messages > 0
    }

    pub fn describe(&self) -> String {
        format!(
            "snipped {} message(s) (-{} chars), pruned {} tool result(s) (-{} chars), summarized {} message(s); estimated {} -> {} tokens",
            self.snipped_messages,
            self.snipped_chars,
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
    /// Tokens available for the message list, after the provider's output
    /// reserve and the fixed per-request overhead the messages share the window
    /// with.
    pub usable_tokens: i64,
}

impl CompactionLimits {
    /// Derive limits from a provider's declared window, or `None` when it
    /// declares none.
    ///
    /// Pressure compaction does not run against a guessed window. Discarding
    /// history because of an invented number is worse than sending a request
    /// that fits, and the overflow path still recovers if it does not.
    ///
    /// `fixed_overhead_tokens` is everything in the request that is not a
    /// message and that compaction cannot shrink — in practice the tool
    /// schemas. Leaving it out is why the threshold used to be measured against
    /// the wrong number: an agent with four MCP servers mounted carries tens of
    /// thousands of tokens of tool definitions on every request, so a message
    /// list at "75% of the window" was really at 90% of it, and pressure
    /// compaction stopped firing before the provider rejected the request.
    pub fn from_provider(
        context_window_tokens: Option<i64>,
        output_reserve_ratio: Option<f64>,
        fixed_overhead_tokens: i64,
    ) -> Option<Self> {
        let window = context_window_tokens.filter(|tokens| *tokens > 0)?;
        let reserve_ratio = output_reserve_ratio
            .filter(|ratio| ratio.is_finite() && *ratio >= 0.0 && *ratio < 1.0)
            .unwrap_or(0.0);
        let reserve = ((window as f64) * reserve_ratio).round() as i64;
        Some(Self {
            usable_tokens: (window - reserve - fixed_overhead_tokens.max(0)).max(1),
        })
    }

    fn pressure_threshold(&self) -> i64 {
        ((self.usable_tokens as f64) * PRESSURE_RATIO) as i64
    }

    fn emergency_threshold(&self) -> i64 {
        ((self.usable_tokens as f64) * EMERGENCY_RATIO) as i64
    }

    fn target(&self) -> i64 {
        ((self.usable_tokens as f64) * TARGET_RATIO) as i64
    }

    /// The token budget the retained tail may occupy before its tool results
    /// are pruned.
    fn tail_ceiling(&self) -> i64 {
        ((self.usable_tokens as f64) * TAIL_CEILING_RATIO) as i64
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

    /// Tokens the summarizer has spent so far, when it spends any.
    ///
    /// Summarizing is a real provider call — the same billable boundary as the
    /// request it is shrinking — and it is subject to the same turn budget. A
    /// summarizer that spends nothing (unavailable, or a test double) reports
    /// zero; the caller records the delta after each pass.
    fn claimed_tokens(&self) -> u64 {
        0
    }
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
    // A pressure pass that has already blown past the emergency ceiling needs
    // the drastic tail, not the normal one: the normal pass will not shrink it
    // enough to land under 55%, and the provider is one estimate away from
    // refusing it.
    let trigger =
        if trigger == CompactionTrigger::Pressure && tokens_before > limits.emergency_threshold() {
            CompactionTrigger::Emergency
        } else {
            trigger
        };
    let mut outcome = CompactionOutcome {
        tokens_before,
        tokens_after: tokens_before,
        ..Default::default()
    };

    // Layer one, cheapest: pure rules, no model call. Drop silent messages and
    // collapse runs of identical lines inside tool results. This runs before
    // pruning because it is free, so it earns the easy reductions before the
    // deterministic head/tail cut has to fire.
    let snip = snip_compact(messages);
    outcome.snipped_messages = snip.dropped_messages;
    outcome.snipped_chars = snip.snipped_chars;

    // Computed after snip: dropping silent messages shifts indices, so the turn
    // boundary must be read against the already-trimmed list.
    let tail_start = retained_tail_start(messages, trigger);
    let (pruned_results, pruned_chars) = prune_tool_results(messages, tail_start, limits);
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

    if let Some(span) = select_span(messages, tail_start, limits) {
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

/// What the rule-based pre-pass removed.
struct SnipOutcome {
    dropped_messages: usize,
    snipped_chars: usize,
}

/// Layer one of the funnel: pure rules, no model call.
///
/// * Drops silent messages — ones carrying no content, no tool calls, no
///   reasoning, and no parts. A `tool` result is never dropped, because even an
///   empty result is a statement that the call ran, and removing it would
///   orphan the assistant tool call that asked for it.
/// * Collapses runs of identical consecutive lines inside a tool result. This is
///   the repeated-stack-trace case: one line printed five hundred times carries
///   every byte of the information a single copy does, and it is exactly the
///   cheap cleanup that should not wait for the deterministic head/tail cut.
fn snip_compact(messages: &mut Vec<ChatMessage>) -> SnipOutcome {
    let mut dropped_messages = 0;
    let mut snipped_chars = 0;
    let original = std::mem::take(messages);
    let mut kept = Vec::with_capacity(original.len());
    for mut message in original {
        let silent = message.role != "tool"
            && message.content.trim().is_empty()
            && message.tool_calls.is_empty()
            && message.parts.is_empty()
            && message
                .reasoning_content
                .as_deref()
                .is_none_or(|reasoning| reasoning.trim().is_empty());
        if silent {
            dropped_messages += 1;
            continue;
        }
        if message.role == "tool" {
            if let Some(collapsed) = collapse_repeated_lines(&message.content) {
                snipped_chars += message.content.chars().count() - collapsed.chars().count();
                message.content = collapsed;
            }
        }
        kept.push(message);
    }
    *messages = kept;
    SnipOutcome {
        dropped_messages,
        snipped_chars,
    }
}

/// Consecutive identical non-empty lines beyond the threshold collapse to one
/// copy plus a marker, or `None` when nothing was repeated.
const REPEATED_LINE_RUN_THRESHOLD: usize = 4;

fn collapse_repeated_lines(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let mut run = 1;
        while index + run < lines.len() && lines[index + run] == line {
            run += 1;
        }
        if !line.trim().is_empty() && run > REPEATED_LINE_RUN_THRESHOLD {
            out.push(line);
            out.push("[... dropped a run of repeated identical lines; one copy kept ...]");
            changed = true;
        } else {
            out.extend_from_slice(&lines[index..index + run]);
        }
        index += run;
    }
    changed.then(|| out.join("\n"))
}

/// Elide oversized tool results so the request stays inside its budget.
///
/// Two passes. Everything outside the retained tail is pruned at the strict
/// threshold, because it is about to be summarized anyway and only the head and
/// tail need survive. Inside the tail, pruning only fires when the tail alone
/// exceeds its ceiling, oldest result first at the laxer threshold, so the
/// recent context the model works from is the last thing to go.
fn prune_tool_results(
    messages: &mut [ChatMessage],
    tail_start: usize,
    limits: CompactionLimits,
) -> (usize, usize) {
    let mut count = 0;
    let mut removed = 0;

    for message in messages.iter_mut().take(tail_start) {
        let (c, r) = prune_tool_result(message, MAX_RETAINED_TOOL_RESULT_CHARS);
        count += c;
        removed += r;
    }

    let ceiling = limits.tail_ceiling();
    let mut tail_tokens = estimate_tokens(&messages[tail_start..]);
    let mut index = tail_start;
    while index < messages.len() && tail_tokens > ceiling {
        let (c, r) = prune_tool_result(&mut messages[index], TAIL_RETAINED_TOOL_RESULT_CHARS);
        if c > 0 {
            tail_tokens = estimate_tokens(&messages[tail_start..]);
        }
        count += c;
        removed += r;
        index += 1;
    }

    (count, removed)
}

/// Prune one message in place, returning the result pruned and characters
/// removed. A checklist is never touched, and short results pass through.
fn prune_tool_result(message: &mut ChatMessage, max_chars: usize) -> (usize, usize) {
    if message.role != "tool" {
        return (0, 0);
    }
    // A checklist is bounded by construction and is the one tool result whose
    // middle cannot be recovered by running the tool again. Eliding it would
    // drop the items in the middle of the agent's own plan.
    if todo::todos_from_output(&message.content).is_some() {
        return (0, 0);
    }
    let Some(pruned) = prune_text_budget(&message.content, max_chars) else {
        return (0, 0);
    };
    let removed = message.content.chars().count() - pruned.chars().count();
    message.content = pruned;
    (1, removed)
}

/// Marks a checklist restated inside a summary message.
///
/// Written and read back: a long thread compacts more than once, and the second
/// pass has to find the list its predecessor carried, or the checklist survives
/// exactly one compaction.
const CHECKLIST_OPEN: &str = "<qunica-todo-checklist>";
const CHECKLIST_CLOSE: &str = "</qunica-todo-checklist>";

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

/// Keep the head and tail of `text`, replacing the middle with a marker, at the
/// given size threshold. The summarized prefix uses
/// [`MAX_RETAINED_TOOL_RESULT_CHARS`] and the tail the laxer
/// [`TAIL_RETAINED_TOOL_RESULT_CHARS`].
fn prune_text_budget(text: &str, max_chars: usize) -> Option<String> {
    let total = text.chars().count();
    if total <= max_chars {
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

/// The longest opening message compaction will pin.
///
/// A pinned message can never be reduced, so pinning an enormous one (a pasted
/// log, a whole file) would hand the overflow path a floor it cannot get under.
/// Past this size the task statement is summarized like anything else.
const MAX_PINNED_TASK_CHARS: usize = 4_000;

/// The number of leading messages compaction never touches.
///
/// The system prompt, and the first thing the user said. That opening message
/// is the task the thread exists to do, and it is the clearest example of
/// context that cannot be rebuilt by re-running anything: a file can be read
/// again and a command run again, but "only use the native APIs, no lodash" is
/// said once. Leaving it in the summarizable span made it the *first* thing to
/// go, and when the summarizer failed the fallback text dropped it outright.
fn preserved_prefix(messages: &[ChatMessage]) -> usize {
    let pinnable = messages.get(1).is_some_and(|message| {
        message.role == "user" && message.content.chars().count() <= MAX_PINNED_TASK_CHARS
    });
    if pinnable {
        2
    } else {
        1
    }
}

/// The message index at which the retained tail begins for a trigger.
///
/// The tail is measured in turns, not messages: a turn carries its tool results
/// with it, so a heavy tool exchange does not crowd out the turns around it.
/// The emergency and overflow tiers keep only a skeleton — the whole point of
/// those passes is to shrink the request for real.
fn retained_tail_start(messages: &[ChatMessage], trigger: CompactionTrigger) -> usize {
    let turns = match trigger {
        CompactionTrigger::Pressure => RETAINED_TAIL_TURNS,
        CompactionTrigger::Emergency | CompactionTrigger::Overflow => EMERGENCY_RETAINED_TAIL_TURNS,
    };
    let start = preserved_prefix(messages);
    let heads: Vec<usize> = (start..messages.len())
        .filter(|&index| is_turn_head(messages, index))
        .collect();
    if heads.len() <= turns {
        start
    } else {
        heads[heads.len() - turns]
    }
}

/// Whether `index` can start the retained tail without cutting a turn in two.
///
/// A `tool` result is never a turn start: it answers the assistant call before
/// it. An `assistant` message starts a turn only when it made the calls
/// (carries `tool_calls`) or is a plain reply not immediately preceded by a
/// tool result; an assistant right after `tool` messages is the conclusion of
/// that exchange, not a place to resume from.
fn is_turn_head(messages: &[ChatMessage], index: usize) -> bool {
    match messages[index].role.as_str() {
        "assistant" => {
            !messages[index].tool_calls.is_empty()
                || index == 0
                || messages[index - 1].role != "tool"
        }
        "tool" => false,
        _ => true,
    }
}

/// Step `end` back to the start of the turn it touches, if it touches one.
///
/// A boundary landing on a tool result or on the answer that concludes an
/// exchange cuts that turn in half: the summary keeps the call and its results
/// while the tail keeps only the answer. Retreating to the `assistant` message
/// that made the calls keeps the whole turn in the tail.
fn retreat_to_turn_head(messages: &[ChatMessage], mut end: usize, start: usize) -> usize {
    while end > start && !is_turn_head(messages, end) {
        end -= 1;
    }
    end
}

/// Step `end` forward past the turn it touches so the whole turn is summarized.
fn advance_to_turn_head(messages: &[ChatMessage], mut end: usize) -> usize {
    while end < messages.len() && !is_turn_head(messages, end) {
        end += 1;
    }
    end
}

/// The span of messages to replace with a summary.
///
/// Starts after the preserved prefix and stops short of the retained tail. The
/// boundary is snapped to a turn start rather than left at the raw index:
/// a raw cut landing mid-turn would otherwise leave the
/// turn's answer in the tail while summarizing the call and results that
/// produced it, or orphan a tool result at the head of the tail.
///
/// The straddling turn goes, whole, one way or the other. It stays in the tail
/// when that leaves the span non-empty and the tail affordable enough to keep;
/// otherwise the whole turn is summarized too. The affordability check bounds
/// the tail alone — a heuristic, not a landing guarantee — and only moves the
/// boundary when the snap does. A tail whose raw boundary is already a turn
/// start is left as-is: trimming an oversized-but-clean tail is the job of
/// tail-ceiling pruning, not of boundary snapping.
fn select_span(
    messages: &[ChatMessage],
    raw: usize,
    limits: CompactionLimits,
) -> Option<std::ops::Range<usize>> {
    let start = preserved_prefix(messages);

    let retreat = retreat_to_turn_head(messages, raw, start);
    let advance = advance_to_turn_head(messages, raw);

    let keep_tail = retreat > start + 1 && estimate_tokens(&messages[retreat..]) <= limits.target();
    let end = if keep_tail { retreat } else { advance };

    // A span worth a model call and a splice.
    (end > start + 1).then_some(start..end)
}

/// Render a span as plain text for the summarizer.
fn render_transcript(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let mut line = format!("[{}] {}", message.role, message.content.trim());
            // Why a decision was made is not recoverable by re-running
            // anything: the model does not reproduce the same reasoning on a
            // second pass. It is replayed into history, so it is in the span
            // being dropped — a summarizer that never sees it can only record
            // what was done, not what it was for.
            if let Some(reasoning) = message.reasoning_content.as_deref() {
                let reasoning = reasoning.trim();
                if !reasoning.is_empty() {
                    line.push_str(&format!("\n[thinking] {reasoning}"));
                }
            }
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
        "<qunica-context-summary replaced_messages=\"{replaced}\">\nThe earlier part of \
         this conversation was replaced by this summary to stay inside the context window. \
         Treat it as an accurate record of what happened, and say so if you need detail it \
         does not carry.\n\n{summary}\n</qunica-context-summary>"
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
///
/// A fixed field template rather than a free-form essay. Fielded summaries lose
/// less than prose does — they force the model to name the few things a
/// compacted agent actually cannot recover by re-reading the transcript, and
/// they stay comparable from one pass to the next, which is what lets a long
/// thread compact twice without the second pass forgetting what the first
/// carried. The highest-value fields are the ones a re-run cannot rebuild:
/// decisions and their reasons, and side effects already performed.
pub const SUMMARY_INSTRUCTION: &str = "You are compacting the earlier part of a working \
    conversation so it can be dropped from an agent's context window. Write a dense summary that \
    lets the agent continue without re-reading it. Fill the following fields in order; leave a \
    field out entirely when the transcript gives you nothing for it. Preserve exact names, paths, \
    ids, and numbers. Do not add anything not in the transcript, do not editorialize, and do not \
    address the reader.\n\n\
    ## Original task\n\
    What the user originally asked for, in one or two sentences.\n\n\
    ## Completed work\n\
    What has been done so far, as a short checklist.\n\n\
    ## Files and identifiers touched\n\
    Exact paths, commands, symbols, and ids, with what changed.\n\n\
    ## Side effects already performed\n\
    Irreversible or stateful actions that must not be repeated: commands run (builds, installs, \
    tests), files deployed, database writes, git commits, media or files generated. This is the \
    field that prevents the agent from doing the same thing twice when it resumes.\n\n\
    ## Decisions and constraints\n\
    Decisions made and why; constraints the user stated (preferences, bans, style rules). These \
    are stated once and cannot be recovered by re-running anything, so record them verbatim where \
    possible.\n\n\
    ## Results and errors that still matter\n\
    Test outcomes, failures, and the facts the next step depends on.\n\n\
    ## Open questions / next steps\n\
    What remains to be done or decided.";

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

/// Estimate the token cost of the tool definitions sent with every request.
///
/// Not part of [`estimate_tokens`] because it is not part of what compaction
/// can reduce: the schemas are fixed for the turn, so they belong in the
/// limits (see [`CompactionLimits::from_provider`]) rather than in the number
/// measured against them. They are far from negligible — an MCP server may
/// publish up to [`crate::mcp::protocol::MAX_TOOLS_PER_SERVER`] tools, and each
/// carries a name, a description, and a JSON Schema.
pub fn estimate_tool_schema_tokens(tools: &[ToolDefinition]) -> i64 {
    /// Per-tool framing the provider adds around each definition.
    const PER_TOOL_OVERHEAD: i64 = 8;

    tools
        .iter()
        .map(|tool| {
            PER_TOOL_OVERHEAD
                + estimate_text_tokens(&tool.name)
                + estimate_text_tokens(&tool.description)
                + estimate_text_tokens(&tool.input_schema.to_string())
        })
        .sum()
}

fn estimate_message_tokens(message: &ChatMessage) -> i64 {
    /// Role, delimiters, and per-message framing the provider adds.
    const PER_MESSAGE_OVERHEAD: i64 = 8;

    let mut tokens = PER_MESSAGE_OVERHEAD;
    // `ChatMessage::with_parts` derives `content` by concatenating the text
    // parts, so counting both is counting the same text twice.
    if message.parts.is_empty() {
        tokens += estimate_text_tokens(&message.content);
    } else {
        for part in &message.parts {
            tokens += match part {
                qunica_domain::runtime::ChatContentPart::Text { text } => {
                    estimate_text_tokens(text)
                }
                qunica_domain::runtime::ChatContentPart::Image { data_base64, .. } => {
                    estimate_image_tokens(data_base64)
                }
            };
        }
    }
    for call in &message.tool_calls {
        tokens += estimate_text_tokens(&call.name) + estimate_text_tokens(&call.args.to_string());
    }
    if let Some(name) = message.tool_name.as_deref() {
        tokens += estimate_text_tokens(name);
    }
    // Reasoning travels back to providers configured for passback, so a thread
    // of long thinking blocks costs what it costs whether or not this counts it.
    if let Some(reasoning) = message.reasoning_content.as_deref() {
        tokens += estimate_text_tokens(reasoning);
    }
    tokens
}

/// Estimate the token cost of one inline image.
///
/// Providers charge images by pixel count, which the encoded bytes do not
/// reveal without decoding them, so this is a coarse proxy over the decoded
/// size, bounded by the range vision models actually bill (roughly 85 tokens
/// for a thumbnail up to ~1600 for a full-resolution image). Deliberately
/// biased high: an image counted as free is how a thread of four attachments
/// sails past the pressure threshold and is rejected by the provider instead.
fn estimate_image_tokens(data_base64: &str) -> i64 {
    let bytes = (data_base64.len() as i64) * 3 / 4;
    (bytes / 600).clamp(85, 1_600)
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
            .find(|message| message.content.contains("qunica-context-summary"))
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
        assert_eq!(CompactionLimits::from_provider(None, Some(0.2), 0), None);
        assert_eq!(CompactionLimits::from_provider(Some(0), None, 0), None);
        assert_eq!(
            CompactionLimits::from_provider(Some(100_000), Some(0.2), 0)
                .unwrap()
                .usable_tokens,
            80_000
        );
        assert_eq!(
            CompactionLimits::from_provider(Some(100_000), None, 0)
                .unwrap()
                .usable_tokens,
            100_000
        );
    }

    #[test]
    fn tool_schemas_come_out_of_the_message_budget() {
        // Mounting a few MCP servers is worth tens of thousands of tokens the
        // messages never get to use.
        assert_eq!(
            CompactionLimits::from_provider(Some(100_000), Some(0.2), 30_000)
                .unwrap()
                .usable_tokens,
            50_000
        );
        // A window smaller than its own overhead still leaves a positive
        // budget, so the caller divides by something rather than nothing.
        assert_eq!(
            CompactionLimits::from_provider(Some(1_000), None, 5_000)
                .unwrap()
                .usable_tokens,
            1
        );
    }

    #[test]
    fn tool_definitions_are_not_free() {
        let tools = vec![ToolDefinition {
            name: "Read".to_string(),
            description: "Read a file from the workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "file_path": { "type": "string" } },
                "required": ["file_path"]
            }),
        }];
        assert!(estimate_tool_schema_tokens(&tools) > 30);
        assert_eq!(estimate_tool_schema_tokens(&[]), 0);
    }

    #[test]
    fn an_image_is_not_estimated_as_free() {
        let image =
            qunica_domain::runtime::ChatContentPart::image("image/png", "A".repeat(1_200_000));
        let message = ChatMessage::with_parts("user", vec![image]);
        let tokens = estimate_message_tokens(&message);
        assert!(tokens > 1_000, "{tokens}");
    }

    #[test]
    fn text_carried_as_a_part_is_not_counted_twice() {
        let text = "x".repeat(4_000);
        let plain = ChatMessage::text("user", text.clone());
        let as_part = ChatMessage::with_parts(
            "user",
            vec![qunica_domain::runtime::ChatContentPart::text(text.clone())],
        );
        // `with_parts` derives `content` from the text parts, so the two
        // messages carry the same text and must cost the same.
        assert_eq!(
            estimate_message_tokens(&plain),
            estimate_message_tokens(&as_part)
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
        assert_eq!(
            messages[1].content, "question 0",
            "the opening task must survive"
        );
        assert!(messages[2]
            .content
            .contains("The agent answered twenty questions."));
        assert!(messages[2].content.contains("qunica-context-summary"));
        assert!(outcome.made_progress());
    }

    #[tokio::test]
    async fn the_opening_task_is_never_summarized_away() {
        let mut messages = vec![
            ChatMessage::text("system", "You are a test agent."),
            ChatMessage::text("user", "Refactor the parser. Do not add dependencies."),
        ];
        messages.extend(thread(20).into_iter().skip(1));
        let limits = CompactionLimits { usable_tokens: 100 };

        // Twice, because the second pass is the one that used to eat it: the
        // span always started at index 1, which is exactly where it lives.
        for pass in ["first", "second"] {
            compact(
                &mut messages,
                limits,
                CompactionTrigger::Pressure,
                &FixedSummarizer("the agent did some work"),
            )
            .await
            .unwrap_or_else(|| panic!("the {pass} pass should compact"));
            messages.extend(thread(20).into_iter().skip(1));
        }

        assert_eq!(
            messages[1].content, "Refactor the parser. Do not add dependencies.",
            "a constraint the user stated once is not recoverable by re-running anything"
        );
    }

    #[tokio::test]
    async fn an_enormous_opening_message_is_not_pinned() {
        // Pinning it would give the overflow path a floor it cannot get under.
        let mut messages = vec![
            ChatMessage::text("system", "system"),
            ChatMessage::text("user", "x".repeat(MAX_PINNED_TASK_CHARS + 1)),
        ];
        messages.extend(thread(20).into_iter().skip(1));
        assert_eq!(preserved_prefix(&messages), 1);
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
        assert!(messages[2].content.contains("could not be summarized"));
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

    /// A thread whose tail edge contains one tool turn: `assistant(calls) ->
    /// tool -> assistant(text)`, followed by a trailing user turn so the turn
    /// has a real boundary on both sides.
    ///
    /// Returns the messages plus the indices of the call and the concluding
    /// answer, captured before the trailing pairs are appended.
    fn turn_on_the_tail_edge() -> (Vec<ChatMessage>, usize, usize) {
        let mut messages = thread(4);
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            args: json!({ "command": "cargo build" }),
            provider_metadata: None,
        };
        messages.push(ChatMessage::assistant_tool_calls("", vec![call.clone()]));
        messages.push(ChatMessage::tool_result(
            call.id,
            call.name,
            "build output".to_string(),
        ));
        messages.push(ChatMessage::text("assistant", "the build failed due to X"));
        let calls_index = messages.len() - 3;
        let conclusion_index = messages.len() - 1;
        messages.extend(thread(2).into_iter().skip(1));
        (messages, calls_index, conclusion_index)
    }

    #[test]
    fn a_span_never_leaves_an_orphan_tool_result_at_the_head_of_the_tail() {
        // The tail boundary lands mid tool exchange; the span must extend past
        // the result so the retained tail starts with a complete turn.
        let mut messages = thread(2);
        messages.extend(tool_exchange("build output"));
        messages.extend(thread(3).into_iter().skip(1));
        let raw = messages
            .iter()
            .position(|message| message.role == "tool")
            .unwrap();

        let span = select_span(
            &messages,
            raw,
            CompactionLimits {
                usable_tokens: 10_000,
            },
        )
        .expect("a span should be selectable");
        assert_eq!(span.start, preserved_prefix(&messages));
        assert!(
            is_turn_head(&messages, span.end),
            "the retained tail must begin with a complete turn, not a tool result or a dangling answer"
        );
    }

    #[test]
    fn a_straddling_tool_result_pulls_the_whole_turn_into_the_tail() {
        let (messages, calls_index, conclusion_index) = turn_on_the_tail_edge();
        // Land the raw boundary on the tool result, one message after the call.
        let raw = calls_index + 1;
        let limits = CompactionLimits {
            usable_tokens: 10_000,
        };

        let span = select_span(&messages, raw, limits).expect("a span should be selectable");

        assert_eq!(span.end, calls_index);
        assert!(
            span.end < conclusion_index,
            "the answer must stay with the call that produced it"
        );
    }

    #[test]
    fn a_boundary_on_a_concluding_answer_is_not_split() {
        let (messages, calls_index, _) = turn_on_the_tail_edge();
        // Land the raw boundary exactly on the concluding answer — the position
        // the old forward-scan silently left split.
        let raw = calls_index + 2;
        let limits = CompactionLimits {
            usable_tokens: 10_000,
        };

        let span = select_span(&messages, raw, limits).expect("a span should be selectable");

        assert_eq!(span.end, calls_index);
    }

    #[test]
    fn a_turn_too_large_for_the_tail_is_summarized_instead() {
        let mut messages = thread(4);
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            args: json!({ "command": "cargo build" }),
            provider_metadata: None,
        };
        messages.push(ChatMessage::assistant_tool_calls("", vec![call.clone()]));
        messages.push(ChatMessage::tool_result(
            call.id,
            call.name,
            "x".repeat(30_000),
        ));
        messages.push(ChatMessage::text("assistant", "done"));
        let huge_index = messages.len() - 2;
        messages.extend(thread(3).into_iter().skip(1));

        let raw = huge_index;
        let limits = CompactionLimits {
            usable_tokens: 10_000,
        };
        let span = select_span(&messages, raw, limits).expect("a span should be selectable");

        // The straddling turn is too big to keep whole in the tail, so the
        // boundary advances past it and the tail it leaves is affordable.
        assert!(span.end > huge_index);
        assert!(estimate_tokens(&messages[span.end..]) <= limits.target());
    }

    #[test]
    fn an_emergency_tail_never_begins_with_a_tool_result() {
        let mut messages = thread(4);
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            args: json!({ "command": "cargo build" }),
            provider_metadata: None,
        };
        messages.push(ChatMessage::assistant_tool_calls("", vec![call.clone()]));
        messages.push(ChatMessage::tool_result(
            call.id,
            call.name,
            "output".to_string(),
        ));
        messages.push(ChatMessage::text("assistant", "done"));

        let tail_start = retained_tail_start(&messages, CompactionTrigger::Emergency);
        assert!(
            is_turn_head(&messages, tail_start),
            "even the skeleton tail begins on a turn start, never a tool result"
        );
        assert_ne!(messages[tail_start].role, "tool");
    }

    #[test]
    fn a_clean_boundary_is_left_untouched() {
        let messages = thread(6);
        let raw = messages.len() - 4; // an already-clean turn head
        let span = select_span(
            &messages,
            raw,
            CompactionLimits {
                usable_tokens: 10_000,
            },
        )
        .expect("a span should be selectable");
        assert_eq!(
            span.end, raw,
            "a boundary already on a turn start must not move"
        );
    }

    #[test]
    fn a_thread_shorter_than_the_retained_tail_has_no_span() {
        let limits = CompactionLimits {
            usable_tokens: 10_000,
        };
        let messages = thread(1); // one user turn + one assistant turn
        let tail_start = retained_tail_start(&messages, CompactionTrigger::Pressure);
        assert!(select_span(&messages, tail_start, limits).is_none());

        let empty: Vec<ChatMessage> = Vec::new();
        assert_eq!(retained_tail_start(&empty, CompactionTrigger::Pressure), 1);
    }

    #[test]
    fn pruning_keeps_both_ends_of_a_tool_result() {
        let text = format!("HEAD{}TAIL", "m".repeat(20_000));
        let pruned = prune_text_budget(&text, MAX_RETAINED_TOOL_RESULT_CHARS).unwrap();
        assert!(pruned.starts_with("HEAD"));
        assert!(pruned.ends_with("TAIL"));
        assert!(pruned.chars().count() < text.chars().count());
        assert!(prune_text_budget("short", MAX_RETAINED_TOOL_RESULT_CHARS).is_none());
    }

    #[test]
    fn snip_drops_silent_non_tool_messages_and_keeps_empty_tool_results() {
        let mut messages = vec![
            ChatMessage::text("system", "You are a test agent."),
            ChatMessage::text("user", "   "),
            ChatMessage::text("assistant", ""),
            ChatMessage::tool_result("call-1", "Bash", ""),
        ];

        let outcome = snip_compact(&mut messages);

        assert_eq!(outcome.dropped_messages, 2, "the blank user and assistant");
        let shape: Vec<(String, String)> = messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("system".to_string(), "You are a test agent.".to_string()),
                ("tool".to_string(), String::new()),
            ],
            "an empty tool result is a statement that the call ran, not noise"
        );
    }

    #[test]
    fn snip_collapses_a_run_of_identical_log_lines() {
        let mut lines = vec!["building", "packages"];
        lines.extend(std::iter::repeat_n(
            "error: conflicting type in crate `foo`",
            500,
        ));
        lines.push("done");
        let mut messages = vec![ChatMessage::tool_result("call-1", "Bash", lines.join("\n"))];

        let outcome = snip_compact(&mut messages);

        assert_eq!(outcome.dropped_messages, 0);
        assert!(outcome.snipped_chars > 0);
        let content = &messages[0].content;
        assert!(content.contains("error: conflicting type"), "{content}");
        assert!(content.contains("repeated identical lines"), "{content}");
        assert_eq!(
            content.matches("error: conflicting type").count(),
            1,
            "one copy survives: {content}"
        );
    }

    #[test]
    fn snip_leaves_distinct_lines_and_short_runs_alone() {
        let text = "a\nb\nc\nd\nd\n"; // a run of 2 identical "d" is under the threshold
        let mut messages = vec![ChatMessage::tool_result("call-1", "Bash", text.to_string())];
        snip_compact(&mut messages);
        assert_eq!(messages[0].content, text);
    }

    #[test]
    fn the_tail_keeps_fewer_turns_under_emergency_than_pressure() {
        let messages = thread(20);
        let pressure = retained_tail_start(&messages, CompactionTrigger::Pressure);
        let emergency = retained_tail_start(&messages, CompactionTrigger::Emergency);
        let overflow = retained_tail_start(&messages, CompactionTrigger::Overflow);
        assert!(
            pressure < emergency,
            "pressure keeps more turns than emergency"
        );
        assert_eq!(
            emergency, overflow,
            "emergency and overflow share a skeleton"
        );
    }

    #[test]
    fn the_emergency_ceiling_sits_above_the_pressure_ceiling() {
        let limits = CompactionLimits {
            usable_tokens: 10_000,
        };
        assert!(limits.emergency_threshold() > limits.pressure_threshold());
    }

    #[tokio::test]
    async fn pressure_near_overflow_escalates_to_the_skeleton_tail() {
        let mut messages = thread(30);
        let outcome = compact(
            &mut messages,
            CompactionLimits { usable_tokens: 600 },
            CompactionTrigger::Pressure,
            &FixedSummarizer("summary"),
        )
        .await
        .expect("the estimate sits above 92%, so the pass must run");

        assert!(outcome.summarized_messages > 0);
        // system + pinned task + the summary + a two-turn skeleton tail.
        assert_eq!(
            messages.len(),
            5,
            "an emergency pass keeps only a skeleton, but kept {} messages",
            messages.len()
        );
    }

    #[tokio::test]
    async fn a_normal_pressure_pass_keeps_the_full_tail() {
        let mut messages = thread(30);
        // A window where 61 small messages are past 75% but short of 92%, so no
        // escalation: the tail stays at the full size.
        let outcome = compact(
            &mut messages,
            CompactionLimits { usable_tokens: 800 },
            CompactionTrigger::Pressure,
            &FixedSummarizer("summary"),
        )
        .await
        .unwrap();

        // span replaced [2 .. len-8), so 8 turns (8 messages, in a simple
        // thread) survive.
        assert_eq!(messages.len(), 2 + 1 + RETAINED_TAIL_TURNS);
        assert!(outcome.summarized_messages > 0);
    }

    #[test]
    fn the_tail_is_pruned_only_above_the_ceiling() {
        // A ceiling far above the tail leaves it alone.
        let mut small = vec![ChatMessage::text("system", "s")];
        small.extend(tool_exchange(&"x".repeat(30_000)));
        let tail_start = retained_tail_start(&small, CompactionTrigger::Pressure);
        let (count, removed) = prune_tool_results(
            &mut small,
            tail_start,
            CompactionLimits {
                usable_tokens: 1_000_000,
            },
        );
        assert_eq!((count, removed), (0, 0));

        // A ceiling below the oversized tail prunes the tool result inside it.
        let mut big = vec![ChatMessage::text("system", "s")];
        big.extend(tool_exchange(&"x".repeat(30_000)));
        let tail_start = retained_tail_start(&big, CompactionTrigger::Pressure);
        let (count, removed) = prune_tool_results(
            &mut big,
            tail_start,
            CompactionLimits {
                usable_tokens: 10_000,
            },
        );
        assert!(count > 0);
        assert!(removed > 0);
    }

    #[test]
    fn a_tool_exchange_counts_as_one_turn() {
        let mut messages = vec![ChatMessage::text("system", "s")];
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "Bash".to_string(),
            args: json!({ "command": "cargo build" }),
            provider_metadata: None,
        };
        messages.push(ChatMessage::assistant_tool_calls("", vec![call.clone()]));
        for _ in 0..20 {
            messages.push(ChatMessage::tool_result(
                call.id.clone(),
                call.name.clone(),
                "output".to_string(),
            ));
        }
        messages.push(ChatMessage::text("assistant", "done"));

        // Twenty-two messages, one turn: even the emergency tier (two turns)
        // keeps the whole exchange instead of cutting it two messages from the
        // end the way a message count would.
        assert_eq!(
            retained_tail_start(&messages, CompactionTrigger::Emergency),
            1,
            "a tool exchange must count as one turn, not twenty-two messages"
        );
    }

    #[test]
    fn a_checklist_in_the_tail_survives_ceiling_pruning() {
        let long: Vec<(String, &str)> = (0..30)
            .map(|index| (format!("step {index} {}", "x".repeat(400)), "pending"))
            .collect();
        let borrowed: Vec<(&str, &str)> = long
            .iter()
            .map(|(content, status)| (content.as_str(), *status))
            .collect();
        let mut messages = vec![ChatMessage::text("system", "s")];
        messages.extend(todo_exchange(&borrowed));

        let tail_start = retained_tail_start(&messages, CompactionTrigger::Pressure);
        // A ceiling low enough that the oversized checklist would be pruned if
        // it were any other tool result.
        let (count, removed) = prune_tool_results(
            &mut messages,
            tail_start,
            CompactionLimits {
                usable_tokens: 1_000,
            },
        );
        assert_eq!(
            (count, removed),
            (0, 0),
            "a checklist is exempt even in the tail"
        );
        assert_eq!(latest_checklist(&messages).unwrap().len(), 30);
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
            summary.find(CHECKLIST_OPEN) > summary.find("</qunica-context-summary>"),
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

        let tail_start = retained_tail_start(&messages, CompactionTrigger::Pressure);
        let (count, removed) = prune_tool_results(
            &mut messages,
            tail_start,
            CompactionLimits {
                usable_tokens: 100_000,
            },
        );
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
