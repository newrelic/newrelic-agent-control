package process

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"time"

	"github.com/newrelic/supervisor/process/split"
	log "github.com/sirupsen/logrus"
)

var defaultBackoff = FixedBackoff(1 * time.Second)

type Process struct {
	// Command line to be run on a bourne shell.
	Cmdline string
	// Backoff policy to restart a failed process. If empty it defaults to waiting one second between attempts
	// (defaultBackoff).
	Backoff Backoff
}

func (p *Process) Start(ctx context.Context) error {
	if p.Backoff == nil {
		p.Backoff = defaultBackoff
	}

	runtimeErrCh, err := p.supervise(ctx)
	if err != nil {
		return fmt.Errorf("supervising %q: %w", p.Cmdline, err)
	}

	for {
		select {
		case <-ctx.Done():
			return fmt.Errorf("stopped supervising process: %w", err)

		case runtimeErr := <-runtimeErrCh:
			log.Warnf("Process %q exited with: %v", p.Cmdline, runtimeErr)

			backoff, bErr := p.Backoff.Backoff()
			if bErr != nil {
				return fmt.Errorf("not retrying process due to backoff policy: %w", bErr)
			}

			log.Infof("Restarting after %v", backoff)

			time.Sleep(backoff)
			runtimeErrCh, err = p.supervise(ctx)
			if err != nil {
				return fmt.Errorf("supervising %q: %w", p.Cmdline, err)
			}
		}
	}
}

func (p *Process) supervise(ctx context.Context) (chan error, error) {
	args, err := split.Split(p.Cmdline)
	if err != nil {
		return nil, fmt.Errorf("splitting cmdline: %w", err)
	}

	cmd := exec.CommandContext(ctx, args[0], args[1:]...)
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr
	err = cmd.Start()
	if err != nil {
		return nil, fmt.Errorf("starting process: %w", err)
	}

	runtimeErrCh := make(chan error)

	go func() {
		runtimeErrCh <- cmd.Wait()
		close(runtimeErrCh)
	}()

	return runtimeErrCh, nil
}
