# Local e2e execution

The `e2e-runner` executes the on-host end-to-end scenarios that CI runs in
[`component_onhost_e2e.yml`](../../.github/workflows/component_onhost_e2e.yml). The runner must run on the
machine under test (it installs, starts and uninstalls Agent Control there).

Scenarios and their arguments are self-documented:

```console
e2e-runner --help                  # available scenarios
e2e-runner <scenario> --help       # arguments of a scenario
```

The scenarios run in CI are the `matrix.group` entries of the workflow for each platform.

## Common inputs

Every scenario that installs Agent Control needs:

- **Agent Control package**: `--artifacts-package-dir` points to a folder with the package built from your changes
  (`.deb` on Linux, `.zip` on Windows). Use a `--agent-control-version` that differs from any released version so the
  local package is the one installed. Packages are built with goreleaser, see [`.goreleaser.yml`](../../.goreleaser.yml)
  and the build-related sections of [`docs/DEVELOPMENT.md`](../../docs/DEVELOPMENT.md).
- **New Relic credentials**: account id, region, API key, license key, and a System Identity (client id and private key
  file). These are the same values CI reads from the repository secrets.
- **`--agent-metadata`**: the versions of the agents under test, as a JSON array. CI generates it with the
  [`agent-versions`](../../.github/actions/agent-versions) action; the exact value used can be seen in the logs of any CI
  run. It can also be provided through the `AGENT_METADATA` environment variable.

## Linux

Requirements:

- Ubuntu Linux Virtual Machine
- The Virtual Machine needs rust and docker
- Run the runner as root

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
    apt update
    apt install docker.io -y
    systemctl start docker.service
    systemctl enable docker.service
    # Installing build-essential package will restart a bunch of services, including sshd, therefore you be kicked out
    apt install build-essential rustup -y
    su - vagrant -c "rustup default stable"
  SHELL
end
```

1. Build the `.deb` with your changes and place it in a folder to use as `--artifacts-package-dir`. Build natively
   inside the VM: cross-compiling from another architecture may not work.
2. Compile the e2e-runner

    ```console
    cd /newrelic-agent-control
    cargo build -p e2e-runner
    ```

3. Execute a scenario as root, providing the [common inputs](#common-inputs)

    ```console
    sudo /newrelic-agent-control/target/debug/e2e-runner <scenario> [OPTIONS]
    ```

Long scenarios block the terminal: run them detached (`nohup`) and follow their output in a file.

## Windows

Requirements:

- Windows Virtual Machine with rust installed
- Administrator privileges

On an ARM64 VM, build both the e2e-runner and the Agent Control package for `x86_64-pc-windows-msvc`; Windows runs
them under emulation, and the package naming expects `amd64`.

1. Build the Agent Control `.zip` and place it in a folder to use as `--artifacts-package-dir`.
2. Run a PowerShell **Administrator** console
3. Execute a scenario, providing the [common inputs](#common-inputs)

    ```console
    PS> cargo run -p e2e-runner -- <scenario> [OPTIONS]
    ```

The scenario teardown runs the shipped `uninstall.ps1`, so Agent Control ends up fully uninstalled from the VM even when
the scenario succeeds.
