package process

import "time"

// Backoff governs how much time a process supervisor should wait before attempting to restart a process upon failure.
type Backoff interface {
	// Backoff is called on each supervised process failure when such failures are considered transient.
	// The process supervisor will wait the returned duration before attempting to start the process again.
	// If Backoff returns an error, the supervisor will not try to restart the process and will exit with an error
	// instead.
	Backoff() (time.Duration, error)
}

// BackoffFunc is a function that implements the Backoff interface.
// By casting any `func() (time.Duration, error)` to BackoffFunc, it can be used as a Backoff.
type BackoffFunc func() (time.Duration, error)

func (bf BackoffFunc) Backoff() (time.Duration, error) {
	return bf()
}

func FixedBackoff(fixed time.Duration) BackoffFunc {
	return func() (time.Duration, error) {
		return fixed, nil
	}
}
