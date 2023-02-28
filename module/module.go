package module

import (
	"context"
	"net/http"

	"github.com/open-telemetry/opamp-go/protobufs"
)

type Module interface {
	Namespace() string
	Start(ctx context.Context, c Context) error
}

type Context struct {
	HostID        string
	Root          string
	RemoteConfig  RemoteConfig
	ULIDGenerator ULIDGenerator
}

type RemoteConfig struct {
	URL     string
	Headers map[string]string
}

func (r RemoteConfig) HTTPHeader() http.Header {
	hh := http.Header{}
	for k, v := range r.Headers {
		hh[k] = append(hh[k], v)
	}

	return hh
}

type ULIDGenerator func() string

func AsKeyValue(m map[string]string) (keyvalues []*protobufs.KeyValue) {
	for k, v := range m {
		keyvalues = append(keyvalues, &protobufs.KeyValue{
			Key: k,
			Value: &protobufs.AnyValue{
				Value: &protobufs.AnyValue_StringValue{StringValue: v},
			},
		})
	}

	return keyvalues
}
