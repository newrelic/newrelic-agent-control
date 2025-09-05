## Summary


For limited preview, the Agent Control will be shipped with the necessary dependencies including:

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
ARCH=amd64 make run
```

## Test

```
make test
```