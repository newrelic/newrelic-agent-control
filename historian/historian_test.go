package historian_test

import (
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/newrelic/supervisor/historian"
)

func TestHistorian(t *testing.T) {
	t.Parallel()

	tdir := t.TempDir()

	first := filepath.Join(tdir, "first")
	second := filepath.Join(tdir, "second")
	third := filepath.Join(tdir, "third")

	h := historian.Historian{HistorySize: 1}

	mkdir(t, first)
	err := h.Push(first)
	if err != nil {
		t.Fatalf("error pushing folder: %v", err)
	}
	assertExists(t, first)
	assertNotExists(t, second)
	assertNotExists(t, third)

	mkdir(t, second)
	err = h.Push(second)
	if err != nil {
		t.Fatalf("error pushing folder: %v", err)
	}
	assertExists(t, first)
	assertExists(t, second)
	assertNotExists(t, third)

	mkdir(t, third)
	err = h.Push(third)
	if err != nil {
		t.Fatalf("error pushing folder: %v", err)
	}
	assertNotExists(t, first)
	assertExists(t, second)
	assertExists(t, third)

	folder, err := h.Rollback()
	if err != nil {
		t.Fatalf("error rolling back folder: %v", err)
	}
	if folder != second {
		t.Fatalf("Expected %q, got %q", second, folder)
	}
	assertNotExists(t, first)
	assertExists(t, second)
	assertNotExists(t, third)

	folder, err = h.Rollback()
	if !errors.Is(err, historian.ErrNoEntry) {
		t.Fatalf("expected ErrNoEntry on last entry, got: %v", err)
	}
	// Entry should still exist.
	assertExists(t, second)
}

func TestHistorian_Clean(t *testing.T) {
	h := historian.Historian{HistorySize: 99999}

	tdir := t.TempDir()

	names := []string{"1", "2", "3", "4", "5", "6", "7", "8", "9"}
	for _, name := range names {
		folder := filepath.Join(tdir, name)
		mkdir(t, folder)
		err := h.Push(folder)
		if err != nil {
			t.Fatalf("pushing %q: %v", folder, err)
		}
	}

	for _, name := range names {
		folder := filepath.Join(tdir, name)
		assertExists(t, folder)
	}

	h.HistorySize = 4
	err := h.Clean()
	if err != nil {
		t.Fatalf("cleaning old entries: %v", err)
	}

	shouldDeleted := names[:4]
	shouldExist := names[5:]

	for _, name := range shouldDeleted {
		folder := filepath.Join(tdir, name)
		assertNotExists(t, folder)
	}
	for _, name := range shouldExist {
		folder := filepath.Join(tdir, name)
		assertExists(t, folder)
	}
}

func mkdir(t *testing.T, folder string) {
	t.Helper()

	err := os.MkdirAll(folder, 0777)
	if err != nil {
		t.Fatalf("error creating folder %q: %v", folder, err)
	}
}

func assertExists(t *testing.T, folder string) {
	t.Helper()

	info, err := os.Stat(folder)
	if err != nil {
		t.Fatalf("cannot stat %q: %v", folder, err)
	}
	if !info.IsDir() {
		t.Fatalf("%q is not a directory", folder)
	}
}

func assertNotExists(t *testing.T, folder string) {
	t.Helper()

	_, err := os.Stat(folder)
	if errors.Is(err, os.ErrNotExist) {
		return
	}

	if err != nil {
		t.Fatalf("asserting non-existence of %q: %v", folder, err)
	}

	t.Fatalf("%q exists", folder)
}
