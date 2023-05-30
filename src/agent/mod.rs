use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter, Result as FmtResult};
use std::marker::PhantomData;
use std::result::Result;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

use log::{info, trace};
use serde_json::Value;

use crate::agent::config::Config as AgentConfig;
use crate::config::config::Error as ConfigError;
use crate::config::config::Getter;
use crate::context::ctx::Ctx;
use crate::stream::OutputEvent;
use crate::supervisor::factory::SupervisorFactory;
use crate::supervisor::supervisor::{Result as SupervisorResult, SupervisorHandle};

pub(crate) mod config;

#[derive(Debug)]
pub struct AgentError;

impl Display for AgentError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "invalid first item to double")
    }
}

impl From<ConfigError> for AgentError {
    fn from(_: ConfigError) -> AgentError {
        AgentError
    }
}

/// The Agent Struct that injects a config getter that implements
/// the config::Getter trait and uses the V value serializer
pub struct Agent<G: Getter<AgentConfig<Value>> + Sync, V: Debug + Sync> {
    conf_getter: G,
    supervisors: Arc<Mutex<HashMap<String, Box<dyn SupervisorHandle + Sync>>>>,
    _marker: PhantomData<(AgentConfig<Value>, V)>,
}

impl<G: Getter<AgentConfig<Value>> + Sync, V: Debug + Sync> Agent<G, V> {
    pub fn new(getter: G) -> Self {
        Self {
            conf_getter: getter,
            _marker: PhantomData,
            supervisors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// The start function calls the config getter to print the configuration.
    pub fn start(&self) -> Result<(), AgentError> {
        // application configuration
        let conf = self.conf()?;

        // TODO <std_management>
        let (tx, rx) = std::sync::mpsc::channel::<OutputEvent>();
        thread::spawn(move || {
            Self::start_std_receivers(rx);
        });
        // TODO </std_management>


        // build and run supervisors
        for (agent_name, agent_conf) in conf.agents {
            // all supervisors will send to the same channel
            let std_sender = Mutex::new(tx.clone());
            // build supervisor
            match SupervisorFactory::from_config(agent_name.clone(), std_sender) {
                Err(e) => {
                    eprintln!("cannot build supervisor {}", e);
                }
                Ok(mut spv) => {
                    // start supervisor
                    if let Err(e) = spv.start() {
                        eprintln!("cannot start supervisor {}", e);
                        continue;
                    }
                    // store supervisor in local state var
                    self.supervisors.lock().unwrap().insert(agent_name.clone(), spv);
                }
            }
        }

        Ok(())
    }

    #[allow(dead_code)] // TODO This is not used for now
    pub fn stop(&self) {
        self.supervisors.clone().lock().unwrap().iter_mut().for_each(|(name, spv)| {
            trace!("stopping agent {}",name);
            match spv.stop() {
                Err(e) => eprintln!("error stopping supervisor {} : {}", name, e),
                Ok(()) => info!("successfully stopped {}",name)
            }
        })
    }

    // TODO : Temporal as POC of reading logs
    fn start_std_receivers(rec: Receiver<OutputEvent>) {
        for line in rec {
            println!("{:?}", line)
        }
    }

    pub fn conf(&self) -> Result<AgentConfig<Value>, AgentError> {
        match self.conf_getter.get() {
            Err(_) => Err(AgentError),
            Ok(x) => Ok(x)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Config;

    use super::*;

    struct TestGetter {
        agents: HashMap<String, Value>,
    }

    impl TestGetter {
        fn new(agents: HashMap<String, Value>) -> Self {
            Self { agents }
        }
    }

    impl Getter<Config<Value>> for TestGetter where {
        fn get(&self) -> crate::config::config::Result<Config<Value>> {
            let cnf = Config { agents: self.agents.clone() };
            Ok(cnf)
        }
    }

    #[test]
    fn one_test() {
        let getter: TestGetter = TestGetter::new(HashMap::new());
        let ag: Agent<TestGetter, Value> = Agent::new(getter);
        ag.start();
    }
}