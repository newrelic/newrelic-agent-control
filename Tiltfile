# -*- mode: Python -*-

# Settings and defaults.
project_name = 'newrelic-super-agent'
cluster_context = 'minikube'

# Use explicitly allowed kubeconfigs as a safety measure.
allow_k8s_contexts(cluster_context)

local_resource(
    'build-rust-binary',
    cmd="make build-super-agent",
    deps=[
        './src',
        'Cargo.toml',
        'Cargo.lock',
        'src'
    ]
)

# Build the final Docker image with the binary.
docker_build(
    'ci.local/super-agent-dev',
    context='.',
    dockerfile='./Dockerfile'
)

load('ext://helm_remote', 'helm_remote')

helm_remote(
    chart='super-agent-deployment',
    repo_url='https://helm-charts.newrelic.com',
    release_name='super-agent-deployment',
    namespace='default',
    version='0.0.0-beta',
    values=['tilt-dev-values.yaml'],
)

# To make sure your binary is built before deploying.
k8s_resource(
    'super-agent-deployment',
    resource_deps=['build-rust-binary']
)
