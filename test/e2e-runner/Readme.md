# Local e2e execution

## Windows

Requirements:
- Windows Virtual Machine with rust installed
- Administrator privileges

1. Run a PowerShell **Administrator** console
2. Execute the e2e-runner (scenarios and required arguments are self-documented):

  ```
  PS> cargo run -p e2e-runner -- --help
  This tool runs end-to-end tests for newrelic-agent-control on Windows.

    PREREQUISITES:
    - Must be run as Administrator on Windows

    Usage: e2e-runner.exe [OPTIONS] <COMMAND>

    Commands:
      install Simple installation of Agent Control on Windows
      help     Print this message or the help of the given subcommand(s)

    Options:
      -l, --log-level <LOG_LEVEL>
              Log level (trace, debug, info, warn, error)

              [default: info]

      -h, --help
              Print help (see a summary with '-h')

    PS> cargo run -p e2e-runner -- install --help
    Simple installation of Agent Control on Windows

    Usage: e2e-runner.exe install --zip-package <ZIP_PACKAGE>

    Options:
      -z, --zip-package <ZIP_PACKAGE>  Path to the Windows zip package file
      -h, --help                       Print help
  ```

## Linux

Requirements:
  - Ubuntu Linux Virtual Machine (Eg: vagrant + parallels)
  - The Virtual Machine needs rust and docker

Vagrantfile example:

```ruby
Vagrant.configure("2") do |config|
  config.vm.box = "bento/ubuntu-24.04"
  config.vm.box_version = "202404.26.0"
  config.vm.synced_folder "<YOUR-AGENT-CONTROL-SOURCE-CODE-PATH>", "/newrelic-agent-control"
  config.vm.provision "shell", inline: <<-SHELL
    rm /etc/machine-id
    rm /var/lib/dbus/machine-id
    systemd-machine-id-setup
    apt-get update
    apt install build-essential rustup docker.io -y
    systemctl start docker.service
    systemctl enable docker.service
  SHELL
end
```

1. Compile the e2e-runner

  ```bash
  $ cd /newrelic-agent-control
  $ cargo build -p e2e-runner
  ```

2. Execute the e2e-runner (scenarios and arguments are self-documented)

  ```bash
  $ sudo -i
  root@vagrant:~# /newrelic-agent-control/target/debug/e2e-runner --help
  This tool runs end-to-end tests for newrelic-agent-control on Linux.

  PREREQUISITES:
  - Debian package manager
  - Systemctl
  - Run as root

  Usage: e2e-runner [OPTIONS] <COMMAND>

  Commands:
    infra-agent  Arguments to be set for every test that needs Agent Control installation
    help         Print this message or the help of the given subcommand(s)

  Options:
    -l, --log-level <LOG_LEVEL>
            Log level (trace, debug, info, warn, error)

            [default: info]

    -h, --help
            Print help (see a summary with '-h')

  root@vagrant:~# /newrelic-agent-control/target/debug/e2e-runner infra-agent --help
  Arguments to be set for every test that needs Agent Control installation

  Usage: e2e-runner infra-agent [OPTIONS] --deb-package-dir <DEB_PACKAGE_DIR> --nr-api-key <NR_API_KEY> --nr-license-key <NR_LICENSE_KEY> --nr-account-id <NR_ACCOUNT_ID> --system-identity-client-id <SYSTEM_IDENTITY_CLIENT_ID> --agent-control-private-key <AGENT_CONTROL_PRIVATE_KEY> --agent-control-version <AGENT_CONTROL_VERSION>

  Options:
        --deb-package-dir <DEB_PACKAGE_DIR>
            Folder where '.deb' packages are stored
        --recipes-repo <RECIPES_REPO>
            Recipes repository [default: https://github.com/newrelic/open-install-library.git]
        --recipes-repo-branch <RECIPES_REPO_BRANCH>
            Recipes repository branch [default: main]
        --nr-api-key <NR_API_KEY>
            New Relic API key for programmatic access to New Relic services
        --nr-license-key <NR_LICENSE_KEY>
            New Relic license key for agent authentication
        --nr-account-id <NR_ACCOUNT_ID>
            New Relic account identifier for associating the agent
        --system-identity-client-id <SYSTEM_IDENTITY_CLIENT_ID>
            System Identity client id
        --agent-control-private-key <AGENT_CONTROL_PRIVATE_KEY>
            System Identity private key
        --agent-control-version <AGENT_CONTROL_VERSION>
            Specific version of agent control to install
        --nr-region <NR_REGION>
            New Relic region [default: US]
        --migrate-config-infra <MIGRATE_CONFIG_INFRA>
            Flag to migrate existing infrastructure agent configuration [default: true]
    -h, --help
            Print help
  ```
