package monitor

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"

	log "github.com/sirupsen/logrus"
)

//nolint:gochecknoglobals // Unexported immutable default.
var defaultBackoff = FixedBackoff(1 * time.Second)

type Monitor struct {
	// Command is a path to the binary that will be run.
	Command string
	// Arguments is a slice of arguments to be passed to command.
	Arguments []string
	// Backoff policy to restart a failed process. If empty it defaults to waiting one second between attempts
	// (defaultBackoff).
	Backoff Backoff
}

func (m *Monitor) Start(ctx context.Context) error {
	if m.Backoff == nil {
		m.Backoff = defaultBackoff
	}

	runtimeErrCh, err := m.supervise(ctx)
	if err != nil {
		return fmt.Errorf("supervising %q: %w", asCMDline(m.Command, m.Arguments), err)
	}

	for {
		select {
		case <-ctx.Done():
			return fmt.Errorf("stopped supervising process: %w", ctx.Err())

		case runtimeErr := <-runtimeErrCh:
			log.Warnf("Monitor %q exited with: %v", asCMDline(m.Command, m.Arguments), runtimeErr)

			backoff, bErr := m.Backoff.Backoff()
			if bErr != nil {
				return fmt.Errorf("not retrying process due to backoff policy: %w", bErr)
			}

			log.Infof("Restarting after %v", backoff)

			time.Sleep(backoff)
			runtimeErrCh, err = m.supervise(ctx)
			if err != nil {
				return fmt.Errorf("supervising %q: %w", asCMDline(m.Command, m.Arguments), err)
			}
		}
	}
}

func (m *Monitor) supervise(ctx context.Context) (chan error, error) {
	//nolint:gosec // Command and arguments are intentionally user-defined.
	cmd := exec.CommandContext(ctx, m.Command, m.Arguments...)
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("starting process: %w", err)
	}

	runtimeErrCh := make(chan error)

	go func() {
		runtimeErrCh <- cmd.Wait()
		close(runtimeErrCh)
	}()

	return runtimeErrCh, nil
}

// asCMDLine joins command and args on a space-delimited string. It is mostly used for logging.
func asCMDline(command string, args []string) string {
	return strings.Join(append([]string{command}, args...), " ")
}
