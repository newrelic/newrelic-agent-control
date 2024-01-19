# -*- mode: Python -*-
#### Tilt args

# chart_source:
#  - local: Use a local copy of the chart defined in 'local_chart_repo'.
#  - branch: Git clones and uses the newrelic/helm-charts repo on a specific branch 'chart_repo_branch'.
#  - helm-repo: Use the newrelic helm repo.
config.define_string('chart_source')
config.define_string('local_chart_repo')
config.define_string('chart_repo_branch')
config.define_string('namespace')
config.define_string('helm_values_file')
# build_with:
#  - cargo: No crosscompilation, faster than docker
#  - docker: Supports crosscompilaton
config.define_string('build_with')
config.define_string('arch')

cfg = config.parse()

######## Feature Branch Workaround ########

force_workaround = False
feature_branch = '<feature-branch>'

######## ########


# Use explicitly allowed kubeconfigs as a safety measure.
allow_k8s_contexts('minikube')

#### Build SA binary
build_with = cfg.get('build_with','docker')
arch = cfg.get('arch','arm64')
if build_with == 'cargo':
  local_resource(
      'build-binary',
      cmd="cargo build --features=k8s && mkdir -p bin && mv target/debug/newrelic-super-agent bin/newrelic-super-agent-"+arch,
      deps=[
        './super-agent',
      ]
  )
else: 
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

load('ext://helm_resource', 'helm_repo','helm_resource')
load('ext://git_resource', 'git_checkout')

update_dependencies = False
deps=[]
chart = ''

#### Pick the chart source
chart_source = cfg.get('chart_source','helm-repo')
if force_workaround:
  chart_source = 'branch'

if chart_source == 'local':
  chart = cfg.get('local_chart_repo','../helm-charts/charts/')
  update_dependencies = True
  deps=[chart+'super-agent-deployment/templates']
elif chart_source == 'branch':
  branch = cfg.get('chart_repo_branch',feature_branch)
  git_checkout('https://github.com/newrelic/helm-charts#'+branch, checkout_dir='local/helm-charts', unsafe_mode=False)
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

namespace=cfg.get('namespace', 'default')

#### Adds the secret used by the e2e test to configure otel collector to send metrics.
load('ext://secret', 'secret_from_dict')
k8s_yaml(secret_from_dict(
  name='test-env',
  namespace=namespace,
  inputs = {
    'E2E_TEST_ID' : os.getenv('SCENARIO_TAG'),
    'LICENSE_KEY' : os.getenv('LICENSE_KEY'),
}))
k8s_resource(new_name='e2e test secret',objects=['test-env:secret'])

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

    '--values=' + cfg.get('helm_values_file','local/super-agent-deployment.yml'),
    ],
  # Required to force build the image
  image_deps=['tilt.local/super-agent-dev'],
  image_keys=[('image.registry', 'image.repository', 'image.tag')],
  resource_deps=['super-agent','build-binary'],
)
