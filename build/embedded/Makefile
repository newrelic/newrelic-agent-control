GOCMD?= go
GOOS=$(shell $(GOCMD) env GOOS)
GOARCH=$(shell $(GOCMD) env GOARCH)
BINARY_NAME=embedded-downloader-$(GOOS)-$(GOARCH)
# flags
ARCH?=amd64
STAGING?=false

.DEFAULT_GOAL := all

.PHONY: all
all: run

.PHONY: clean
clean:
	@if [ "$(shell pwd | grep -Eo 'build/embedded$$')" = "build/embedded" ]; then \
		rm -rf bin/; \
		rm -rf artifacts/; \
    fi

.PHONY: build
build: dependencies
	@go build -o bin/$(BINARY_NAME) ./...

.PHONY: dependencies
dependencies:
	@go mod download

.PHONY: run
run:
	@go run ./... -arch $(ARCH) -staging $(STAGING)

.PHONY: debug
debug:
	@go build -gcflags "all=-N -l" -o ./bin/$(BINARY_NAME)_debug ./...
	@dlv --listen=:2345 --headless=true --api-version=2 --accept-multiclient exec ./bin/$(BINARY_NAME)_debug

.PHONY: test
test:
	@go test ./...