package split_test

import (
	"errors"
	"testing"

	"github.com/google/go-cmp/cmp"
	"github.com/newrelic/supervisor/process/split"
)

func TestSplit(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name    string
		cmdline string
		args    []string
		err     error
	}{
		{
			name:    "Single_Command",
			cmdline: "/foo/bar",
			args:    []string{"/foo/bar"},
		},
		{
			name:    "Command_With_Spaces",
			cmdline: "/foo/bar arg1 arg2",
			args:    []string{"/foo/bar", "arg1", "arg2"},
		},
		{
			name:    "Command_With_Newline",
			cmdline: "/foo/bar\narg1 arg2",
			err:     split.ErrSyntax,
		},
		{
			// Quotes are unsupported for now
			name:    "Errors_On_Double_Quotes",
			cmdline: `/foo/bar "one arg"`,
			err:     split.ErrSyntax,
		},
		{
			// Quotes are unsupported for now
			name:    "Errors_On_Single_Quotes",
			cmdline: `/foo/bar 'one arg'`,
			err:     split.ErrSyntax,
		},
		{
			// Backslashes are unsupported for now
			name:    "Errors_On_Escape",
			cmdline: "/foo/bar one\\ arg",
			err:     split.ErrSyntax,
		},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			result, err := split.Split(tc.cmdline)
			if !errors.Is(err, tc.err) {
				t.Fatalf("expected error to be %v, got %v", tc.err, err)
			}

			diff := cmp.Diff(tc.args, result)
			if diff != "" {
				t.Fatalf("results did not match expected:\n%s", diff)
			}
		})
	}
}
