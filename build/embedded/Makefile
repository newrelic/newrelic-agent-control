
GOCMD?= go
GOOS=$(shell $(GOCMD) env GOOS)
GOARCH=$(shell $(GOCMD) env GOARCH)
BINARY_NAME=embedded-downloader-$(GOOS)-$(GOARCH)

.DEFAULT_GOAL := all

.PHONY: all
all: dependencies download

.PHONY: build
build: dependencies
	@go build -o bin/$(BINARY_NAME) ./...

.PHONY: dependencies
dependencies:
	@go mod download

.PHONY: run
run:
	@go run ./... -arch $(GOARCH)

.PHONY: debug
debug:
	@go build -gcflags "all=-N -l" -o ./bin/$(BINARY_NAME)_debug ./...
	@dlv --listen=:2145 --headless=true --api-version=2 --accept-multiclient exec ./bin/$(BINARY_NAME)_debug
