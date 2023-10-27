use std::collections::HashMap;

use crate::config::super_agent_configs::AgentID;

use super::{NotStartedSubAgent, StartedSubAgent};

struct NotStartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: NotStartedSubAgent;

struct StartedSubAgents<S>(HashMap<AgentID, S>)
where
    S: StartedSubAgent;
