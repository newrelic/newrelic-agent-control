package main

import (
	"github.com/stretchr/testify/assert"
	"testing"
)

func Test_ConfigExpand(t *testing.T) {
	testCases := []struct {
		name         string
		content      string
		staging      bool
		arch         string
		expectedConf Config
		expectedErr  error
	}{
		{
			name:    "fallback to default",
			staging: false,
			arch:    "amd64",
			content: `
# defaults
url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
staging_url: "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
destination: "./artifacts/{{.Arch}}"

# artifacts
artifacts:
  - name: newrelic-infra
    files:
      - name: newrelic-infra binary
        src: newrelic-infra/usr/bin/newrelic-infra

  - name: nr-otel-collector
    files:
      - name: nr-otel-collector-binary
        src: nr-otel-collector/usr/bin/nr-otel-collector
      - name: another file
        src: nr-otel-collector/usr/bin/another-file
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
				defaults: Defaults{
					URL:         "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					StagingURL:  "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					Destination: "./artifacts/{{.Arch}}",
				},
			},
		},
		{
			name:    "fallback to default staging url",
			staging: true,
			arch:    "amd64",
			content: `
# defaults
url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
staging_url: "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
destination: "./artifacts/{{.Arch}}"

# artifacts
artifacts:
  - name: newrelic-infra
    files:
      - name: newrelic-infra binary
        src: newrelic-infra/usr/bin/newrelic-infra

  - name: nr-otel-collector
    files:
      - name: nr-otel-collector-binary
        src: nr-otel-collector/usr/bin/nr-otel-collector
      - name: another file
        src: nr-otel-collector/usr/bin/another-file
`,
			expectedConf: Config{
				Artifacts: []Artifact{
					{
						Name:    "newrelic-infra",
						URL:     "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
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
						URL:     "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
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
				defaults: Defaults{
					URL:         "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					StagingURL:  "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					Destination: "./artifacts/{{.Arch}}",
				},
			},
		},
		{
			name:    "fallback to default + hardcoded ones mixed",
			staging: false,
			arch:    "amd64",
			content: `
# defaults
url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
staging_url: "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
destination: "./artifacts/{{.Arch}}"

# artifacts
artifacts:
  - name: newrelic-infra
    files:
      - name: newrelic-infra binary
        src: newrelic-infra/usr/bin/newrelic-infra

  - name: nr-otel-collector
    url: "http://www.some.url/and/path"
    files:
      - name: nr-otel-collector-binary
        src: nr-otel-collector/usr/bin/nr-otel-collector
        dest: "./hardcoded/dest"
      - name: another file
        src: nr-otel-collector/usr/bin/another-file
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
						URL:     "http://www.some.url/and/path",
						Version: "0.1.0",
						Files: []File{
							{
								Name: "nr-otel-collector-binary",
								Src:  "nr-otel-collector/usr/bin/nr-otel-collector",
								Dest: "./hardcoded/dest",
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
				defaults: Defaults{
					URL:         "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					StagingURL:  "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz",
					Destination: "./artifacts/{{.Arch}}",
				},
			},
		},
		{
			name:    "validate required url",
			staging: false,
			arch:    "amd64",
			content: `
# defaults
url: ""
staging_url: "https://nr-downloads-ohai-staging.s3-website-us-east-1.amazonaws.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
destination: "./artifacts/{{.Arch}}"
`,
			expectedErr: errRequiredValue,
		},
		{
			name:    "validate required staging url",
			staging: false,
			arch:    "amd64",
			content: `
# defaults
url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
staging_url: ""
destination: "./artifacts/{{.Arch}}"
`,
			expectedErr: errRequiredValue,
		},
		{
			name:    "validate required destination",
			staging: false,
			arch:    "amd64",
			content: `
# defaults
url: "https://download.newrelic.com/infrastructure_agent/binaries/linux/{{.Arch}}/{{.Name}}_linux_{{.Version | trimv}}_{{.Arch}}.tar.gz"
staging_url: "some url"
destination: ""
`,
			expectedErr: errRequiredValue,
		},
	}
	for i := range testCases {
		testCase := testCases[i]
		t.Run(testCase.name, func(t *testing.T) {
			conf, err := config(
				testCase.staging,
				testCase.arch,
				map[string]string{"newrelic-infra": "1.42.2", "nr-otel-collector": "0.1.0"},
				[]byte(testCase.content),
			)
			if testCase.expectedErr == nil {
				assert.NoError(t, err)
				assert.Equal(t, testCase.expectedConf, conf)
			} else {
				assert.ErrorIs(t, err, testCase.expectedErr)
			}
		})
	}
}
