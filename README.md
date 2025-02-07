| ⚠️ | New Relic agent control is in preview and licensed under the New Relic Pre-Release Software Notice. |
|---------------|:------------------------|

# New Relic agent control

Our agent combines the power of existing New Relic capabilities with open standards technologies. The agent control is designed to be lightweight and efficient, and it can be configured to collect a wide range of data, including metrics, traces, logs, and events. 
The agent has a modular architecture, with a generic supervisor that is responsible for orchestrating the configured agents. It also integrates with New Relic fleet manager. 

- [Getting started](#getting-started)
- [Documentation](#documentation)
- [Support](#support)
- [Privacy](#privacy)
- [Contribute](#contribute)
- [License](#license)

## Getting started

Follow the [installation steps](https://docs-preview.newrelic.com/docs/new-relic-agent-control#quickstart) to get started on Linux hosts and [running in Kubernetes](/docs/README.md#running-in-kubernetes) to run it in your cluster.

For troubleshooting, see [Diagnose issues with agent logging](https://docs-preview.newrelic.com/docs/new-relic-agent-control#troubleshooting).

## Documentation

Learn more from the [technical documentation in this repository](/docs/README.md) and the [Limited Preview Documentation](https://docs-preview.newrelic.com/docs/new-relic-agent-control).

## Support

Should you need assistance with New Relic products, you are in good hands with several support diagnostic tools and support channels.

>New Relic offers NRDiag, [a client-side diagnostic utility](https://docs.newrelic.com/docs/using-new-relic/cross-product-functions/troubleshooting/new-relic-diagnostics) that automatically detects common problems with New Relic agents. If NRDiag detects a problem, it suggests troubleshooting steps. NRDiag can also automatically attach troubleshooting data to a New Relic Support ticket. Remove this section if it doesn't apply.

If the issue has been confirmed as a bug or is a feature request, file a GitHub issue.

**Support Channels**

* [New Relic Documentation](https://docs.newrelic.com): Comprehensive guidance for using our platform
* [New Relic Community](https://forum.newrelic.com/): The best place to engage in troubleshooting questions
* [New Relic University](https://learn.newrelic.com/): A range of online training for New Relic users of every level
* [New Relic Technical Support](https://support.newrelic.com/) 24/7/365 ticketed support. Read more about our [Technical Support Offerings](https://docs.newrelic.com/docs/licenses/license-information/general-usage-licenses/support-plan).

## Privacy

At New Relic we take your privacy and the security of your information seriously, and are committed to protecting your information. We must emphasize the importance of not sharing personal data in public forums, and ask all users to scrub logs and diagnostic information for sensitive information, whether personal, proprietary, or otherwise.

We define “Personal Data” as any information relating to an identified or identifiable individual, including, for example, your name, phone number, post code or zip code, Device ID, IP address, and email address.

For more information, review [New Relic’s General Data Privacy Notice](https://newrelic.com/termsandconditions/privacy).

## Contribute

We encourage your contributions to improve this project! Keep in mind that when you submit your pull request, you'll need to sign the CLA via the click-through using CLA-Assistant. You only have to sign the CLA one time per project.

If you have any questions, or to execute our corporate CLA (which is required if your contribution is on behalf of a company), drop us an email at opensource@newrelic.com.

As noted in our [security policy](../../security/policy), New Relic is committed to the privacy and security of our customers and their data. We believe that providing coordinated disclosure by security researchers and engaging with the security community are important means to achieve our security goals.

If you would like to contribute to this project, review [these guidelines](./CONTRIBUTING.md).

## License

New Relic agent control is licensed under the New Relic Pre-Release Software Notice.

It also uses source code from third-party libraries. You can find full details on which libraries are used and the terms under which they are licensed in the third-party notices document.
