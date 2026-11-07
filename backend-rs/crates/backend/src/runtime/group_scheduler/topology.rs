use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TopologySnapshot {
    Mesh {
        agents: Vec<String>,
    },
    Star {
        hub: String,
        spokes: Vec<String>,
    },
    Hierarchical {
        leaders: Vec<String>,
        workers: Vec<String>,
    },
    Ring {
        ordered: Vec<String>,
    },
}

/// Return the ordered candidates reachable for the next scheduled speaker.
///
/// Note: candidate loading, membership revalidation and budgets remain with the
/// coordinator. This pure policy preserves their order except for ring traversal;
/// topology validation and degradation are separate steps.
pub(in crate::runtime) fn topology_frontier(
    topology: &TopologySnapshot,
    candidate_ids: &[String],
    previous_speaker: Option<&str>,
) -> Vec<String> {
    if let Some(source) = previous_speaker {
        return match topology {
            TopologySnapshot::Mesh { .. } => candidate_ids.to_vec(),
            TopologySnapshot::Ring { ordered } => {
                let Some(source_index) = ordered.iter().position(|agent_id| agent_id == source)
                else {
                    return Vec::new();
                };
                ordered
                    .iter()
                    .cycle()
                    .skip(source_index + 1)
                    .take(ordered.len().saturating_sub(1))
                    .find(|agent_id| candidate_ids.contains(*agent_id))
                    .cloned()
                    .into_iter()
                    .collect()
            }
            _ => candidate_ids
                .iter()
                .filter(|target| allows_agent_edge(topology, source, target))
                .cloned()
                .collect(),
        };
    }

    match topology {
        TopologySnapshot::Mesh { .. } => candidate_ids.to_vec(),
        TopologySnapshot::Star { hub, .. } if candidate_ids.contains(hub) => vec![hub.clone()],
        TopologySnapshot::Hierarchical { leaders, .. } => {
            let available = candidate_ids
                .iter()
                .filter(|agent_id| leaders.contains(agent_id))
                .cloned()
                .collect::<Vec<_>>();
            if available.is_empty() {
                candidate_ids.to_vec()
            } else {
                available
            }
        }
        TopologySnapshot::Ring { ordered } => ordered
            .iter()
            .find(|agent_id| candidate_ids.contains(agent_id))
            .cloned()
            .into_iter()
            .collect(),
        TopologySnapshot::Star { .. } => candidate_ids.to_vec(),
    }
}

pub fn validate_topology(snapshot: &TopologySnapshot) -> Result<(), TopologyError> {
    let agents = match snapshot {
        TopologySnapshot::Mesh { agents } => agents,
        TopologySnapshot::Star { hub, spokes } => {
            if spokes.contains(hub) {
                return Err(TopologyError::DuplicateAgent);
            }
            if hub.is_empty() {
                return Err(TopologyError::MissingRequiredRole);
            }
            return ensure_unique(spokes);
        }
        TopologySnapshot::Hierarchical { leaders, workers } => {
            if leaders.is_empty() {
                return Err(TopologyError::MissingRequiredRole);
            }
            if leaders.iter().chain(workers).any(|id| id.is_empty()) {
                return Err(TopologyError::InvalidAgent);
            }
            let mut all = leaders.clone();
            all.extend(workers.clone());
            return ensure_unique(&all);
        }
        TopologySnapshot::Ring { ordered } => {
            if ordered.len() < 2 {
                return Err(TopologyError::InvalidRing);
            }
            return ensure_unique(ordered);
        }
    };
    if agents.iter().any(|id| id.is_empty()) {
        return Err(TopologyError::InvalidAgent);
    }
    ensure_unique(agents)
}

pub fn allows_agent_edge(snapshot: &TopologySnapshot, source: &str, target: &str) -> bool {
    if source == target {
        return false;
    }
    match snapshot {
        TopologySnapshot::Mesh { agents } => {
            agents.contains(&source.to_owned()) && agents.contains(&target.to_owned())
        }
        TopologySnapshot::Star { hub, spokes } => {
            (source == hub && spokes.contains(&target.to_owned()))
                || (target == hub && spokes.contains(&source.to_owned()))
        }
        TopologySnapshot::Hierarchical { leaders, workers } => {
            leaders.contains(&source.to_owned())
                && (leaders.contains(&target.to_owned()) || workers.contains(&target.to_owned()))
                || workers.contains(&source.to_owned()) && leaders.contains(&target.to_owned())
        }
        TopologySnapshot::Ring { ordered } => ordered
            .iter()
            .position(|id| id == source)
            .is_some_and(|index| ordered[(index + 1) % ordered.len()] == target),
    }
}

fn ensure_unique(agents: &[String]) -> Result<(), TopologyError> {
    if agents.iter().any(|id| id.is_empty()) {
        return Err(TopologyError::InvalidAgent);
    }
    let mut seen = std::collections::HashSet::new();
    if agents.iter().all(|id| seen.insert(id)) {
        Ok(())
    } else {
        Err(TopologyError::DuplicateAgent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TopologyError {
    #[error("topology must contain the required role")]
    MissingRequiredRole,
    #[error("topology contains duplicate agents")]
    DuplicateAgent,
    #[error("topology contains an invalid agent id")]
    InvalidAgent,
    #[error("ring topology needs at least two agents")]
    InvalidRing,
}

#[cfg(test)]
mod tests {
    use super::{allows_agent_edge, validate_topology, TopologySnapshot};
    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }
    #[test]
    fn topologies_allow_only_their_documented_edges() {
        let mesh = TopologySnapshot::Mesh {
            agents: ids(&["a", "b"]),
        };
        let star = TopologySnapshot::Star {
            hub: "a".into(),
            spokes: ids(&["b", "c"]),
        };
        let hierarchy = TopologySnapshot::Hierarchical {
            leaders: ids(&["a"]),
            workers: ids(&["b", "c"]),
        };
        let ring = TopologySnapshot::Ring {
            ordered: ids(&["a", "b", "c"]),
        };
        for snapshot in [&mesh, &star, &hierarchy, &ring] {
            assert!(validate_topology(snapshot).is_ok());
        }
        assert!(allows_agent_edge(&mesh, "a", "b"));
        assert!(allows_agent_edge(&star, "b", "a"));
        assert!(!allows_agent_edge(&star, "b", "c"));
        assert!(allows_agent_edge(&hierarchy, "a", "b"));
        assert!(!allows_agent_edge(&hierarchy, "b", "c"));
        assert!(allows_agent_edge(&ring, "c", "a"));
        assert!(!allows_agent_edge(&ring, "a", "c"));
    }
}

#[cfg(test)]
mod topology_frontier_tests {
    use super::{topology_frontier, TopologySnapshot};

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn mesh_frontier_preserves_candidate_order_and_membership() {
        let topology = TopologySnapshot::Mesh {
            agents: ids(&["a", "b", "c"]),
        };
        for previous in [None, Some("b"), Some("unknown")] {
            // Membership and duplicate filtering belong to the caller.
            let candidates = ids(&["c", "b", "a", "outside", "b"]);
            assert_eq!(
                topology_frontier(&topology, &candidates, previous),
                candidates
            );
        }
    }

    #[test]
    fn star_frontier_starts_at_hub_then_preserves_legal_candidate_order() {
        let topology = TopologySnapshot::Star {
            hub: "h".into(),
            spokes: ids(&["a", "b", "c"]),
        };
        for (previous, candidates, expected) in [
            (None, ids(&["c", "h", "a"]), ids(&["h"])),
            (
                None,
                ids(&["c", "a", "outside"]),
                ids(&["c", "a", "outside"]),
            ),
            (
                Some("h"),
                ids(&["c", "h", "a", "outside", "a"]),
                ids(&["c", "a", "a"]),
            ),
            (Some("a"), ids(&["b", "h", "a"]), ids(&["h"])),
            (Some("a"), ids(&["b", "c"]), ids(&[])),
            (Some("unknown"), ids(&["h", "a"]), ids(&[])),
        ] {
            assert_eq!(
                topology_frontier(&topology, &candidates, previous),
                expected,
                "previous={previous:?}, candidates={candidates:?}"
            );
        }
    }

    #[test]
    fn hierarchical_frontier_preserves_candidate_order_and_role_edges() {
        let topology = TopologySnapshot::Hierarchical {
            leaders: ids(&["l1", "l2"]),
            workers: ids(&["w1", "w2"]),
        };
        for (previous, candidates, expected) in [
            (None, ids(&["w1", "l2", "l1", "w2"]), ids(&["l2", "l1"])),
            (
                None,
                ids(&["w2", "outside", "w1"]),
                ids(&["w2", "outside", "w1"]),
            ),
            (
                Some("l1"),
                ids(&["w2", "l2", "l1", "w1", "outside"]),
                ids(&["w2", "l2", "w1"]),
            ),
            (
                Some("w1"),
                ids(&["w2", "l2", "l1", "w1"]),
                ids(&["l2", "l1"]),
            ),
            (Some("w1"), ids(&["w2"]), ids(&[])),
            (Some("unknown"), ids(&["l1", "w1"]), ids(&[])),
        ] {
            assert_eq!(
                topology_frontier(&topology, &candidates, previous),
                expected,
                "previous={previous:?}, candidates={candidates:?}"
            );
        }
    }

    #[test]
    fn ring_frontier_starts_in_topology_order_and_skips_unavailable_agents() {
        let topology = TopologySnapshot::Ring {
            ordered: ids(&["a", "b", "c", "d"]),
        };
        for (candidates, expected) in [
            (ids(&["d", "b", "a", "c"]), ids(&["a"])),
            (ids(&["d", "c"]), ids(&["c"])),
            (ids(&["outside"]), ids(&[])),
        ] {
            assert_eq!(
                topology_frontier(&topology, &candidates, None),
                expected,
                "candidates={candidates:?}"
            );
        }
    }

    #[test]
    fn ring_frontier_advances_wraps_and_never_reselects_its_source() {
        let topology = TopologySnapshot::Ring {
            ordered: ids(&["a", "b", "c", "d"]),
        };
        for (previous, candidates, expected) in [
            ("a", ids(&["d", "c", "b", "a"]), ids(&["b"])),
            ("a", ids(&["d", "c"]), ids(&["c"])),
            ("d", ids(&["c", "b", "a", "d"]), ids(&["a"])),
            ("c", ids(&["b", "a"]), ids(&["a"])),
            ("b", ids(&["b", "outside"]), ids(&[])),
            ("unknown", ids(&["a", "b"]), ids(&[])),
        ] {
            assert_eq!(
                topology_frontier(&topology, &candidates, Some(previous)),
                expected,
                "previous={previous:?}, candidates={candidates:?}"
            );
        }
    }

    #[test]
    fn empty_candidates_produce_an_empty_frontier_for_every_topology() {
        for topology in [
            TopologySnapshot::Mesh {
                agents: ids(&["a", "b"]),
            },
            TopologySnapshot::Star {
                hub: "a".into(),
                spokes: ids(&["b"]),
            },
            TopologySnapshot::Hierarchical {
                leaders: ids(&["a"]),
                workers: ids(&["b"]),
            },
            TopologySnapshot::Ring {
                ordered: ids(&["a", "b"]),
            },
        ] {
            for previous in [None, Some("a"), Some("unknown")] {
                assert!(
                    topology_frontier(&topology, &[], previous).is_empty(),
                    "topology={topology:?}, previous={previous:?}"
                );
            }
        }
    }

    #[test]
    fn frontier_does_not_validate_or_repair_invalid_topology_snapshots() {
        // Validation is a separate responsibility. Preserve the old helper's
        // behavior even for snapshots that validate_topology would reject.
        for (topology, previous, expected) in [
            (TopologySnapshot::Ring { ordered: ids(&[]) }, None, ids(&[])),
            (
                TopologySnapshot::Ring { ordered: ids(&[]) },
                Some("a"),
                ids(&[]),
            ),
            (
                TopologySnapshot::Ring {
                    ordered: ids(&["a"]),
                },
                None,
                ids(&["a"]),
            ),
            (
                TopologySnapshot::Ring {
                    ordered: ids(&["a"]),
                },
                Some("a"),
                ids(&[]),
            ),
            (
                TopologySnapshot::Ring {
                    ordered: ids(&["a", "a", "b"]),
                },
                Some("a"),
                ids(&["a"]),
            ),
            (
                TopologySnapshot::Hierarchical {
                    leaders: ids(&[]),
                    workers: ids(&["a", "b"]),
                },
                None,
                ids(&["b", "a"]),
            ),
            (
                TopologySnapshot::Star {
                    hub: String::new(),
                    spokes: ids(&["a", "b"]),
                },
                None,
                ids(&["b", "a"]),
            ),
        ] {
            assert_eq!(
                topology_frontier(&topology, &ids(&["b", "a"]), previous),
                expected,
                "topology={topology:?}, previous={previous:?}"
            );
        }
    }

    #[test]
    fn automatic_mesh_keeps_the_previous_speaker_in_the_moderator_frontier() {
        let topology = TopologySnapshot::Mesh {
            agents: vec!["a".into(), "b".into(), "c".into()],
        };

        assert_eq!(
            topology_frontier(&topology, &["a".into(), "b".into(), "c".into()], Some("b")),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
        assert_eq!(
            topology_frontier(&topology, &["b".into()], Some("b")),
            vec!["b".to_owned()]
        );
    }
}
