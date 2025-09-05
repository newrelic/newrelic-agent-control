package main

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"strings"
	"text/template"

	"gopkg.in/yaml.v3"
)

var errLoadingConfig = errors.New("error loading config")
var errRequiredValue = errors.New("required value missing")

type Config struct {
	Artifacts []Artifact `yaml:"artifacts"`
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

func configFromFile(arch string, versions map[string]string) (Config, error) {
	// Read the YAML file into a byte slice
	yamlFile, err := os.ReadFile("embedded.yaml")
	if err != nil {
		return Config{}, errors.Join(errLoadingConfig, err)
	}
	return config(arch, versions, yamlFile)
}

func config(arch string, versions map[string]string, content []byte) (Config, error) {
	// Unmarshal YAML into Config struct
	var cnf Config
	err := yaml.Unmarshal(content, &cnf)
	if err != nil {
		return Config{}, errors.Join(errLoadingConfig, err)
	}

	if err := processAndValidateConfig(arch, versions, &cnf); err != nil {
		return Config{}, err
	}

	fmt.Println("FINAL CONFIG:")
	fmt.Println(cnf)

	return cnf, nil
}

func processAndValidateConfig(arch string, versions map[string]string, cnf *Config) error {
	for i := range cnf.Artifacts {
		artifact := &cnf.Artifacts[i]

		if artifact.Name == "" {
			return fmt.Errorf("artifact at index %d is missing a required 'name'", i)
		}

		if artifact.URL == "" {
			return fmt.Errorf("artifact '%s' is missing required field 'url'", artifact.Name)
		}

		artifact.Arch = arch

		if v, ok := versions[artifact.Name]; ok {
			artifact.Version = v
		} else {
			return fmt.Errorf("version not found for artifact: '%s'", artifact.Name)
		}

		for j := range artifact.Files {
			file := &artifact.Files[j]

			if file.Src == "" {
				return fmt.Errorf("file '%s' in artifact '%s' is missing a required 'src' field", file.Name, artifact.Name)
			}

			if file.Dest == "" {
				return fmt.Errorf("file '%s' in artifact '%s' is missing required field 'dest'", file.Name, artifact.Name)
			}
		}
	}
	return nil
}

// newTemplate creates a new template and adds the helper trimv function
func newTemplate(name string) *template.Template {
	return template.New(name).Funcs(
		template.FuncMap{
			// trimv is a helper template function that removes leading v from the input string, typically a version
			"trimv": func(str string) string {
				return strings.TrimPrefix(str, "v")
			},
		},
	)
}

func renderTemplate(tpl *template.Template, artifact Artifact) (string, error) {
	urlbuf := &bytes.Buffer{}
	err := tpl.Execute(urlbuf, artifact)
	if err != nil {
		return "", err
	}
	return urlbuf.String(), nil
}
