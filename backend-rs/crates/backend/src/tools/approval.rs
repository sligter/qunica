//! What a tool call may need a human to authorise.
//!
//! The vocabulary here is shared by three layers that must agree: the tool that
//! decides a call needs a decision, the runtime that pauses the turn and later
//! replays the call, and the API that carries the user's answer back. Keeping it
//! in one place is what lets a grant made in the UI be checked by the shell
//! policy without either knowing about the other.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Rule ids the user has approved for the current thread.
///
/// Grants are keyed on a policy rule (`delete-files`), not on a command string.
/// A user who approves deleting a build directory has authorised the capability,
/// and re-asking for the next `rm` in the same thread would be nagging rather
/// than consent. The grant does not outlive the thread.
///
/// [`ApprovalGrants::bypass_all`] is the separate, blunter thing: an agent the
/// owner has explicitly put in unattended mode, where nothing is asked and
/// nothing is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovalGrants {
    rules: HashSet<String>,
    bypass_all: bool,
}

impl ApprovalGrants {
    pub fn new(rules: impl IntoIterator<Item = String>) -> Self {
        Self {
            rules: rules.into_iter().collect(),
            bypass_all: false,
        }
    }

    pub fn grant(&mut self, rule: impl Into<String>) {
        self.rules.insert(rule.into());
    }

    /// Turn off every guard for this agent: nothing pauses for approval, and the
    /// rules that normally refuse outright stop refusing.
    ///
    /// Only an agent whose owner switched it into unattended mode gets this. It
    /// is deliberately one flag rather than a per-rule list — a partial bypass
    /// reads as safer than it is, and the thing being expressed here is "I am
    /// not going to be at the keyboard", not "these five verbs are fine".
    pub fn set_bypass_all(&mut self, bypass_all: bool) {
        self.bypass_all = bypass_all;
    }

    /// Whether this agent runs with every approval gate off.
    pub fn bypass_all(&self) -> bool {
        self.bypass_all
    }

    pub fn contains(&self, rule: &str) -> bool {
        self.bypass_all || self.rules.contains(rule)
    }

    /// True when nothing at all has been authorised — no remembered rule and no
    /// blanket bypass.
    pub fn is_empty(&self) -> bool {
        !self.bypass_all && self.rules.is_empty()
    }
}

/// The structured question a paused tool call is asking its user.
///
/// Travels three hops without being reinterpreted: the tool emits it inside its
/// result, the runtime lifts it onto the `approval_required` stream event and
/// into the interrupted message, and the client renders it as the approval card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Policy rule id a "remember this" grant is keyed on.
    pub rule: String,
    /// Short name of the capability, for the card's headline.
    pub capability: String,
    /// Why this call needs a decision.
    pub reason: String,
    /// The tool the model called.
    pub tool_name: String,
    /// Exactly what is being authorised — for the shell, the command line.
    pub subject: String,
}

impl ApprovalRequest {
    /// One line for a model or a log; the client renders the fields instead.
    pub fn summary(&self) -> String {
        format!(
            "Approval required to {}: {} ({})",
            self.capability, self.subject, self.reason
        )
    }
}

/// How the user answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// The paused call this answers. Checked against the interrupted message so
    /// a stale card cannot authorise a different command.
    pub tool_call_id: String,
    pub approved: bool,
    /// Approve every later call covered by the same rule in this thread.
    #[serde(default)]
    pub remember: bool,
    /// Optional note from the user, passed to the model with the outcome — the
    /// place to say *why* something was declined, which is usually the part the
    /// agent needs in order to do something else instead.
    #[serde(default)]
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_covers_its_own_rule_and_nothing_else() {
        let mut grants = ApprovalGrants::default();
        assert!(grants.is_empty());
        grants.grant("delete-files");
        assert!(grants.contains("delete-files"));
        assert!(!grants.contains("git-force-push"));
    }

    #[test]
    fn a_bypass_covers_every_rule_including_ones_never_granted() {
        let mut grants = ApprovalGrants::default();
        grants.set_bypass_all(true);
        assert!(grants.bypass_all());
        assert!(grants.contains("delete-files"));
        assert!(grants.contains("anything-at-all"));
        assert!(
            !grants.is_empty(),
            "a bypass has authorised everything, which is the opposite of empty"
        );
    }

    #[test]
    fn a_decision_defaults_to_a_one_time_answer() {
        let decision: ApprovalDecision =
            serde_json::from_str(r#"{"tool_call_id":"c1","approved":true}"#).unwrap();
        assert!(decision.approved);
        assert!(!decision.remember, "remembering must be opt-in");
        assert_eq!(decision.note, None);
    }

    #[test]
    fn a_request_round_trips_through_json() {
        let request = ApprovalRequest {
            rule: "delete-files".to_string(),
            capability: "delete files in this workspace".to_string(),
            reason: "it deletes files".to_string(),
            tool_name: "Pwsh".to_string(),
            subject: "rm -rf build".to_string(),
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<ApprovalRequest>(&encoded).unwrap(),
            request
        );
        assert!(request.summary().contains("rm -rf build"));
    }
}
