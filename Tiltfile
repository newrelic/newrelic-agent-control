# -*- mode: Python -*-
#### Config

# All default configs are set for local execution of Tilt and e2e test run.

namespace = os.getenv('NAMESPACE','default')

# Options:
# local: Use a local copy of the chart defined in 'local_chart_repo'.
# branch: Git clones and uses the newrelic/helm-charts repo on a specific branch 'feature_branch'.
# helm-repo: Use latest released chart from newrelic helm repo.
chart_source = os.getenv('CHART_SOURCE','helm-repo')

sa_chart_values_file = os.getenv('SA_CHART_VALUES_FILE','local/super-agent-deployment.yml')

local_chart_repo = os.getenv('LOCAL_CHART_REPO','../helm-charts/charts/')

# Options:
# cargo: No crosscompilation, faster than docker
# docker: Supports crosscompilaton
build_with = os.getenv('BUILD_WITH','docker')

arch = os.getenv('ARCH','arm64')

# This env var is automatically added by the e2e action.
scenario_tag = os.getenv('SCENARIO_TAG')

otel_endpoint = os.getenv('OTEL_ENDPOINT','https://staging-otlp.nr-data.net:4317')

license_key = os.getenv('LICENSE_KEY')

######## Feature Branch Workaround ########

# Use the branch source to get the chart form a feature branch in the NR helm-charts repo.
# chart_source = 'branch'
feature_branch = '<feature-branch>'


#### Build SA binary
if build_with == 'cargo':
  local_resource(
      'build-binary',
      cmd="cargo build --features=k8s && mkdir -p bin && mv target/debug/newrelic-super-agent bin/newrelic-super-agent-"+arch,
      deps=[
        './super-agent',
      ]
  )
elif build_with == 'docker': 
  local_resource(
      'build-binary',
      cmd="make BUILD_MODE=debug BUILD_FEATURE=k8s ARCH=%s build-super-agent" % arch,
      deps=[
        './super-agent',
      ]
  )

#### Build the final Docker image with the binary.
docker_build(
    'tilt.local/super-agent-dev',
    context='.',
    dockerfile='./Dockerfile',
    only = ['./bin','./Dockerfile', './Tiltfile']
)

#### Adds the secret used by the e2e test to configure otel collector to send metrics.
load('ext://secret', 'secret_from_dict')
allow_k8s_contexts('minikube') # Use explicitly allowed kubeconfigs as a safety measure.
k8s_yaml(secret_from_dict(
  name='test-env',
  namespace=namespace,
  inputs = {
    'E2E_TEST_ID' : scenario_tag,
    'OTEL_ENDPOINT' : otel_endpoint,
    'LICENSE_KEY' : license_key,
}))
k8s_resource(new_name='e2e test secret',objects=['test-env:secret'])


#### Set-up charts

load('ext://helm_resource', 'helm_repo','helm_resource')
load('ext://git_resource', 'git_checkout')

update_dependencies = False
deps=[]
chart = ''

if chart_source == 'local':
  chart = local_chart_repo
  update_dependencies = True
  deps=[chart+'super-agent-deployment/templates']
elif chart_source == 'branch':
  git_checkout('https://github.com/newrelic/helm-charts#'+feature_branch, checkout_dir='local/helm-charts', unsafe_mode=False)
  chart = 'local/helm-charts/charts/'
  update_dependencies = True
  deps=[chart+'super-agent-deployment/templates']
elif chart_source == 'helm-repo':
  chart = 'newrelic/'
  helm_repo(
    'newrelic',
    'https://helm-charts.newrelic.com',
    resource_name='newrelic-helm-repo',
    )

#### Installs charts
helm_resource(
  'flux',
  chart+'super-agent',
  namespace=namespace,
  release_name='flux',
  update_dependencies=update_dependencies,
  flags=[
    '--create-namespace',
    '--version=>=0.0.0-beta',
    
    '--set=helm.create=false',
    ],
  resource_deps=['newrelic-helm-repo']
)
helm_resource(
  'sa-deployment',
  chart+'super-agent-deployment',
  deps=deps, # re-deploy chart if modified locally
  namespace=namespace,
  release_name='sa-deployment',
  update_dependencies=update_dependencies,
  flags=[
    '--create-namespace',
    '--version=>=0.0.0-beta',
    
    '--set=image.registry=tilt.local',
    '--set=image.repository=super-agent-dev',
    '--set=image.imagePullPolicy=Always',

    '--values=' + sa_chart_values_file,
    ],
  # Required to force build the image
  image_deps=['tilt.local/super-agent-dev'],
  image_keys=[('image.registry', 'image.repository', 'image.tag')],
  resource_deps=['flux','build-binary'],
)
