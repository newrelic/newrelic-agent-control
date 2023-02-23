package main

import (
	"os"

	"github.com/newrelic/supervisor"
	"github.com/newrelic/supervisor/hostidentifier"
	"github.com/newrelic/supervisor/module"
	"github.com/newrelic/supervisor/module/otelcol"
	log "github.com/sirupsen/logrus"
)

func main() {
	log.SetLevel(log.TraceLevel)

	lk := os.Getenv("NR_LK")
	if lk == "" {
		log.Fatalf("NR Staging License Key is missing, please set it on NR_LK env var.")
	}

	supervisorPath := "/tmp/supervisor-hack"
	err := os.MkdirAll(supervisorPath, 0770)
	if err != nil {
		log.Fatalf("Error creating %q: %v", supervisorPath, err)
	}

	sup := supervisor.Supervisor{
		Root:     supervisorPath,
		HostIDer: hostidentifier.Hostname{},
		ULIDGenerator: func() string {
			return "01GRK808RSANTAPYT0755N4JES"
		},
		RemoteConfig: module.RemoteConfig{
			URL: "https://opamp.staging-service.newrelic.com:443/v1/opamp",
			Headers: map[string]string{
				"API-Key": lk,
			},
		},
	}

	sup.Add(otelcol.Module{
		OtelcolBinaryPath: "otelcol-contrib",
	})

	err = sup.Start()
	log.Fatal(err)
}
