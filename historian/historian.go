package historian

import (
	"errors"
	"fmt"
	"os"
	"sync"

	log "github.com/sirupsen/logrus"
)

// Historian remembers a history of folders, and cleans up (removes) older folders as new ones are pushed.
type Historian struct {
	// HistorySize specifies the amount of history entries to keep _in addition to_ the most recent one.
	// When set to 0, historian will only keep the most recent entry.
	HistorySize int

	// DryRun, when set to true, will cause Historian to log deletions rather than actually perform them.
	DryRun bool

	mtx     sync.Mutex
	history []string
}

// Push tells the Historian to remember a new folder, pushing it to the top of its memory.
// After pushing, Historial will clean up older folders it remembers so only HistorySize+1 folders remain.
// Historian may return I/O errors that occur when deleting old entries, but the push operation itself cannot fail.
func (h *Historian) Push(entry string) error {
	h.mtx.Lock()
	h.history = append(h.history, entry)
	h.mtx.Unlock()

	return h.Clean()
}

var ErrNoEntry = errors.New("no entry to roll back")

// Rollback makes the Historian forget and remove the most recent folder, and returns the second most recent.
// If there are no second-recent entry to return, Historian doesn't remove the most recent entry and returns ErrNoEntry.
func (h *Historian) Rollback() (string, error) {
	h.mtx.Lock()
	defer h.mtx.Unlock()

	// 0   1   2   3   len=4
	oneBeforeLast := len(h.history) - 2
	// 0   1   2   3
	//         ^ oneBeforeLast=2
	if oneBeforeLast < 0 {
		return "", ErrNoEntry
	}

	err := h.remove(h.history[len(h.history)-1])
	if err != nil {
		return "", err
	}

	h.history = h.history[:oneBeforeLast+1]

	return h.history[len(h.history)-1], nil
}

// Clean can be used to force Historian to remove old folders it remembers so only HistorySize+1 remain.
// Clean is automatically called after Push.
// Clean may return I/O errors that can take place when removing old folders.
func (h *Historian) Clean() error {
	h.mtx.Lock()
	defer h.mtx.Unlock()

	// 0   1   2   3   len=4
	mostRecent := len(h.history) - 1 // Most recent entry
	// 0   1   2   3
	//             ^ mostRecent=3
	oldest := mostRecent - h.HistorySize // Oldest one we want to keep
	// h.HistorySize=2
	// 0   1   2   3
	//     ^ oldest=1

	if oldest <= 0 {
		// Nothing to do here
		return nil
	}

	toRemove := h.history[:oldest]
	for _, dir := range toRemove {
		err := h.remove(dir)
		if err != nil {
			return err
		}
	}

	h.history = h.history[oldest:]

	return nil
}

func (h *Historian) remove(path string) error {
	if h.DryRun {
		log.Warnf("Historian dry run: Would os.RemoveAll(%q)", path)
		return nil
	}

	err := os.RemoveAll(path)
	if err != nil {
		return fmt.Errorf("removing %q: %w", path, err)
	}

	return nil
}
