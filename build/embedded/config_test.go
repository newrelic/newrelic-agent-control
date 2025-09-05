package main

import (
	"fmt"
	"github.com/stretchr/testify/assert"
	"testing"
)

func Test_ConfigExpand(t *testing.T) {
	testCases := []struct {
		name         string
		content      string
		arch         string
		expectedConf Config
		expectedErr  error
	}{
		{
			name: "fallback to default",
			arch: "amd64",
			content: `
# artifacts
artifacts:
  - name: newrelic-infra
    url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
    files:
      - name: newrelic-infra binary
        src: newrelic-infra/usr/bin/newrelic-infra
        dest: "./artifacts/{{.Arch}}"

  - name: nr-otel-collector
    url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
    files:
      - name: nr-otel-collector-binary
        src: nr-otel-collector/usr/bin/nr-otel-collector
        dest: "./artifacts/{{.Arch}}"
      - name: another file
        src: nr-otel-collector/usr/bin/another-file
        dest: "./artifacts/{{.Arch}}"
`,
			expectedConf: Config{
				Artifacts: []Artifact{
					{
						Name:    "newrelic-infra",
						URL:     "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
						Version: "1.42.2",
						Files: []File{
							{
								Name: "newrelic-infra binary",
								Src:  "newrelic-infra/usr/bin/newrelic-infra",
								Dest: "./artifacts/{{.Arch}}",
							},
						},
						Arch: "amd64",
					},
					{
						Name:    "nr-otel-collector",
						URL:     "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
						Version: "0.1.0",
						Files: []File{
							{
								Name: "nr-otel-collector-binary",
								Src:  "nr-otel-collector/usr/bin/nr-otel-collector",
								Dest: "./artifacts/{{.Arch}}",
							},
							{
								Name: "another file",
								Src:  "nr-otel-collector/usr/bin/another-file",
								Dest: "./artifacts/{{.Arch}}",
							},
						},
						Arch: "amd64",
					},
				},
			},
		},
		{
			name: "error when artifact name is missing",
			arch: "amd64",
			content: `
    artifacts:
      - url: "some-url" # 'name' field is missing
        files:
          - name: a-file
            src: a-src
            dest: a-dest
    `,
			expectedErr: fmt.Errorf("artifact at index 0 is missing a required 'name'"),
		},
		{
			name: "error when version is missing from map",
			arch: "amd64",
			content: `
    artifacts:
      - name: unknown-artifact # This name is not in the versions map
        url: "some-url"
        files:
          - name: a-file
            src: a-src
            dest: a-dest
    `,
			expectedErr: fmt.Errorf("version not found for artifact: 'unknown-artifact'"),
		},
		{
			name: "error when artifact url is missing",
			arch: "amd64",
			content: `
    artifacts:
      - name: newrelic-infra # Has a name, but the URL is missing
        files:
          - name: a-file
            src: a-src
            dest: a-dest
    `,
			expectedErr: fmt.Errorf("artifact 'newrelic-infra' is missing required field 'url'"),
		},
		{
			name: "error when file src is missing",
			arch: "amd64",
			content: `
    artifacts:
      - name: newrelic-infra
        url: "some-url"
        files:
          - name: a-file
            dest: a-dest # 'src' field is missing
    `,
			expectedErr: fmt.Errorf("file 'a-file' in artifact 'newrelic-infra' is missing a required 'src' field"),
		},
	}
	for i := range testCases {
		testCase := testCases[i]
		t.Run(testCase.name, func(t *testing.T) {
			conf, err := config(
				testCase.arch,
				map[string]string{"newrelic-infra": "1.42.2", "nr-otel-collector": "0.1.0"},
				[]byte(testCase.content),
			)
			if testCase.expectedErr == nil {
				assert.NoError(t, err)
				assert.Equal(t, testCase.expectedConf, conf)
			} else {
				assert.Error(t, err)
                assert.EqualError(t, err, testCase.expectedErr.Error())
			}
		})
	}
}
