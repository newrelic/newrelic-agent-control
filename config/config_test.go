package config_test

import (
	"errors"
	"fmt"
	"io/fs"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/go-cmp/cmp"
	"github.com/newrelic/supervisor/config"
	"github.com/open-telemetry/opamp-go/protobufs"
)

var unknownConfig = errors.New("unsupported key")

// testMerger is a merger for tests that takes any input config and returns it renamed by replacing in with out.
// Any input config that does not start with `in` is rejected as an error.
func testMerger(opAMPConfig map[string][]byte) (map[string][]byte, error) {
	out := make(map[string][]byte)

	for k, v := range opAMPConfig {
		if strings.HasPrefix(k, "in") {
			out[strings.ReplaceAll(k, "in", "out")] = v
		} else {
			return nil, fmt.Errorf("%q: %w", k, unknownConfig)
		}
	}

	return out, nil
}

func listFiles(t *testing.T, folder string) []string {
	t.Helper()

	var files []string
	err := filepath.WalkDir(folder, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}

		if path == folder {
			// Exclude own directory from listing
			return nil
		}

		trail := ""
		if d.IsDir() {
			trail = "/"
		}

		relpath, err := filepath.Rel(folder, path)
		if err != nil {
			return fmt.Errorf("computing rel path: %w", err)
		}

		files = append(files, relpath+trail)

		return nil
	})
	if err != nil {
		t.Fatalf("reading output dir: %v", err)
	}

	return files
}

func TestHandler_Propagates_Errors(t *testing.T) {
	t.Parallel()

	td := t.TempDir()
	h := config.Handler{
		Merger: config.MergerFunc(testMerger),
		Root:   td,
	}

	_, err := h.Handle(&protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{ConfigMap: map[string]*protobufs.AgentConfigFile{
			"unsupportedConfig": {Body: []byte("file 1")},
			"in/File2":          {Body: []byte("file 2")},
		}},
	})
	if !errors.Is(err, unknownConfig) {
		t.Fatalf("Handler should have errored due to merger erroring, got %v", err)
	}
}

func TestHandler_Handles_Files(t *testing.T) {
	t.Parallel()

	td := t.TempDir()
	h := config.Handler{
		Merger: config.MergerFunc(testMerger),
		Root:   td,
	}

	t.Logf("Running for the first time")

	initialD, err := h.Handle(&protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{ConfigMap: map[string]*protobufs.AgentConfigFile{
			"inFile1":  {Body: []byte("file 1")},
			"in/File2": {Body: []byte("file 2")},
		}},
	})
	if err != nil {
		t.Fatalf("Handler errored with %v", err)
	}

	files := listFiles(t, initialD)
	if diff := cmp.Diff(files, []string{"out/", "out/File2", "outFile1"}); diff != "" {
		t.Fatalf("output files did not match expectations:\n%s", diff)
	}

	t.Logf("Running again with the same input files")

	dir, err := h.Handle(&protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{ConfigMap: map[string]*protobufs.AgentConfigFile{
			"inFile1":  {Body: []byte("file 1")},
			"in/File2": {Body: []byte("file 2")},
		}},
	})
	if err != nil {
		t.Fatalf("Handler errored with %v", err)
	}
	if initialD != dir {
		t.Fatalf("Handler should not have created any new dir as there are no changes on the second run")
	}

	t.Logf("Running a third time with a changed file")

	dir, err = h.Handle(&protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{ConfigMap: map[string]*protobufs.AgentConfigFile{
			"inFile1":  {Body: []byte("file 1")},
			"in/File2": {Body: []byte("actually it's file 3")},
		}},
	})
	if err != nil {
		t.Fatalf("Handler errored with %v", err)
	}
	if initialD == dir {
		t.Fatalf("Handler should have created a new dir as contents of in/File2 changed")
	}

	files = listFiles(t, dir)
	if diff := cmp.Diff(files, []string{"out/", "out/File2", "outFile1"}); diff != "" {
		t.Fatalf("output files did not match expectations:\n%s", diff)
	}

	t.Logf("Running a fourth time with a removed file")

	dir, err = h.Handle(&protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{ConfigMap: map[string]*protobufs.AgentConfigFile{
			"inFile1": {Body: []byte("file 1")},
		}},
	})
	if err != nil {
		t.Fatalf("Handler errored with %v", err)
	}

	files = listFiles(t, dir)
	if diff := cmp.Diff(files, []string{"outFile1"}); diff != "" {
		t.Fatalf("output files did not match expectations:\n%s", diff)
	}
}

func TestAsMap(t *testing.T) {
	in := &protobufs.AgentRemoteConfig{
		Config: &protobufs.AgentConfigMap{
			ConfigMap: map[string]*protobufs.AgentConfigFile{
				"foo": {Body: []byte("bar")},
				"boo": {Body: []byte("baz")},
			},
		},
	}

	diff := cmp.Diff(config.AsMap(in), map[string][]byte{
		"foo": []byte("bar"),
		"boo": []byte("baz"),
	})

	if diff != "" {
		t.Fatalf("map ouput did not match expectations:\n%s", diff)
	}
}
