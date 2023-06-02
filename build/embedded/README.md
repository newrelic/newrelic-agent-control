## Summary


For limited preview, the Meta Agent will be shipped with the necessary dependencies including:

* newrelic-infra
* nr-otel-collector
* nri-flex
* nri-docker
* nri-prometheus
* fluent-bit-plugin


## Build

Binaries location: ./bin/embedded-downloader-linux-{GOARCH}

```
# GOARCH is taken from Go env
make build

# GOARCH can be overriden
GOARCH=arm64 make build
```

## Run


