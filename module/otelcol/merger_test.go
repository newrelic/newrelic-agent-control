package otelcol_test

import (
	"strings"
	"testing"

	"github.com/google/go-cmp/cmp"
	"github.com/newrelic/supervisor/module/otelcol"
)

func TestMerger(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name string
		in   map[string][]byte
		out  string

		shouldError bool
	}{
		{
			name: "Returns_One_Config_Renamed",
			in: map[string][]byte{
				"something something": []byte(strings.TrimSpace(`
indeed:
  very:
    valid: yes
this: is a yaml file
`)),
			},
			out: strings.TrimSpace(`
indeed:
    very:
        valid: "yes"
this: is a yaml file
`) + "\n",
		},
		{
			name: "Merges_Configs_Deeply",
			in: map[string][]byte{
				"one config": []byte(strings.TrimSpace(`
a: value
config:
  one: value
`)),
				"second config": []byte(strings.TrimSpace(`
b: value
config:
  another: value
`)),
			},
			out: strings.TrimSpace(`
a: value
b: value
config:
    another: value
    one: value
`) + "\n",
		},
		{
			name: "Overwrites_Entries_On_Merge",
			in: map[string][]byte{
				"aaaconfig": []byte(strings.TrimSpace(`
boo: baz
foo: bar
`)),
				"bbbconfig": []byte(strings.TrimSpace(`
foo: notBar
`)),
			},
			out: strings.TrimSpace(`
boo: baz
foo: notBar
`) + "\n",
		},
		{
			name: "Errors_On_Invalid_YAML",
			in: map[string][]byte{
				"invalid": []byte(`not valid yaml`),
			},
			shouldError: true,
		},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			out, err := otelcol.Merger{}.Merge(tc.in)
			if err != nil && !tc.shouldError {
				t.Fatalf("Unexpected error %v", err)
			}

			if diff := cmp.Diff(string(tc.out), string(out["config.yaml"])); diff != "" {
				t.Fatalf("Unexpected output\n:%s", diff)
			}
		})
	}
}
