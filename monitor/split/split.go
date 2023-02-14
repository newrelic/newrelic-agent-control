package split

import (
	"errors"
	"fmt"
	"strings"
)

var ErrSyntax = errors.New("invalid syntax")

// Split takes a command line and returns an slice where the first element is the binary name, and the remaining
// elements are the arguments (argv).
func Split(cmdline string) ([]string, error) {
	for _, ch := range cmdline {
		switch ch {
		case '\'', '"', '`', '\\', '\n':
			return nil, fmt.Errorf("illegal character %q: quotes or escape characters are not supported: %w", ch, ErrSyntax)
		default:
			continue
		}
	}

	return strings.Split(cmdline, " "), nil
}
