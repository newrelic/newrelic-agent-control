package process_test

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/newrelic/supervisor/process"
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

			p := process.Process{Cmdline: tc.path}

			ctx, cancel := context.WithCancel(context.Background())

			// Channel for p.Start to report any error back without relying on unprotected shared variables.
			errChan := make(chan error)
			go func() {
				errChan <- p.Start(ctx)
			}()

			// Assume that within one second startup errors would have been captured
			grace := 1 * time.Second
			time.Sleep(grace)

			select {
			case _ = <-errChan:
			default:
				t.Fatalf("Process did not report startup error within %v", grace)
			}

			cancel()
		})
	}
}

func TestProcess_Does_Not_Fail_Runtime(t *testing.T) {
	t.Parallel()

	p := process.Process{Cmdline: "sleep 1 && exit 1"}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for p.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- p.Start(ctx)
	}()

	// Assume that within one second startup errors would have been captured
	grace := 3 * time.Second
	time.Sleep(grace)

	select {
	case err := <-errChan:
		t.Fatalf("Process exited after completion %v", err)
	default:
	}

	cancel()
}

func TestProcess_BacksOff(t *testing.T) {
	t.Parallel()

	tmp := t.TempDir()
	dumper, _ := os.Create(filepath.Join(tmp, "dumper"))
	_ = dumper.Close()

	// p will append a "run counter" to a temporary file, wait 1 second so the error is seen as retryable, and exit.
	p := process.Process{
		Cmdline: fmt.Sprintf("echo 'ran' >> %q; sleep 1; exit 1", dumper.Name()),
		Backoff: process.FixedBackoff(3 * time.Second),
	}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for p.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- p.Start(ctx)
	}()

	// After 2 seconds, process should have run once.
	// It will run again 1 (shell sleep) + 3 (backoff) - 2 (this sleep) = 2 seconds later.
	time.Sleep(2 * time.Second)
	file, _ := os.ReadFile(dumper.Name())
	runs := strings.Count(string(file), "ran")

	if runs != 1 {
		t.Fatalf("Process ran %d times, expected 1", runs)
	}

	// 3 seconds later, process should have run again.
	time.Sleep(3 * time.Second)
	file, _ = os.ReadFile(dumper.Name())
	runs = strings.Count(string(file), "ran")

	if runs != 2 {
		t.Fatalf("Process ran %d times, expected 2", runs)
	}

	select {
	case err := <-errChan:
		t.Fatalf("Process exited with error: %v", err)
	default:
	}

	cancel()
}

func TestProcess_Fails_On_BacksOff(t *testing.T) {
	t.Parallel()

	hasBackedOff := false
	backoffError := errors.New("give up")
	p := process.Process{
		Cmdline: "sleep 1; exit 1",
		// Purpose-made backoff strategy that instructs to wait 1s on the first failure, but tells the process to
		// abort on the second.
		Backoff: process.BackoffFunc(func() (time.Duration, error) {
			if !hasBackedOff {
				hasBackedOff = true
				return 2 * time.Second, nil
			}

			return 0, backoffError
		}),
	}

	ctx, cancel := context.WithCancel(context.Background())

	// Channel for p.Start to report any error back without relying on unprotected shared variables.
	errChan := make(chan error)
	go func() {
		errChan <- p.Start(ctx)
	}()

	select {
	case <-time.After(2 * time.Second):
	case err := <-errChan:
		t.Fatalf("Process exited before the first backoff interval: %v", err)
	}

	select {
	case <-time.After(3 * time.Second):
		t.Fatalf("Process did not exit after the second backoff interval")
	case err := <-errChan:
		if !errors.Is(err, backoffError) {
			t.Fatalf("Process exited but not with the expected error")
		}
	}

	cancel()
}
