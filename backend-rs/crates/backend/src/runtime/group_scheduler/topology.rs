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
