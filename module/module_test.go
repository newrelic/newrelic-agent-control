package module

import (
	"net/http"
	"reflect"
	"testing"
)

func TestRemote_HTTPHeader(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		headers map[string]string
		want    http.Header
	}{
		{
			name: "Converts_Headers",
			headers: map[string]string{
				"foo": "bar",
				"boo": "baz",
			},
			want: map[string][]string{
				"foo": {"bar"},
				"boo": {"baz"},
			},
		},
	}
	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			r := RemoteConfig{
				Headers: tt.headers,
			}

			if got := r.HTTPHeader(); !reflect.DeepEqual(got, tt.want) {
				t.Errorf("HTTPHeader() = %v, want %v", got, tt.want)
			}
		})
	}
}
