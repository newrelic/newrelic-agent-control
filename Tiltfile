# -*- mode: Python -*-

# Settings and defaults.
settings = {
  'namespace':'super-agent',
  'cluster_context': 'minikube',
 
  # Use local charts       
  # 'chart_repo':'../helm-charts/charts/',
}

settings.update(read_json('local/tilt_option.json', default={}))

namespace=settings.get('namespace')

# Use explicitly allowed kubeconfigs as a safety measure.
allow_k8s_contexts(settings.get('cluster_context'))

local_resource(
    'build-binary',
    cmd="make BUILD_MODE=debug BUILD_FEATURE=k8s build-super-agent",
    deps=[
        './super-agent',
    ]
)

# Build the final Docker image with the binary.
docker_build(
    'tilt.local/super-agent-dev',
    context='.',
    dockerfile='./Dockerfile',
    only = ['./bin','./Dockerfile', './Tiltfile']
)

load('ext://helm_resource', 'helm_repo','helm_resource')

helm_repo('newrelic','https://helm-charts.newrelic.com')

chart = settings.get('chart_repo','newrelic/')
update_dependencies = False
deps=[]

# Local chart config
if chart != 'newrelic/':
    update_dependencies = True
    deps=[chart+'super-agent-deployment/templates'] # re-deploy chart if modified locally

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
    ]
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

    '--values=local/super-agent-deployment-values.yml',
    ],
  # Required to force build the image
  image_deps=['tilt.local/super-agent-dev'],
  image_keys=[('image.registry', 'image.repository', 'image.tag')],
)

# To make sure your binary is built before deploying.
k8s_resource(
    'sa-deployment',
    resource_deps=['build-binary']
)
