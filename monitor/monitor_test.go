package monitor_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/newrelic/supervisor/monitor"
)

func TestProcess_Exits_On_Error(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	nonexec, _ := os.Create(filepath.Join(tmp, "nonexec"))
	_, _ = nonexec.WriteString(`#!/bin/sh
sleep infinity
`)
	_ = nonexec.Close()

	for _, tc := range []struct {
		name string
		path string
	}{
		{name: "Non_Existing_Binary", path: "/var/empty/nonexiting-binary"},
		{name: "Non_Executable_Binary", path: nonexec.Name()},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()

			m := monitor.Monitor{Command: tc.path}

			ctx, cancel := context.WithCancel(context.Background())

			// Channel for m.Start to report any error back without relying on unprotected shared variables.
			errChan := make(chan error)
			go func() {
				errChan <- m.Start(ctx)
			}()

			// Startup errors should have been captured within 1 second of starting.
			grace := 1 * time.Second
			time.Sleep(grace)

			select {
			case <-errChan:
			default:
				t.Fatalf("Monitor did not report startup error within %v", grace)
			}

			cancel()
		})
	}
}

func TestProcess_Does_Not_Fail_Runtime(t *testing.T) {
	t.Parallel()

	m := monitor.Monitor{Command: "/bin/sh", Arguments: []string{script(t, `sleep 1 && exit 1`)}}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for m.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- m.Start(ctx)
	}()

	// Within 3 seconds the sleep-then-exit script should have been run and exited at least twice.
	grace := 3 * time.Second
	time.Sleep(grace)

	select {
	case err := <-errChan:
		t.Fatalf("Monitor did not handle and restart supervised process error: %v", err)
	default:
	}

	cancel()
}

func TestProcess_BacksOff(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	dumpfile, _ := os.Create(filepath.Join(tmp, "dumpfile"))
	_ = dumpfile.Close()

	// The dumper script logs executions to `dumpfile`, sleeps one second, and then exits.
	dumper := script(t, fmt.Sprintf("echo 'ran' >> %q; sleep 1; exit 1", dumpfile.Name()))

	m := monitor.Monitor{
		Command:   "/bin/sh",
		Arguments: []string{dumper},
		Backoff:   monitor.FixedBackoff(2 * time.Second),
	}

	ctx, cancel := context.WithCancel(context.Background())
	errChan := make(chan error)
	go func() {
		errChan <- m.Start(ctx)
	}()

	// To test the backoff policy, we will count the executions of the monitored process at several points in time:
	// | Time | Status |
	// |------|--------|
	// | 0s   | Script start (1st run) |
	// | 1s   | Script end (1st run) |
	// | 1s   | Backoff start (1st run) |
	// | 3s   | Backoff end (1st run) |
	// | 3s   | Script start (2nd run) |
	// | 4s   | Script end  (2nd run) |
	// | 4s   | Backoff start  (2nd run) |
	// | 7s   | Backoff end  (2nd run) |

	// After 2 seconds, process should have run once, and we should be in the middle of the first backoff period.
	time.Sleep(2 * time.Second)

	// The dump file should have logs for one execution.
	file, _ := os.ReadFile(dumpfile.Name())
	runs := strings.Count(string(file), "ran")

	if runs != 1 {
		t.Fatalf("Monitor ran %d times, expected 1", runs)
	}

	// It will run again at the 4 seconds mark.
	// 3 seconds later (t=5s), we should be in the middle of the second backoff, and process should have ran one more
	// time.
	time.Sleep(3 * time.Second)
	file, _ = os.ReadFile(dumpfile.Name())
	runs = strings.Count(string(file), "ran")

	if runs != 2 {
		t.Fatalf("Monitor ran %d times, expected 2", runs)
	}

	// Check that the process monitor did not return any error.
	select {
	case err := <-errChan:
		t.Fatalf("Monitor exited with error: %v", err)
	default:
	}

	cancel()
}

func TestProcess_Fails_On_BacksOff(t *testing.T) {
	t.Parallel()

	hasBackedOff := false
	//nolint:goerr113 // It should be okay to dynamically define errors only used for testing.
	backoffError := errors.New("give up")
	m := monitor.Monitor{
		Command:   "/bin/sh",
		Arguments: []string{script(t, "sleep 1; exit 1")},
		// Purpose-made backoff strategy that instructs to wait 1s on the first failure, but tells the process to
		// abort on the second.
		Backoff: monitor.BackoffFunc(func() (time.Duration, error) {
			if !hasBackedOff {
				hasBackedOff = true
				return 2 * time.Second, nil
			}

			return 0, backoffError
		}),
	}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for m.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- m.Start(ctx)
	}()

	select {
	case <-time.After(2 * time.Second):
	case err := <-errChan:
		t.Fatalf("Monitor exited before the first backoff interval: %v", err)
	}

	select {
	case <-time.After(3 * time.Second):
		t.Fatalf("Monitor did not exit after the second backoff interval")
	case err := <-errChan:
		if !errors.Is(err, backoffError) {
			t.Fatalf("Monitor exited but not with the expected error")
		}
	}

	cancel()
}

// script creates a script with the supplied contents. It returns a command that runs the script path to the script by
// invoking a shell.
// Callers should run `/bin/sh $returned_path` as opposed to the returned path directly, as the latter may not work
// on linux systems where `t.TempDir()` is mounted as `noexec`.
func script(t *testing.T, contents string) string {
	t.Helper()

	hash := hex.EncodeToString(sha256.New().Sum([]byte(contents)))[:8]
	dir := t.TempDir()
	path := filepath.Join(dir, fmt.Sprintf("%s.sh", hash))
	err := os.WriteFile(path, []byte(fmt.Sprintf("#!/bin/sh\n%s\n", contents)), 0o600)
	if err != nil {
		t.Fatalf("cannot create test script: %v", err)
	}

	return path
}
