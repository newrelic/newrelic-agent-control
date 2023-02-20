package hostidentifier_test

import (
	"testing"

	"github.com/newrelic/supervisor/hostidentifier"
)

func TestList_HostID(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name     string
		ids      []hostidentifier.IDer
		expected string
	}{
		{
			name: "Returns_First_Available",
			ids: []hostidentifier.IDer{
				hostidentifier.Fake(""),
				hostidentifier.Fake("test"),
				hostidentifier.Fake(""),
			},
			expected: "test",
		},
		{
			name: "Returns_Note",
			ids: []hostidentifier.IDer{
				hostidentifier.Fake(""),
				hostidentifier.Fake(""),
			},
			expected: "",
		},
	}

	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			
			list := hostidentifier.List{}.With(tc.ids...)
			id := list.HostID()

			if id != tc.expected {
				t.Fatalf("expected %q, got %q", tc.expected, id)
			}
		})
	}
}

func TestList_With(t *testing.T) {
	list := hostidentifier.List{}.With(hostidentifier.Fake("first"))
	list = list.With(hostidentifier.Fake("second"))
	id := list.HostID()

	expected := "first"
	if id != expected {
		t.Fatalf("expected %q, got %q", expected, id)
	}
}
