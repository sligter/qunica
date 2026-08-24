//! A ceiling on the size of one tool result, applied where the result is
//! produced.
//!
//! The built-in tools each bound their own output — the shell keeps 12 000
//! characters, `WebFetch` 20 000, `Read` 2 000 lines. MCP tools bound nothing:
//! [`crate::tools::executor`] hands the server's text straight back, and the
//! transports accept 8 MB (HTTP) and 32 MB (SSE) responses. One such result was
//! unrecoverable, because both reductions in [`super::compaction`] deliberately
//! leave the newest messages alone:
//!
//! * pruning skips the retained tail, so the result is never elided;
//! * the summary span stops short of the tail, so it is never summarized away;
//! * and it is persisted, so every later turn in the thread rebuilds it from
//!   the database and fails the same way.
//!
//! Capping here rather than in compaction is what makes that unreachable: the
//! oversized text never enters the transcript, so there is no state a later
//! turn has to dig itself out of.

use async_trait::async_trait;

use crate::llm::ToolCall;
use crate::tools::{todo, ToolResult};

use super::hooks::{RuntimeHook, StepContext};

/// The most characters one tool result may contribute to a thread.
///
/// Well above what any built-in tool produces, so this is a backstop for
/// unbounded sources rather than a second opinion on their limits. Roughly
/// 6 000 tokens of English or 24 000 of CJK — large enough that a legitimate
/// result survives intact, small enough that a handful cannot fill a window.
pub const MAX_TOOL_RESULT_CHARS: usize = 24_000;

/// Characters kept from the start of an oversized result.
const CAP_HEAD_CHARS: usize = 8_000;
/// Characters kept from the end. Weighted towards the tail for the same reason
/// shell output is: errors come last.
const CAP_TAIL_CHARS: usize = 12_000;

/// Cap an oversized tool result, or `None` when it already fits.
///
/// The middle goes rather than the tail, and the marker says how much went and
/// what to do about it. Every source this can fire on is re-runnable — a file
/// can be read again, a command executed again, an MCP tool called again — so
/// naming that is the whole recovery path the model needs.
pub fn cap_tool_output(output: &str) -> Option<String> {
    let total = output.chars().count();
    if total <= MAX_TOOL_RESULT_CHARS {
        return None;
    }
    let head: String = output.chars().take(CAP_HEAD_CHARS).collect();
    let tail: String = output.chars().skip(total - CAP_TAIL_CHARS).collect();
    let elided = total - CAP_HEAD_CHARS - CAP_TAIL_CHARS;
    Some(format!(
        "{head}\n[... {elided} characters elided: this tool returned {total} characters, more \
         than the {MAX_TOOL_RESULT_CHARS} one result may contribute to the conversation. Narrow \
         the request — a path, a filter, a smaller range — and run it again if you need the \
         middle ...]\n{tail}"
    ))
}

/// Whether `output` is exempt from the cap.
///
/// A `TodoWrite` checklist is bounded by construction and is the one result
/// whose middle cannot be recovered by running the tool again: the agent's own
/// plan is not re-derivable from the environment. [`super::compaction`] exempts
/// it from pruning for the same reason, and an exemption that held there but
/// not here would let the checklist be cut before compaction ever saw it.
fn is_exempt(output: &str) -> bool {
    todo::todos_from_output(output).is_some()
}

/// Applies [`cap_tool_output`] to every result before it reaches the model or
/// the transcript.
pub struct ToolResultCapHook;

#[async_trait]
impl RuntimeHook for ToolResultCapHook {
    fn name(&self) -> &'static str {
        "tool-result-cap"
    }

    async fn post_tool(&self, step: &StepContext, call: &ToolCall, result: &mut ToolResult) {
        if is_exempt(&result.output) {
            return;
        }
        let Some(capped) = cap_tool_output(&result.output) else {
            return;
        };
        tracing::warn!(
            agent_id = %step.agent_id,
            tool = %call.name,
            original_chars = result.output.chars().count(),
            capped_chars = capped.chars().count(),
            "tool result exceeded the per-result ceiling and was capped"
        );
        result.output = capped;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolStatus;
    use serde_json::json;

    fn call() -> ToolCall {
        ToolCall {
            id: "call-1".to_string(),
            name: "mcp__docs__search".to_string(),
            args: json!({}),
            provider_metadata: None,
        }
    }

    fn step() -> StepContext {
        crate::runtime::hooks::test_support::step_context()
    }

    #[test]
    fn output_within_the_ceiling_is_untouched() {
        assert_eq!(cap_tool_output(&"x".repeat(MAX_TOOL_RESULT_CHARS)), None);
    }

    #[test]
    fn an_oversized_result_keeps_both_ends_and_shrinks() {
        let output = format!("HEAD{}TAIL", "m".repeat(500_000));
        let capped = cap_tool_output(&output).expect("well over the ceiling");
        assert!(capped.starts_with("HEAD"));
        assert!(capped.ends_with("TAIL"));
        assert!(capped.chars().count() < MAX_TOOL_RESULT_CHARS + 400);
        assert!(capped.contains("characters elided"));
    }

    #[test]
    fn the_ceiling_counts_characters_not_bytes() {
        // A CJK result is three bytes per character; counting bytes would cap it
        // at a third of the text an ASCII result is allowed.
        let output = "文".repeat(MAX_TOOL_RESULT_CHARS);
        assert_eq!(cap_tool_output(&output), None);
    }

    #[tokio::test]
    async fn the_hook_caps_an_unbounded_mcp_result() {
        let mut result = ToolResult {
            status: ToolStatus::Completed,
            output: "y".repeat(2_000_000),
        };
        ToolResultCapHook
            .post_tool(&step(), &call(), &mut result)
            .await;
        assert!(result.output.chars().count() < MAX_TOOL_RESULT_CHARS + 400);
        assert_eq!(result.status, ToolStatus::Completed);
    }

    #[tokio::test]
    async fn an_oversized_checklist_is_still_a_checklist() {
        let items: Vec<_> = (0..400)
            .map(|index| json!({ "content": format!("step {index} {}", "x".repeat(200)), "status": "pending" }))
            .collect();
        let output = format!(
            "status: Completed\n{}",
            json!({ "tool": "TodoWrite", "status": "COMPLETED", "todos": items })
        );
        assert!(output.chars().count() > MAX_TOOL_RESULT_CHARS);

        let mut result = ToolResult {
            status: ToolStatus::Completed,
            output: output.clone(),
        };
        ToolResultCapHook
            .post_tool(&step(), &call(), &mut result)
            .await;
        assert_eq!(
            result.output, output,
            "cutting the middle out of a checklist drops the agent's own plan"
        );
    }
}
