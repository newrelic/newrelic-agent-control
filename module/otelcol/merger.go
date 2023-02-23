package otelcol

import (
	"bytes"
	"errors"
	"fmt"

	"gopkg.in/yaml.v3"
)

const mergedConfigName = "config.yaml"

var ErrUnsupported = errors.New("local configs unsupported")

type Merger struct {
	LocalConfigPath string

	// TODO: This should come from opamp server.
	APIKey string
}

func (om Merger) Merge(opAMPConfig map[string][]byte) (map[string][]byte, error) {
	if om.LocalConfigPath != "" {
		return nil, ErrUnsupported
	}

	keys := make([]string, 0, len(opAMPConfig))
	for key := range opAMPConfig {
		keys = append(keys, key)
	}

	var final map[string]interface{}

	for _, name := range keys {
		config := opAMPConfig[name]

		configMap := map[string]interface{}{}
		err := yaml.Unmarshal(config, &configMap)
		if err != nil {
			return nil, fmt.Errorf("unmarshalling config %q: %w", name, err)
		}

		final = deepMerge(final, configMap)
	}

	finalBinary, err := yaml.Marshal(final)
	if err != nil {
		return nil, fmt.Errorf("converting merged config to yaml: %w", err)
	}

	// TODO: This should come from opamp server.
	finalBinary = bytes.ReplaceAll(finalBinary, []byte("$API_KEY"), []byte(om.APIKey))

	return map[string][]byte{
		mergedConfigName: finalBinary,
	}, nil
}

func deepMerge(a, b map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{}, len(a))
	for k, v := range a {
		out[k] = v
	}

	for k, v := range b {
		if vMap, isMap := v.(map[string]interface{}); isMap {
			if outV, isOnOut := out[k]; isOnOut {
				if outVMap, isOutMap := outV.(map[string]interface{}); isOutMap {
					out[k] = deepMerge(outVMap, vMap)
					continue
				}
			}
		}
		out[k] = v
	}

	return out
}
