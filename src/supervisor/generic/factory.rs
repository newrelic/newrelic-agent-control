use crate::Cmd;
use crate::supervisor::generic::config::AgentConf;
use crate::supervisor::generic::generic_supervisor::GenericSupervisor;
use crate::supervisor::supervisor::Result;

pub struct GenericSupervisorFactory {}

impl GenericSupervisorFactory {
    pub fn from_config(raw_conf: String) -> Result<GenericSupervisor> {
        println!("{:?}", raw_conf);
        // testing some conf unmarshalling
        //TODO control error
        let agent_conf: AgentConf = serde_json::from_str(raw_conf.as_str()).unwrap();
        println!("{:?}", agent_conf);

        let exec = agent_conf.executables.get(0).unwrap().to_owned();
        let cmd = Cmd::new(exec.binary.as_str(), exec.args);
        Ok(GenericSupervisor::new(cmd))
    }
}