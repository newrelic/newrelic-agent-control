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

.PHONY: build
build: dependencies
	@go build -o bin/$(BINARY_NAME) ./...

.PHONY: dependencies
dependencies:
	@go mod download

.PHONY: run
run:
	@go run ./... -arch $(ARCH) -staging $(STAGING)

.PHONY: test
test:
	@go test ./...