package otelcol

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/newrelic/supervisor/config"
	"github.com/newrelic/supervisor/historian"
	"github.com/newrelic/supervisor/module"
	"github.com/newrelic/supervisor/monitor"
	"github.com/open-telemetry/opamp-go/client"
	"github.com/open-telemetry/opamp-go/client/types"
	"github.com/open-telemetry/opamp-go/protobufs"
	log "github.com/sirupsen/logrus"
)

const (
	AttrAgentType  = "agent.type"
	AttrInstanceID = "instance.id"
	AttrHostID     = "host.id"
)

const (
	agentName         = "otelcol"
	configName        = "config.yaml"
	configsFolderName = "config"
)

type Module struct {
	OtelcolBinaryPath string
	LocalConfigPath   string
}

func (m Module) Namespace() string {
	return agentName
}

func (m Module) Start(ctx context.Context, c module.Context) error {
	if m.OtelcolBinaryPath == "" {
		return errors.New("unspecified path to the collector binary")
	}

	u := c.ULIDGenerator()

	logger := log.StandardLogger().
		WithField(AttrAgentType, agentName).
		WithField(AttrInstanceID, u)

	configDir := filepath.Join(c.Root, u, configsFolderName)
	err := os.MkdirAll(configDir, 0750)
	if err != nil && !errors.Is(err, os.ErrExist) {
		return fmt.Errorf("creating directory for config files: %w", err)
	}

	configHandler := config.Handler{
		Merger: Merger{
			LocalConfigPath: m.LocalConfigPath,
			APIKey:          c.RemoteConfig.Headers["API-Key"],
		},
		Root: configDir,
	}

	configHistorian := historian.Historian{}

	// Cancel for an empty context. This will be replaced with the actual cancel for a real process when the first
	// config is received.
	_, cancel := context.WithCancel(context.Background())

	// Directory where configHandler stores the configs.
	currentConfigDir := ""

	opampClient := client.NewHTTP(logger)
	_ = opampClient.SetHealth(&protobufs.AgentHealth{Healthy: false})
	_ = opampClient.SetAgentDescription(&protobufs.AgentDescription{
		IdentifyingAttributes: module.AsKeyValue(map[string]string{
			AttrAgentType: agentName,
			AttrHostID:    c.HostID,
		}),
	})

	errChan := make(chan error)

	err = opampClient.Start(ctx, types.StartSettings{
		OpAMPServerURL: c.RemoteConfig.URL,
		Header:         c.RemoteConfig.HTTPHeader(),
		InstanceUid:    u,
		Callbacks: types.CallbacksStruct{
			OnConnectFunc: func() {
				logger.Debugf("Connected")
			},
			OnConnectFailedFunc: func(err error) {
				logger.Errorf("Could not connect to opamp server: %v", err)
			},
			OnErrorFunc: func(err *protobufs.ServerErrorResponse) {
				logger.Errorf("OpAMP returned an error: %v", err)
			},
			OnMessageFunc: func(ctx context.Context, msg *types.MessageData) {
				switch {
				case msg.RemoteConfig != nil:
					logger.Debugf("Got OpAMP config")

					injectSelfInstrumentation(msg.RemoteConfig, agentName, u)

					newConfigDir, err := configHandler.Handle(msg.RemoteConfig)
					if err != nil {
						errChan <- fmt.Errorf("processing config files from server: %w", err)
						return
					}

					if newConfigDir == currentConfigDir {
						log.Debugf("Effective config did not change")
						return
					}

					log.Debugf("New processed config located in %q", newConfigDir)
					currentConfigDir = newConfigDir
					hErr := configHistorian.Push(currentConfigDir)
					if hErr != nil {
						errChan <- fmt.Errorf("pushing config dir history: %w", hErr)
						return
					}

					log.Infof("Killing previous process")
					cancel()

					pm := monitor.Monitor{
						Cmdline: fmt.Sprintf("%s --config %s", m.OtelcolBinaryPath, filepath.Join(newConfigDir, configName)),
						Backoff: nil,
					}

					log.Infof("Starting new process")
					var processContext context.Context
					processContext, cancel = context.WithCancel(ctx)
					go func() {
						err := pm.Start(processContext)
						if !errors.Is(err, context.Canceled) {
							errChan <- err
						}
					}()

				default:
					logger.Debugf("Got OpAMP message: %v", msg)
				}
			},
			SaveRemoteConfigStatusFunc: func(ctx context.Context, status *protobufs.RemoteConfigStatus) {
				logger.Debugf("Remote config status: %v", status)
			},
		},
		Capabilities: protobufs.AgentCapabilities_AgentCapabilities_AcceptsRemoteConfig |
			protobufs.AgentCapabilities_AgentCapabilities_ReportsStatus,
	})

	if err != nil {
		return fmt.Errorf("starting opamp client: %w", err)
	}

	return <-errChan
}

func injectSelfInstrumentation(rc *protobufs.AgentRemoteConfig, name, ulid string) {
	rc.Config.ConfigMap["__local_selfinstr"] = &protobufs.AgentConfigFile{Body: []byte(fmt.Sprintf(`
service:
  telemetry:
    resource:
      service.name: %s
      service.instance.id: %s
`,
		name,
		ulid,
	)),
	}
}
