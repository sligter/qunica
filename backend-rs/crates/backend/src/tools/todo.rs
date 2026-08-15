//! The checklist an agent keeps for the work in front of it.
//!
//! The vocabulary here is shared by three layers that must agree: the
//! `TodoWrite` tool that normalizes whatever the model sent, the runtime that
//! mirrors the latest list onto the turn record, and the client that renders it.
//! Keeping the shape in one place is what lets a checklist survive a reload
//! instead of living only as text inside one tool result.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most items one call may record.
///
/// A list longer than this has stopped being a checklist and become a plan,
/// and it costs more context on every later turn than the tracking is worth.
pub const MAX_TODOS: usize = 20;

/// Longest a single item may be. Items are one-line labels; a model writing a
/// paragraph is describing the work rather than tracking it.
const MAX_TODO_CHARS: usize = 200;

/// Where one item stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    /// The wire name, which is also the value the tool's schema asks the model
    /// to send back. Anything restating a checklist as prose uses these, so a
    /// model reading it can return the list with one label changed.
    pub fn as_str(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
        }
    }

    /// Read a status from whatever the model actually wrote.
    ///
    /// Models disagree on the spelling (`in_progress`, `in-progress`, `done`),
    /// and a status nobody recognises should leave the item showing as
    /// unfinished rather than reject the call: an item wrongly shown as pending
    /// is a display bug, one silently dropped is lost work.
    pub fn parse(raw: &str) -> Self {
        match raw
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_")
            .as_str()
        {
            "in_progress" | "active" => TodoStatus::InProgress,
            "completed" | "complete" | "done" => TodoStatus::Completed,
            _ => TodoStatus::Pending,
        }
    }
}

/// One checklist line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    #[serde(default)]
    pub status: TodoStatus,
}

/// Read the checklist out of a `TodoWrite` call's arguments.
///
/// Both shapes models produce are accepted: the `{content, status}` objects the
/// schema advertises, and the bare list of strings a lenient provider emits.
/// Reading only strings — as this once did — turned a full checklist into an
/// empty one while still reporting `COMPLETED`, which is the worst failure
/// available to a tool whose only job is to remember something.
pub fn parse_todos(args: &Value) -> Vec<TodoItem> {
    let items: Vec<Value> = match args.get("todos") {
        Some(Value::Array(items)) => items.clone(),
        // A single item sent unwrapped is still an item.
        Some(single @ (Value::String(_) | Value::Object(_))) => vec![single.clone()],
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(parse_item)
        .take(MAX_TODOS)
        .collect()
}

fn parse_item(value: &Value) -> Option<TodoItem> {
    let (content, status) = match value {
        Value::String(text) => (text.as_str(), TodoStatus::Pending),
        Value::Object(_) => {
            // `content` is what the schema asks for; the aliases cover models
            // that name the field after the thing rather than the slot.
            let content = ["content", "task", "text", "title"]
                .iter()
                .find_map(|key| value.get(*key).and_then(Value::as_str))?;
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .map_or(TodoStatus::Pending, TodoStatus::parse);
            (content, status)
        }
        _ => return None,
    };
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    Some(TodoItem {
        content: content.chars().take(MAX_TODO_CHARS).collect(),
        status,
    })
}

/// Lift the normalized checklist back out of a `TodoWrite` result.
///
/// The runtime needs the list as data, not as the text it hands the model, so
/// it can mirror it onto the turn record and the `todo_update` event, and so
/// compaction can recognise a checklist in a transcript it is about to drop.
/// Both forms a result takes are read: the raw JSON the tool returned, and the
/// `status: <Status>\n<json>` framing the runtime wraps it in on the way to the
/// model. Anything that is not a `TodoWrite` result reports `None` rather than
/// guessing.
pub fn todos_from_output(output: &str) -> Option<Vec<TodoItem>> {
    let start = output.find('{')?;
    let value: Value = serde_json::from_str(&output[start..]).ok()?;
    if value.get("tool").and_then(Value::as_str) != Some("TodoWrite") {
        return None;
    }
    serde_json::from_value(value.get("todos")?.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn object_items_keep_their_status() {
        let todos = parse_todos(&json!({
            "todos": [
                { "content": "read the code", "status": "completed" },
                { "content": "write the fix", "status": "in_progress" },
                { "content": "run the tests", "status": "pending" },
            ]
        }));
        assert_eq!(
            todos,
            vec![
                TodoItem {
                    content: "read the code".to_string(),
                    status: TodoStatus::Completed,
                },
                TodoItem {
                    content: "write the fix".to_string(),
                    status: TodoStatus::InProgress,
                },
                TodoItem {
                    content: "run the tests".to_string(),
                    status: TodoStatus::Pending,
                },
            ]
        );
    }

    #[test]
    fn bare_strings_are_still_a_checklist() {
        let todos = parse_todos(&json!({ "todos": ["one", "two"] }));
        assert_eq!(
            todos
                .iter()
                .map(|todo| todo.content.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
        assert!(todos.iter().all(|todo| todo.status == TodoStatus::Pending));
    }

    #[test]
    fn spelling_variants_of_a_status_are_understood() {
        let todos = parse_todos(&json!({
            "todos": [
                { "content": "a", "status": "in-progress" },
                { "content": "b", "status": "DONE" },
                { "content": "c", "status": "somethingelse" },
            ]
        }));
        let statuses: Vec<TodoStatus> = todos.iter().map(|todo| todo.status).collect();
        assert_eq!(
            statuses,
            vec![
                TodoStatus::InProgress,
                TodoStatus::Completed,
                TodoStatus::Pending
            ]
        );
    }

    #[test]
    fn empty_and_unusable_items_are_dropped_without_dropping_the_rest() {
        let todos = parse_todos(&json!({
            "todos": ["  ", { "status": "pending" }, 7, { "content": "kept" }]
        }));
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "kept");
    }

    #[test]
    fn the_list_and_each_item_are_bounded() {
        let long = "x".repeat(MAX_TODO_CHARS + 50);
        let many: Vec<Value> = (0..MAX_TODOS + 5).map(|_| json!(long)).collect();
        let todos = parse_todos(&json!({ "todos": many }));
        assert_eq!(todos.len(), MAX_TODOS);
        assert_eq!(todos[0].content.chars().count(), MAX_TODO_CHARS);
    }

    #[test]
    fn a_missing_or_wrongly_typed_argument_yields_no_items() {
        assert!(parse_todos(&json!({})).is_empty());
        assert!(parse_todos(&json!({ "todos": 3 })).is_empty());
    }

    #[test]
    fn a_result_round_trips_back_into_items() {
        let items = parse_todos(&json!({ "todos": [{ "content": "a", "status": "completed" }] }));
        let output =
            json!({ "tool": "TodoWrite", "status": "COMPLETED", "todos": items }).to_string();
        assert_eq!(
            todos_from_output(&output).unwrap()[0].status,
            TodoStatus::Completed
        );
        // The runtime frames a result before the model reads it, and compaction
        // reads that framed copy out of the transcript.
        let framed = format!("status: Completed\n{output}");
        assert_eq!(todos_from_output(&framed), todos_from_output(&output));
        // Another tool's result is not a checklist, however similar it looks.
        let other = json!({ "tool": "AskUser", "todos": [] }).to_string();
        assert_eq!(todos_from_output(&other), None);
        assert_eq!(todos_from_output("not json"), None);
    }

    #[test]
    fn a_status_label_round_trips_through_its_wire_name() {
        for status in [
            TodoStatus::Pending,
            TodoStatus::InProgress,
            TodoStatus::Completed,
        ] {
            assert_eq!(TodoStatus::parse(status.as_str()), status);
        }
    }
}
