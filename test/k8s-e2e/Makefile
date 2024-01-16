.PHONY: all
all: local-e2e-test

NEWRELIC_E2E ?= go run github.com/newrelic/newrelic-integration-e2e-action@latest
.PHONY: local-e2e-test
local-e2e-test:
	$(NEWRELIC_E2E) \
		--commit_sha=local \
		--retry_attempts=5 \
		--retry_seconds=60 \
		--account_id=${ACCOUNT_ID} \
		--api_key=${API_REST_KEY} \
		--license_key=${LICENSE_KEY} \
		--spec_path=e2e-spec.yml \
		--verbose_mode=true \
		--region=Staging \
		--agent_enabled="false"
