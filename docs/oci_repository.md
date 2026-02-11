# OCI Repository and AgentControl

## Overview
AgentControl manages agent packages (and in the future agentTypes) distributed as OCI (Open Container Initiative) artifacts. 
The package manager handles downloading, extracting, installing packages from OCI registries.

Package references are constructed from three components:
- Registry URL (e.g., `registry.example.com`)
- Repository path (e.g., `agents/my-agent`)
- Version (optional): Can be a tag (`:v1.0.0`), a digest (`@sha256:...`), when both are specified the digest takes precedence.

This data is taken from the Packages section of the [AgentType configuration](./INTEGRATING_AGENTS.md).

## Package structure

The packaged agent must comply with the [OCI image spec](https://github.com/opencontainers/image-spec). In short, that means
that a manifest or index file must exist. Besides, Agent Control expects specific values for some fields.

* `layers/mediaType` must take one of the following values:

    - `application/vnd.newrelic.agent.content.v1.zip`
    - `application/vnd.newrelic.agent.content.v1.tar+gzip`
    - `application/vnd.newrelic.agent-type.content.v1.tar+gzip`

* `annotations` must contain
    - `com.newrelic.artifact.type` with value `package` or `agent-type`

Manifest example:

```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "artifactType": "application/vnd.newrelic.agent.v1",
  "config": {
    "mediaType": "application/vnd.oci.image.config.v1+json",
    "digest": "sha256:7758599fc4d06bd93a65bf28bc98fbff6c559a9a56be1ec3d75ff6aa8a8cfe6e",
    "size": 39
  },
  "layers": [
    {
      "mediaType": "application/vnd.newrelic.agent.content.v1.zip",
      "digest": "sha256:2e2e87f3a9403e735bee76c166b7139be36c1a76079f786e21ab2ce138cd9a1a",
      "size": 21678636,
      "annotations": {
        "com.newrelic.artifact.type": "package",
        "org.opencontainers.image.title": "newrelic-infra-amd64.zip",
        "org.opencontainers.image.version": "1.71.3"
      }
    }
  ],
  "annotations": {
    "org.opencontainers.image.created": "2026-01-23T08:07:06Z"
  }
}
```

Index example:

```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.index.v1+json",
  "manifests": [
    {
      "mediaType": "application/vnd.oci.image.manifest.v1+json",
      "digest": "sha256:82677ba32d1276debe264d14ec5f7b1c61e2a9acbc8c6a6dff779d7133ec8487",
      "size": 617,
      "platform": {
        "architecture": "amd64",
        "os": "linux"
      },
      "artifactType": "application/vnd.newrelic.agent.v1"
    },
    {
      "mediaType": "application/vnd.oci.image.manifest.v1+json",
      "digest": "sha256:5a16021a5101f7ae0583cddae44ea715ad2cfd618b61b8982de1b847958260da",
      "size": 617,
      "platform": {
        "architecture": "arm64",
        "os": "linux"
      },
      "artifactType": "application/vnd.newrelic.agent.v1"
    },
    {
      "mediaType": "application/vnd.oci.image.manifest.v1+json",
      "digest": "sha256:13e6d06647bbaf4f44d4c29bb57e1078c9919da92e2aee3443c122c24b86d3cb",
      "size": 502,
      "platform": {
        "architecture": "amd64",
        "os": "windows"
      },
      "artifactType": "application/vnd.newrelic.agent.v1"
    }
  ]
}
```

## Package Installation Process

When an agent needs to install or update a package, the package manager leverages the following paths:
```
temp_package_path: <base>/packages/<agent-id>/__temp_packages/<package-id>/<sanitized-ref>
final_path:        <base>/packages/<agent-id>/stored_packages/<package-id>/<sanitized-ref>
```

The `final_path` location is where the extracted package will reside after installation and can be referenced 
by the agent through the variable `${nr-sub:packages.infra-agent.dir}`.

**Steps**:
1. Create temporary download directory
2. Download artifact (expects exactly 1 layer/file), if the file was already downloaded, skip download
3. Create final installation directory
4. Extract archive based on `PackageType` (tar.gz or zip) derived from the mime type
5. Delete temporary directory (always, even on failure)

Currently, the whole operation blocks the sub-agent thread until it terminates. 
Notice that the old subAgent (and therefore the binary) is stopped before the new one is downloaded and executed. 
In the next iterations, we will have a non-blocking implementation to avoid the subAgent to be blocked by this operation.

## Error Handling

**Installation Failures**:
- Download errors → Retry if configured, then fail
- Invalid artifact (not exactly 1 file) → Fail with `InvalidData`
- Extraction errors → Delete partial installation directory, fail
- Temp cleanup errors → Installation fails


## Local Development
When developing and debugging locally, you can use a local OCI registry. You can run it using zot: 
```bash
$ ./tools/oci-registry.sh run  
```

Notice that AC is already configured to use HTTP as protocol when connecting to `localhost:5001` if executed/built __without__ `--release`.

## Installation Process
Currently, there is no installation step or script execution, just extraction.
We expect to support installation scripts in the future. TODO

## Signature Verification

Agent Control assumes the signature in the repository is in [Simple Signing format](https://github.com/sigstore/cosign/blob/main/specs/SIGNATURE_SPEC.md#payloads) and it's been created with the [external tool process](https://docs.sigstore.dev/cosign/signing/signing_with_containers/#sign-and-upload-a-generated-payload-in-another-format-from-another-tool). 

> [!NOTE] 
> NewRelic uses a private repository. It doesn't need extra-services like [Rekor](https://docs.sigstore.dev/logging/overview/) or [Fulcio](https://docs.sigstore.dev/certificate_authority/overview/). That's the reason why Agent Control uses the external tool process instead of `cosign sign`.

As a result of the "external tool process", the OCI repository will contain two packages. One for the agent and one for the signature. The signature package contains, among other things, the payload that was signed (in json format) and it's signature in base64. Inside the payload, we find the hash of the signed agent package. This is enough to verify the signature, as we will see in a moment.

Verification Flow:

1. AC receives an order to download a package and it's data
2. Downloads the signature package
3. Verifies the signature is correct (the base64 signature "matches" the payload)
4. Get artifact hash from payload
5. Download artifact from hash (never the tag)
6. Check downloaded artifact hash value is equal to the hash in the payload

## Key Rotation

We hid an important detail in the [Signature Verification section](./oci_repository.md#signature-verification), to make it easier to understand. Agent Control **ALWAYS** downloads the public key when verifying a signature/ This avoids the problem of using a revoked key while the cache isn't updated.

That's great, but what happens during a key rotation? It depends on the specific use case. Agent Control always tries to verify the signature with every single public key published for that package. Avoiding downtimes. There are a couple of edge cases in which we can't do nothing. The user has to wait for the signature or disable signature verification.

* The first key was published
* All keys were revoked

## Garbage collection

Agent Control stores in the system the two latest installed versions of each agent. Any other version of the package is removed from the system.
You can think of it like a FIFO with size 2.

Example:

1. User installs infra agent version 1.0.0 (system stores infra 1.0.0)
2. User installs infra agent version 3.0.0 (system stores infra 1.0.0 and 3.0.0)
3. User installs infra agent version 2.0.0 (system stores infra 2.0.0 and 3.0.0)

## Agent Types Management
TODO not implemented yet


