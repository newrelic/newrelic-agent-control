package main

import (
	"os"
	"os/exec"
)

type Uncompressor interface {
	Uncompress(filepath string, destination string) error
}

type UncompressorSystem struct {
}

func (u UncompressorSystem) Uncompress(filepath string, destination string) error {
	cmd := exec.Command("tar", []string{"-C", destination, "-xf", filepath}...)
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	return cmd.Run()
}
