FROM alpine:3.18

ARG TARGETARCH

RUN apk add --no-cache --upgrade && apk add --no-cache tini

COPY --chmod=755 bin/newrelic-super-agent-${TARGETARCH} /bin/newrelic-super-agent

USER nobody

ENTRYPOINT ["/sbin/tini", "--", "/bin/newrelic-super-agent"]
