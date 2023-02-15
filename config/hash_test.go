package config

import (
	"bytes"
	"testing"
)

func TestHash(t *testing.T) {
	t.Parallel()

	oneFile := map[string][]byte{
		"file1": []byte("contents1"),
	}

	twoFiles := map[string][]byte{
		"file1": []byte("contents1"),
		"file2": []byte("contents2"),
	}

	twoFilesDifferentContent := map[string][]byte{
		"file1": []byte("contents1"),
		"file2": []byte("contents3"),
	}

	if bytes.Equal(hash(oneFile), hash(twoFiles)) {
		t.Fatalf("hash for one file and two files are the same")
	}

	if bytes.Equal(hash(twoFiles), hash(twoFilesDifferentContent)) {
		t.Fatalf("hash file sets with different contents are the same")
	}
}
