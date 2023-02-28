package supervisor

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"path/filepath"

	"github.com/newrelic/supervisor/hostidentifier"
	"github.com/newrelic/supervisor/module"
	"github.com/oklog/ulid/v2"
	log "github.com/sirupsen/logrus"
)

var (
	ErrNoHostID  = errors.New("could not find a Host ID from the supplied providers")
	ErrNoModules = errors.New("no enabled modules found")
)

type Supervisor struct {
	Root         string
	HostIDer     hostidentifier.IDer
	RemoteConfig module.RemoteConfig

	ULIDGenerator module.ULIDGenerator

	modules  []module.Module
	contexts []context.CancelFunc
}

func (s *Supervisor) Add(m module.Module) {
	s.modules = append(s.modules, m)
}

func (s *Supervisor) Start() error {
	if s.Root == "" {
		s.Root = "/"
	}

	if s.ULIDGenerator == nil {
		s.ULIDGenerator = func() string {
			return ulid.MustNew(ulid.Now(), rand.Reader).String()
		}
	}

	errCh := make(chan error, len(s.modules))

	id := s.HostIDer.HostID()
	if id == "" {
		return ErrNoHostID
	}

	if len(s.modules) == 0 {
		return ErrNoModules
	}

	for _, m := range s.modules {
		mContext := module.Context{
			HostID:        id,
			RemoteConfig:  s.RemoteConfig,
			Root:          filepath.Join(s.Root, m.Namespace()),
			ULIDGenerator: s.ULIDGenerator,
		}

		ctx, cancel := context.WithCancel(context.Background())
		go func(m module.Module) {
			errCh <- m.Start(ctx, mContext)
		}(m)

		s.contexts = append(s.contexts, cancel)
	}

	err := <-errCh
	log.Errorf("Module exited with an error, terminating remaining modules")
	for _, cancel := range s.contexts {
		cancel()
	}

	return fmt.Errorf("supervisor exited due to module error: %w", err)
}
