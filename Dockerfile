# Docker file use for the released version of the Super Agent.

# Using debian image until the super agent is statically compiled, and move to Alpine.
FROM debian:bookworm-slim

ARG TARGETARCH

RUN apt update && apt install tini

COPY --chmod=755 bin/newrelic-super-agent-${TARGETARCH} /bin/newrelic-super-agent

USER nobody

ENTRYPOINT ["/usr/bin/tini", "--", "/bin/newrelic-super-agent"]
