package pkg

import (
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/go-cmp/cmp"
)

// $ openssl rand 16 > /tmp/bytes
// $ base64 < /tmp/bytes
// ROCmeZh0qlJY/setFw4m7A==
// $ xxd < /tmp/bytes
// 00000000: 44e0 a679 9874 aa52 58fe c7ad 170e 26ec  D..y.t.RX.....&.

func TestReadHashFile(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name     string
		contents string
		expected []byte
	}{
		{
			name:     "Regular_File",
			contents: "44e0a6799874aa5258fec7ad170e26ec",
			expected: b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
		},
		{
			name:     "Empty_File",
			contents: "",
			expected: nil,
		},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			root := t.TempDir()

			err := os.WriteFile(filepath.Join(root, "file"), []byte(tc.contents), 0o600)
			if err != nil {
				t.Fatalf("creating file: %v", err)
			}

			hash, err := readHashFile(filepath.Join(root, "file"))
			if err != nil {
				t.Fatalf("couldn't read hash from file: %v", err)
			}

			if diff := cmp.Diff(tc.expected, hash); diff != "" {
				t.Fatalf("decoded hash does not match expected\n%s", diff)
			}
		})
	}
}

func TestReadHashFile_Nonexisting_File(t *testing.T) {
	t.Parallel()
	root := t.TempDir()

	hash, err := readHashFile(filepath.Join(root, "file"))
	if err != nil {
		t.Fatalf("Err should be nil for non-existing file")
	}

	if hash != nil {
		t.Fatalf("Hash should be nil for non-existing file")
	}
}

func TestWriteHashFile(t *testing.T) {
	t.Parallel()

	for _, tc := range []struct {
		name     string
		contents []byte
		expected string
	}{
		{
			name:     "Regular_File",
			expected: "44e0a6799874aa5258fec7ad170e26ec",
			contents: b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
		},
	} {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			root := t.TempDir()

			err := os.WriteFile(filepath.Join(root, "file"), []byte(tc.expected), 0o600)
			if err != nil {
				t.Fatalf("creating file: %v", err)
			}

			err = writeHashFile(filepath.Join(root, "file"), tc.contents)
			if err != nil {
				t.Fatalf("couldn't write hash to file: %v", err)
			}

			fileContents, err := os.ReadFile(filepath.Join(root, "file"))
			if err != nil {
				t.Fatalf("couldn't read hash from file: %v", err)
			}

			if diff := cmp.Diff(tc.expected, string(fileContents)); diff != "" {
				t.Fatalf("decoded hash does not match expected\n%s", diff)
			}
		})
	}
}

func b64MustDecode(in string) []byte {
	b, err := base64.StdEncoding.DecodeString(in)
	if err != nil {
		panic(err)
	}

	return b
}
