package supervisor_test

import (
	"os"
	"testing"

	"github.com/newrelic/supervisor"
	"github.com/newrelic/supervisor/hostidentifier"
	"github.com/newrelic/supervisor/module"
	"github.com/newrelic/supervisor/module/otelcol"
	log "github.com/sirupsen/logrus"
)

func TestDemo(t *testing.T) {
	t.Parallel()

	log.StandardLogger().SetLevel(log.DebugLevel)

	root := t.TempDir()

	s := supervisor.Supervisor{
		HostIDer: hostidentifier.ListWith(hostidentifier.Hostname{}, hostidentifier.Fake("test-unknown")),
		RemoteConfig: module.RemoteConfig{
			URL: "https://opamp.staging-service.newrelic.com:443/v1/opamp",
			Headers: map[string]string{
				"API-Key": "e2e1131b8ed49b66b87c4f0af68fd8acFFFFNRAL",
			},
		},
		Root: os.DirFS(root),
	}

	s.Add(otelcol.Module{})

	err := s.Start()
	if err != nil {
		t.Fatalf("Supervisor returned error: %v", err)
	}
}
