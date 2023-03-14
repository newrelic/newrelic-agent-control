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

			// Assume that within one second startup errors would have been captured
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

	// Assume that within one second startup errors would have been captured
	grace := 3 * time.Second
	time.Sleep(grace)

	select {
	case err := <-errChan:
		t.Fatalf("Monitor exited after completion %v", err)
	default:
	}

	cancel()
}

func TestProcess_BacksOff(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	dumpfile, _ := os.Create(filepath.Join(tmp, "dumpfile"))
	_ = dumpfile.Close()

	dumper := script(t, fmt.Sprintf("echo 'ran' >> %q; sleep 1; exit 1", dumpfile.Name()))

	// mp will append a "run counter" to a temporary file, wait 1 second so the error is seen as retryable, and exit.
	m := monitor.Monitor{
		Command:   "/bin/sh",
		Arguments: []string{dumper},
		Backoff:   monitor.FixedBackoff(3 * time.Second),
	}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for mp.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- m.Start(ctx)
	}()

	// After 2 seconds, process should have run once.
	// It will run again 1 (shell sleep) + 3 (backoff) - 2 (this sleep) = 2 seconds later.
	time.Sleep(2 * time.Second)
	file, _ := os.ReadFile(dumpfile.Name())
	runs := strings.Count(string(file), "ran")

	if runs != 1 {
		t.Fatalf("Monitor ran %d times, expected 1", runs)
	}

	// 3 seconds later, process should have run again.
	time.Sleep(3 * time.Second)
	file, _ = os.ReadFile(dumpfile.Name())
	runs = strings.Count(string(file), "ran")

	if runs != 2 {
		t.Fatalf("Monitor ran %d times, expected 2", runs)
	}

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

	hash := hex.EncodeToString(sha256.New().Sum([]byte(contents)))
	dir := t.TempDir()
	path := filepath.Join(dir, fmt.Sprintf("%s.sh", hash))
	err := os.WriteFile(path, []byte(fmt.Sprintf("#!/bin/sh\n%s\n", contents)), 0640)

	if err != nil {
		t.Fatalf("cannot create test script: %v", err)
	}

	return path
}
