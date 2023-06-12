package main

import (
	"os"
	"os/exec"
)

type UncompressorTarGz struct {
}

func (u UncompressorTarGz) Uncompress(compressedFilePath string, _ FileType, destination string) error {
	cmd := exec.Command("tar", []string{"-C", destination, "-xf", compressedFilePath}...)
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	return cmd.Run()
}
