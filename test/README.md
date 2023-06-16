# Distributions tests

Once a prerelease is finished for a New Relic OpenTelemetry Collector distribution,
the following tests will be launched to ensure packages are not malformed and can be easily
installed on the corresponding platforms.

1. Packaging installation
2. Extended packaging and binaries tests
3. Canaries

Tests launched in steps two and three use Ansible to perform the validation, thus, an inventory
file needs to be provided. EC2 instances with the corresponding inventory file
can be easily created using the [provision tool](./provision/README.md).

## Packaging installation (Molecule)

After all the packages are uploaded to the staging repositories, the pipeline will launch
the [pkg-installation-testing-action](https://github.com/newrelic/pkg-installation-testing-action) to ensure all packages
can be installed on the supported platforms. The action uses Molecule to run a container for each supported platform and
Ansible to install and assert the package version.

After a release, the action will be relaunch but pointing to the production bucket.

## Packaging extended tests

Given an Ansible inventory file, the packaging tests will execute a bunch of Ansible roles that
verify the installation, upgrade and uninstallation of a given package. In addition, service (systemd) checks
will be done to ensure the proper execution of the binaries.

The executed Ansible playbooks can be found [here](./packaging/ansible/test.yaml).

[Full documentation](./packaging/README.md).

## Canaries

Canaries is the name we use to denote a few instances that are launched for a few days before the release is triggered. Those instances run the latest two versions of the distribution as a docker container, so we can verify there are no major difference between them.

[Full documentation](./canaries/README.md).
