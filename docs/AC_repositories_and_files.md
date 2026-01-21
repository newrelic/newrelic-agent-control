# Agent Control repositories and files

This is a documentation of the repositories and files related to Agent Control.

### Description of important files and directories

#### Static files
The parent directories `/etc/newrelic-agent-control/...` and `C:\Program Files\New Relic\newrelic-agent-control...` 
are used to store the **static** configs of AC and the values for its defined 
sub-agents inside the `local-data` directory for each sub-agent.
These files are expected to be put there and edited manually by the customer (or the installation process). 
Notice that the Static config of AC it is not hot-reloaded, 
so changes to it will only take effect after AC is restarted.

This is also where the identity private key of AC is stored, inside the `keys` directory.

Notice the file `environment_variables.yaml` that in windows is used to pass environment variables to be injected in the agents.


#### Dynamic files
The remote configurations and in general any files expected to dynamically change during AC execution are stored in 
`/var/lib/newrelic-agent-control` and `C:\ProgramData\New Relic\newrelic-agent-control`. 
Several kinds of transient files might be present there at any time, and AC might 
delete some of them (or all) when it first boots to start from a clean slate:

- The remote configurations of AC and each sub-agent, retrieved as is from FC, are stored in their respective subfolder 
inside `fleet-data`, in a file named `remote_config.yaml`. For example, `fleet-data/agent-control/remote_config.yaml`. 
Some other tracking information are present, such as the remote config hashes and host identifiers.
- The rendered files that are expected to be used by the sub-agent process directly (like configuration files for the 
New Relic Infrastructure Agent) will be added to the `auto-generated` directory, with a subdirectory being created for each sub-agent ID.

Moreover, `filesystem` is the directory where Agent Control will render files for each sub-agent, moreover, each agentType could create
its own subdirectories inside it to store files that are not managed by AC but are expected to be persistent across restarts.

#### Logs
The directory inside `[...]/log/<agent-id>` will store the logs if file logging was configured, 
following a similar directory structure for AC and the sub-agents.


### What's about K8s?

In k8s deployments, the same structure is followed, but configMaps and secrets.
The private key of the system identity is stored in a secret that by default is stored in `agent-control-auth` secret.
Everything that is static is expected in `local-data-<agentID>`, on the other hand, everything dynamic is stored in `fleet-data-<agentID>`
Obviously, there are no packages stored.

## Filesystem layout

The following shows the directory structure used by Agent Control, assuming an existing sub-agent with the ID `newrelic-infra`.

### Linux

```console
/
├── etc
│   └── newrelic-agent-control
│       ├── keys
│       │    └── agent-control-identity.key
│       └── local-data
│              ├── agent-control
│              │    ├── instance_id.yaml
│              │    └── local_config.yaml
│              └── newrelic-infra
│                   ├── instance_id.yaml
│                   └── local_config.yaml
└── var
    ├── lib
    │   └── newrelic-agent-control
    │       ├── packages
    │       │    └── nr-infra
    │       │         ├──  __temp_packages  
    │       │         └── stored_packages
    │       │              └── infra-agent
    │       │                   └── oci_ghcr_io__newrelic__testing_infra_agent_v1_71_3
    │       │                        │   newrelic-infra
    │       │                        └── integrations
    │       │                             └── nri-docker
    │       ├── fleet-data
    │       │    ├── agent-control
    │       │    │    └── remote_config.yaml
    │       │    └── newrelic-infra
    │       │         └── remote_config.yaml 
    │       └── filesystem
    │            └── newrelic-infra
    │                ├── integrations.d
    │                │   └── nri-redis.yaml
    │                └── newrelic-infra.yaml
    └── log
        ├── newrelic-agent-control
        │   └── newrelic-agent-control.log.2025-01-15-23
        └── newrelic-infra
            ├── stdout.log.2025-01-15-23
            └── stderr.log.2025-01-15-23
```

### Windows

```console
C:\Program Files\New Relic\newrelic-agent-control
│   environment_variables.yaml
│   newrelic-agent-control.exe
├───keys
│       agent-control-identity.key
└───local-data
    ├───agent-control
    │       local_config.yaml
    │
    └───nr-infra
            local_config.yaml

C:\ProgramData\New Relic\newrelic-agent-control
├───filesystem
│   └───nr-infra
│       ├───config
│       │       newrelic-infra.yaml
│       └───data
├───fleet-data
│   ├───agent-control
│   │       instance_id.yaml
│   └───nr-infra
│           instance_id.yaml
├───logs
│   └───nr-infra
│           stderr.log.2026-01-19-22
└───packages
    └───nr-infra
        ├───stored_packages
        │   └───infra-agent
        │       └───oci_ghcr_io__newrelic__testing_infra_agent_v1_71_3
        │           │   newrelic-infra.exe
        │           └───integrations
        │                   nri-docker.exe
        └───__temp_packages
            └───infra-agent
```
