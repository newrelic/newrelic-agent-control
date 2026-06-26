//! A registry decorator that memoizes successful agent type lookups.
use std::collections::HashMap;
use std::sync::RwLock;

use super::{AgentTypeRegistry, AgentTypeRegistryError};
use crate::agent_type::agent_type_id::AgentTypeID;
use crate::agent_type::definition::AgentTypeDefinition;

/// An [AgentTypeRegistry] decorator that memoizes successful lookups from an inner registry.
///
/// Agent type artifacts are immutable for a given [AgentTypeID] (its tag embeds name and version),
/// so a definition is cached for the process lifetime once resolved. Only successful lookups are
/// cached: errors propagate untouched and are retried on the next call, avoiding pinning a transient
/// failure or a not-yet-published agent type.
pub struct CachingRegistry<R: AgentTypeRegistry> {
    inner: R,
    cache: RwLock<HashMap<AgentTypeID, AgentTypeDefinition>>,
}

impl<R: AgentTypeRegistry> CachingRegistry<R> {
    /// Wraps an inner registry, caching its successful lookups.
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl<R: AgentTypeRegistry> AgentTypeRegistry for CachingRegistry<R> {
    fn get(
        &self,
        agent_type_id: &AgentTypeID,
    ) -> Result<AgentTypeDefinition, AgentTypeRegistryError> {
        if let Some(definition) = self
            .cache
            .read()
            .expect("agent type cache lock poisoned")
            .get(agent_type_id)
        {
            return Ok(definition.clone());
        }

        // The inner lookup runs without holding the lock so a slow source (e.g. a remote download)
        // does not block other readers. A concurrent cold miss may fetch twice and store the same
        // value, which is harmless.
        let definition = self.inner.get(agent_type_id)?;

        self.cache
            .write()
            .expect("agent type cache lock poisoned")
            .insert(agent_type_id.clone(), definition.clone());

        Ok(definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_type::registry::tests::MockAgentTypeRegistry;
    use assert_matches::assert_matches;

    fn agent_type_id(name: &str) -> AgentTypeID {
        AgentTypeID::try_from(format!("ns/{name}:0.0.0").as_str()).unwrap()
    }

    #[test]
    fn test_repeated_lookups_hit_the_cache() {
        let id = agent_type_id("agent");
        let definition = AgentTypeDefinition::empty_with_metadata(id.clone());

        let mut inner = MockAgentTypeRegistry::new();
        // `should_get` expects exactly one inner call: the second `get` must be served from cache.
        inner.should_get(id.clone(), &definition);

        let registry = CachingRegistry::new(inner);

        assert_eq!(registry.get(&id).unwrap(), definition);
        assert_eq!(registry.get(&id).unwrap(), definition);
    }

    #[test]
    fn test_distinct_ids_each_delegate_once() {
        let first_id = agent_type_id("first");
        let second_id = agent_type_id("second");
        let first = AgentTypeDefinition::empty_with_metadata(first_id.clone());
        let second = AgentTypeDefinition::empty_with_metadata(second_id.clone());

        let mut inner = MockAgentTypeRegistry::new();
        inner.should_get(first_id.clone(), &first);
        inner.should_get(second_id.clone(), &second);

        let registry = CachingRegistry::new(inner);

        assert_eq!(registry.get(&first_id).unwrap(), first);
        assert_eq!(registry.get(&second_id).unwrap(), second);
        // Repeated lookups for both are served from cache (no further inner calls expected).
        assert_eq!(registry.get(&first_id).unwrap(), first);
        assert_eq!(registry.get(&second_id).unwrap(), second);
    }

    #[test]
    fn test_errors_are_not_cached() {
        let id = agent_type_id("missing");

        let mut inner = MockAgentTypeRegistry::new();
        // Two separate failing lookups are expected: an error must fall through to the inner
        // registry on every call rather than being memoized.
        inner.expect_get_not_found(id.clone());
        inner.expect_get_not_found(id.clone());

        let registry = CachingRegistry::new(inner);

        assert_matches!(registry.get(&id), Err(AgentTypeRegistryError::NotFound(_)));
        assert_matches!(registry.get(&id), Err(AgentTypeRegistryError::NotFound(_)));
    }
}
