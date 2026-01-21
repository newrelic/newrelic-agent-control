# OCI Repository and AgentControl

## Overview
AgentControl manages agent packages (and in the future agentTypes) distributed as OCI (Open Container Initiative) artifacts. 
The package manager handles downloading, extracting, installing packages from OCI registries.

Package references are constructed from three components:
- Registry URL (e.g., `registry.example.com`)
- Repository path (e.g., `agents/my-agent`)
- Version (optional): Can be tag (`:v1.0.0`), digest (`@sha256:...`), or both

Such data is taken from the AgentType configuration, from the Packages section.

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

Currently, the whole operation is blocking the subAgent thread, until it terminates. 
In the next iterations, we will have a non-blocking implementation to avoid the subAgent to be blocked by this operation.

## Error Handling

**Installation Failures**:
- Download errors → Retry if configured, then fail
- Invalid artifact (not exactly 1 file) → Fail with `InvalidData`
- Extraction errors → Delete partial installation directory, fail
- Temp cleanup errors → Installation fails

## Installation Process
Currently, there is no installation step or script execution, just extraction.
We expect to support installation scripts in the future. TODO

## Signature Verification
TODO @danielorihuela

## Garbage collection
TODO not implemented yet

## Agent Types Management
TODO not implemented yet


