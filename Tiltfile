# -*- mode: Python -*-
# This Tiltfile is used by the e2e tests to setup the environment and for local development.

#### Config
# This env var is automatically added by the e2e action.
license_key = os.getenv('LICENSE_KEY', "")
namespace = os.getenv('NAMESPACE','default')
sa_chart_values_file = os.getenv('SA_CHART_VALUES_FILE','local/agent-control-tilt.yml')
cluster = os.getenv('CLUSTER', "")

# build_with options:
# cargo: No crosscompilation, faster than cross
# cross: Supports crosscompilaton
build_with = os.getenv('BUILD_WITH','cross')
arch = os.getenv('ARCH','arm64')

#### Build SA binary

if build_with == 'cargo':
  local_resource(
      'build-binary',
      cmd="cargo build --package newrelic_agent_control --bin newrelic-agent-control-k8s && mkdir -p bin && rm -f bin/newrelic-agent-control-"+arch+" && mv target/debug/newrelic-agent-control-k8s bin/newrelic-agent-control-"+arch,
      deps=[
        './agent-control',
      ]
  )
elif build_with == 'cross': 
  local_resource(
      'build-binary',
      cmd="make BUILD_MODE=debug BIN=newrelic-agent-control-k8s ARCH=%s build-agent-control" % arch,
      deps=[
        './agent-control',
      ]
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

######## Feature Branch Workaround ########
# Use the branch source to get the chart form a feature branch in the NR helm-charts repo.
chart_source = os.getenv('CHART_SOURCE', 'branch') # local|branch|helm-repo
feature_branch = 'gsanchez/feat/use-ac-deployment-dependency'

# relative path to the NR Helm Charts repo on your local machine
local_chart_repo = os.getenv('LOCAL_CHARTS_PATH','')

#### Set-up charts
load('ext://helm_resource', 'helm_repo','helm_resource')
load('ext://git_resource', 'git_checkout')
update_dependencies = False
deps=[]
chart = ''
extra_resource_deps=[]

if chart_source == 'local':
  chart = local_chart_repo
  update_dependencies = True
  deps=[chart+'agent-control/charts/agent-control-deployment/templates']
elif chart_source == 'branch':
  git_checkout('https://github.com/newrelic/helm-charts#'+feature_branch, checkout_dir='local/helm-charts', unsafe_mode=False)
  chart = 'local/helm-charts/charts/'
  update_dependencies = True
  deps=[chart+'agent-control-deployment/templates']
elif chart_source == 'helm-repo':
  chart = 'newrelic/'
  helm_repo(
    'newrelic',
    'https://helm-charts.newrelic.com',
    resource_name='newrelic-helm-repo',
    )
  extra_resource_deps=['newrelic-helm-repo']

flags_helm = ['--create-namespace','--version=>=0.0.0-beta','--set=agent-control-deployment.image.imagePullPolicy=Always','--values=' + sa_chart_values_file]

if license_key != '':
  flags_helm.append('--set=global.licenseKey='+license_key)
  
if cluster != '':
  flags_helm.append('--set=global.cluster='+cluster)

#### Installs charts
helm_resource(
  'flux',
  chart+'agent-control',
  deps=deps, # re-deploy chart if modified locally
  namespace=namespace,
  release_name='sa',
  update_dependencies=update_dependencies,
  flags=flags_helm,
  image_deps=['tilt.local/agent-control-dev'],
  image_keys=[('agent-control-deployment.image.registry', 'agent-control-deployment.image.repository', 'agent-control-deployment.image.tag')],
  resource_deps=['build-binary']+extra_resource_deps
)

# We had flaky e2e test failing due to timeout applying the chart on 30s
update_settings(k8s_upsert_timeout_secs=150)
