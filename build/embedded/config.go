package main

import (
	"errors"
	"gopkg.in/yaml.v3"
	"os"
)

var errLoadingConfig = errors.New("error loading config")
var errRequiredValue = errors.New("required value missing")

type Config struct {
	Artifacts []Artifact `yaml:"artifacts"`
	defaults  Defaults   `yaml:"-"`
}

type Defaults struct {
	Destination string `yaml:"destination"`
	URL         string `yaml:"url"`
	StagingURL  string `yaml:"staging_url"`
}

type Artifact struct {
	Name    string `yaml:"name"`
	URL     string `yaml:"url"`
	Version string `yaml:"version"`
	Files   []File `yaml:"files"`
	Arch    string `yaml:"-"`
}

func (a Artifact) renderedUrl() (string, error) {
	tpl, err := newTemplate("url").Parse(a.URL)
	if err != nil {
		return "", errors.Join(errParsingUrlTemplate, err)
	}

	return renderTemplate(tpl, a)
}

type File struct {
	Name string `yaml:"name"`
	Src  string `yaml:"src"`
	Dest string `yaml:"dest"`
}

func (f File) parseDest(artifact Artifact) (string, error) {
	tpl, err := newTemplate("dest").Parse(f.Dest)
	if err != nil {
		return "", err
	}

	return renderTemplate(tpl, artifact)
}

func (f File) parseSrc(artifact Artifact) (string, error) {
	tpl, err := newTemplate("src").Parse(f.Src)
	if err != nil {
		return "", err
	}
	return renderTemplate(tpl, artifact)
}

func configFromFile(staging bool, arch string) (Config, error) {
	// Read the YAML file into a byte slice
	yamlFile, err := os.ReadFile("embedded.yaml")
	if err != nil {
		return Config{}, errors.Join(errLoadingConfig, err)
	}
	return config(staging, arch, yamlFile)
}

func config(staging bool, arch string, content []byte) (Config, error) {
	// Unmarshal YAML into Config struct
	var cnf Config
	err := yaml.Unmarshal(content, &cnf)
	if err != nil {
		return Config{}, errors.Join(errLoadingConfig, err)
	}

	// Unmarshal YAML into Config Defaults struct
	err = yaml.Unmarshal(content, &cnf.defaults)
	if err != nil {
		return Config{}, errors.Join(errLoadingConfig, err)
	}

	// validate required
	if cnf.defaults.URL == "" {
		return Config{}, errors.Join(errRequiredValue, errors.New("cnf.defaults.URL is missing"))
	}
	if cnf.defaults.Destination == "" {
		return Config{}, errors.Join(errRequiredValue, errors.New("cnf.defaults.Destination is missing"))
	}
	if cnf.defaults.StagingURL == "" {
		return Config{}, errors.Join(errRequiredValue, errors.New("cnf.defaults.StagingURL is missing"))
	}

	expandDefaults(staging, arch, &cnf)

	return cnf, nil
}

func expandDefaults(staging bool, arch string, cnf *Config) {
	// fill the non specified values with defaults
	defaultUrl := cnf.defaults.URL
	if staging {
		defaultUrl = cnf.defaults.StagingURL
	}

	for i := range cnf.Artifacts {
		if cnf.Artifacts[i].URL == "" {
			cnf.Artifacts[i].URL = defaultUrl
		}
		if cnf.Artifacts[i].Arch == "" {
			cnf.Artifacts[i].Arch = arch
		}
		for j := range cnf.Artifacts[i].Files {
			if cnf.Artifacts[i].Files[j].Dest == "" {
				cnf.Artifacts[i].Files[j].Dest = cnf.defaults.Destination
			}
		}
	}
}
