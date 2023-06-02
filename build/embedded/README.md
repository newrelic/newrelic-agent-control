## Summary


For limited preview, the Meta Agent will be shipped with the necessary dependencies including:

* newrelic-infra
* nr-otel-collector
* nri-flex
* nri-docker
* nri-prometheus
* fluent-bit-plugin

### Globally (from root dir)

## Build

```
make build/embedded
```

## Clean

```
make build/embedded-clean
```

### Locally

## Build

Binaries location: ./bin/embedded-downloader-linux-{GOARCH}

```
# GOARCH and GOOS are taken from Go env
make build

# GOARCH andGOOS can be overridden
GOARCH=arm64 GOOS=linux make build
```

## Clean

```
make clean
```

## Run

```
make run

#flags
STAGING=true ARCH=amd64 make run
```

## Test

```
make test
```