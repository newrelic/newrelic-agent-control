.PHONY: all
all: local-e2e-test

#REGION is where to send the e2e data. Possible values: "US", "EU", "Staging", "Local". See action.yaml for more info.
REGION ?= US
NEWRELIC_E2E ?= go run github.com/newrelic/newrelic-integration-e2e-action@latest
.PHONY: local-e2e-test
local-e2e-test:
	$(NEWRELIC_E2E) \
		--commit_sha=local \
		--retry_attempts=5 \
		--account_id=${ACCOUNT_ID} \
		--api_key=${API_REST_KEY} \
		--license_key=${LICENSE_KEY} \
		--region=$(REGION) \
		--spec_path=${SPEC_PATH} \
		--verbose_mode=true \
		--agent_enabled="false"
