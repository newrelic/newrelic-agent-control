package config

import (
	"bytes"
	"crypto/rand"
	"fmt"
	"os"
	"path/filepath"

	"github.com/oklog/ulid/v2"
	"github.com/open-telemetry/opamp-go/protobufs"
	log "github.com/sirupsen/logrus"
)

// Handler encapsulate common config management functionality.
// It relies on an implementation-dependent Merger, which is responsible from converting input configs from OpAMP into
// the config files agents expect.
type Handler struct {
	// Merger is used to convert, or merge, the config files coming from the OpAMP server into config files
	// understandable by an agent.
	Merger Merger
	// Root is the folder where subfolders containing the output config sets will be placed.
	Root string

	// hash contains the checksum of the last config applied.
	hash []byte
	// dir contains the directory returned for the last config applied.
	dir string
}

// Handle feeds the protobufs.AgentRemoteConfig into the configured Merger and writes the output to disk.
// Handle will place the output files on a randomly created directory, which path is returned by this function.
// A new directory is created every time the output config set changes, and the caller is expected to clean up any
// previously created directory after it has finished working with it.
// If the Merger returns an error, that same error is returned by Handle (wrapper).
func (h *Handler) Handle(remoteConfig *protobufs.AgentRemoteConfig) (string, error) {
	outConfigs, err := h.Merger.Merge(AsMap(remoteConfig))
	if err != nil {
		return h.dir, fmt.Errorf("processing configs from server: %w", err)
	}

	newHash := hash(outConfigs)
	if bytes.Equal(newHash, h.hash) {
		log.Debugf("Computed effective config has not changed, reusing old config directory")
		return h.dir, nil
	}

	configSetDir := ulid.MustNew(ulid.Now(), rand.Reader).String()
	h.dir = filepath.Join(h.Root, configSetDir)
	log.Tracef("Folder for new config set: %q", h.dir)

	for partialPath, configFile := range outConfigs {
		configFilePath := filepath.Join(h.dir, partialPath)
		configFileDir := filepath.Dir(configFilePath)

		log.Tracef("Creating directory %q for file %q", configFileDir, configFilePath)
		err := os.MkdirAll(configFileDir, 0777) // Before umask.
		if err != nil {
			return h.dir, fmt.Errorf("creating directory %q", configFileDir)
		}

		log.Tracef("Writing file %q", configFilePath)
		err = os.WriteFile(configFilePath, configFile, 0666) // Before umask
		if err != nil {
			// Return true as some configs may have been written.
			return h.dir, fmt.Errorf("creating config file %q: %w", configFilePath, err)
		}
	}

	h.hash = newHash

	return h.dir, nil
}

// AsMap converts protobufs.AgentRemoteConfig to a plain map[string][]byte.
func AsMap(remoteConfig *protobufs.AgentRemoteConfig) map[string][]byte {
	inConfigs := make(map[string][]byte)

	for name, config := range remoteConfig.Config.ConfigMap {
		inConfigs[name] = config.Body
	}

	return inConfigs
}

// Merger is an object that can receive a map of config files from the OpAMP server and write them on the
// locations where an agent expects them.
// Implementations must return errors if config files arrive from the server with unexpected names or content.
type Merger interface {
	Merge(opAMPConfig map[string][]byte) (map[string][]byte, error)
}

// MergerFunc is the function interface for Merger.
type MergerFunc func(opAMPConfig map[string][]byte) (map[string][]byte, error)

func (mf MergerFunc) Merge(opAMPConfig map[string][]byte) (map[string][]byte, error) {
	return mf(opAMPConfig)
}
