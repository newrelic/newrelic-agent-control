# -*- mode: Python -*-
# This Tiltfile is used by the e2e tests to setup the environment and for local development.
ci_settings(readiness_timeout = '10m')

load('ext://helm_resource', 'helm_repo','helm_resource')
load('ext://git_resource', 'git_checkout')

#### Config
# This env var is automatically added by the e2e action.
license_key = os.getenv('LICENSE_KEY', "")
namespace = os.getenv('NAMESPACE','default')
sa_chart_values_file = os.getenv('SA_CHART_VALUES_FILE','local/agent-control-tilt.yml')
cluster = os.getenv('CLUSTER', "")
# Branch of the helm-charts repo to use.
feature_branch = os.getenv('FEATURE_BRANCH', "master")

# Enables basic auth in chartmuseum (for testing reasons)
# 
chartmuseum_basic_auth = os.getenv('CHARTMUSEUM_BASIC_AUTH', "")

# build_with options:
arch = os.getenv('ARCH','arm64')

#### Build SA binary
local_resource(
  'build-binary',
  cmd="make BUILD_MODE=debug ARCH=%s build-agent-control-cli" % arch +
    "&& make BUILD_MODE=debug ARCH=%s build-agent-control-k8s" % arch,
  deps= ['./agent-control'],
)

#### Build the final Docker image with the binary.
docker_build(
  'tilt.local/agent-control-dev',
  context='.',
  dockerfile='./Dockerfiles/Dockerfile_agent_control',
  only = ['./bin','./Dockerfile', './Tiltfile']
)

#### Build the final Docker image with the binary.
docker_build(
  'tilt.local/agent-control-cli-dev',
  context='.',
  dockerfile='./Dockerfiles/Dockerfile_agent_control_cli',
  only = ['./bin','./Dockerfile', './Tiltfile']
)

#### Set-up charts

#### install chart museum
helm_repo(
  'chartmuseum',
  'https://chartmuseum.github.io/charts',
  resource_name='chartmuseum-repo',
  )

chartmuseum_flags = [
  # activate API to upload charts
  '--set=env.open.DISABLE_API=false'
]

if chartmuseum_basic_auth != '':
  chartmuseum_flags.append('--set=env.existingSecret=chartmuseum-auth')
  chartmuseum_flags.append('--set=env.existingSecretMappings.BASIC_AUTH_USER=username')
  chartmuseum_flags.append('--set=env.existingSecretMappings.BASIC_AUTH_PASS=password')

  # This is the secret with the format expected by Flux HelmRepository secretRef.
  local_resource(
    'chartmuseum-auth-secret',
    cmd="""kubectl delete --ignore-not-found secret chartmuseum-auth &&
     kubectl create secret generic chartmuseum-auth --from-literal=username=testUser --from-literal=password=testPassword
     """,
    resource_deps=['chartmuseum-repo'],
  )

helm_resource(
  'chartmuseum',
  'chartmuseum/chartmuseum',
  namespace='default',
  release_name='chartmuseum',
  resource_deps=['chartmuseum-repo'],
  flags=chartmuseum_flags,
  port_forwards=['8080']
)

## we are saving the chart version for agent-control-deployment that is expected by agent-control
## also nri-bundle that is expected by some tests.
local_resource(
    'local-child-chart-upload',
    allow_parallel=True,
    cmd="""
     rm -rf local/helm-charts-tmp &&
     git clone --depth=1 https://github.com/newrelic/helm-charts --branch """ + feature_branch +"""  local/helm-charts-tmp &&
     helm package --dependency-update --version 0.0.1 --destination local/helm-charts-tmp local/helm-charts-tmp/charts/agent-control-deployment &&
     curl -u testUser:testPassword -X DELETE http://localhost:8080/api/charts/agent-control-deployment/0.0.1 &&
     curl -u testUser:testPassword --data-binary "@local/helm-charts-tmp/agent-control-deployment-0.0.1.tgz" http://localhost:8080/api/charts &&
     helm package --dependency-update --version 0.0.1 --destination local/helm-charts-tmp local/helm-charts-tmp/charts/nri-bundle &&
     curl -u testUser:testPassword -X DELETE http://localhost:8080/api/charts/nri-bundle/0.0.1 &&
     curl -u testUser:testPassword --data-binary "@local/helm-charts-tmp/nri-bundle-0.0.1.tgz" http://localhost:8080/api/charts &&
     helm package --dependency-update --version 0.0.1 --destination local/helm-charts-tmp local/helm-charts-tmp/charts/agent-control-cd &&
     curl -u testUser:testPassword -X DELETE http://localhost:8080/api/charts/agent-control-cd/0.0.1 &&
     curl -u testUser:testPassword --data-binary "@local/helm-charts-tmp/agent-control-cd-0.0.1.tgz" http://localhost:8080/api/charts
    """,
    resource_deps=['chartmuseum'],
)

ac_flags = [
  '--timeout=150s',
  '--create-namespace',
  '--set=agentControlDeployment.chartRepositoryUrl=http://chartmuseum.default.svc.cluster.local:8080',
  '--set=agentControlDeployment.chartVersion=0.0.1',
  '--version=>=0.0.0-beta',
  '--set=agentControlDeployment.chartValues.image.imagePullPolicy=Always',
  '--values=' + sa_chart_values_file,
]

ac_chart_deps = ['build-binary', 'local-child-chart-upload']

latest_flux = os.getenv('LATEST_FLUX', 'false').lower() == 'true'

if latest_flux:
    ## we are saving the latest flux version chart in the local repository and updating the dependencies to point to it in
    ## agent-control-cd so we can test the latest version of flux.
    local_resource(
        'local-latest-flux-chart-upload',
        cmd="""
         flux_latest_version=$(curl -s https://fluxcd-community.github.io/helm-charts/index.yaml | yq eval '.entries.flux2[0].version' -) &&
         cd_latest_version=$(curl -s https://newrelic.github.io/helm-charts/index.yaml | yq eval '.entries.agent-control-cd[0].version' -) &&
         yq eval ".dependencies |= map(select(.name == \\"flux2\\") | .version = \\"$flux_latest_version\\")" -i local/helm-charts-tmp/charts/agent-control-cd/Chart.yaml &&
         helm package --dependency-update --version "$cd_latest_version" --destination local/helm-charts-tmp local/helm-charts-tmp/charts/agent-control-cd &&
         curl -u testUser:testPassword -X DELETE http://localhost:8080/api/charts/agent-control-cd/${cd_latest_version} &&
         curl -u testUser:testPassword --data-binary "@local/helm-charts-tmp/agent-control-cd-${cd_latest_version}.tgz" http://localhost:8080/api/charts
        """,
        resource_deps=['local-child-chart-upload'],
    )

    ac_chart_deps.append('local-latest-flux-chart-upload')
    ac_flags.append('--set=agent-control-cd.chartRepositoryUrl=http://chartmuseum.default.svc.cluster.local:8080')

if license_key != '':
  ac_flags.append('--set=global.licenseKey='+license_key)
  
if cluster != '':
  ac_flags.append('--set=global.cluster='+cluster)


local_resource(
    'wait-install-job',
    allow_parallel=True,
    cmd="kubectl wait --for=create --timeout=200s job/ac-agent-control-install-job",
    resource_deps=['local-child-chart-upload'],
)

local_resource(
    'log-install-job',
    allow_parallel=True,
    serve_cmd="while true ; do kubectl logs -f job/ac-agent-control-install-job || sleep 1; continue ; done",
    resource_deps=['wait-install-job'],
)

#### Installs charts
helm_resource(
  'agent-control',
  'local/helm-charts-tmp/charts/agent-control',
  namespace=namespace,
  release_name='ac',
  update_dependencies=True,
  # workaround for https://github.com/tilt-dev/tilt/issues/6058
  pod_readiness='ignore',
  flags=ac_flags,
  image_deps=['tilt.local/agent-control-dev', 'tilt.local/agent-control-cli-dev'],
  image_keys=[('agent-control-deployment.image.registry', 'agent-control-deployment.image.repository', 'agent-control-deployment.image.tag'),
              [('toolkitImage.registry', 'toolkitImage.repository', 'toolkitImage.tag'),
              ('agent-control-cd.installer.image.registry', 'agent-control-cd.installer.image.repository', 'agent-control-cd.installer.image.tag')]],
  resource_deps=ac_chart_deps
)

# We had flaky e2e test failing due to timeout applying the chart on 30s
update_settings(k8s_upsert_timeout_secs=200)
