package main

import (
	"os"
	"os/exec"
	"path/filepath"
)

type UncompressorDeb struct {
	uncompressorTarGz UncompressorTarGz
}

func NewUncompressorDeb(uncompressorTarGz UncompressorTarGz) *UncompressorDeb {
	return &UncompressorDeb{uncompressorTarGz: uncompressorTarGz}
}

func (u UncompressorDeb) Uncompress(compressedFilePath string, fileType FileType, destination string) error {
	cmd := exec.Command("ar", []string{"x", compressedFilePath}...)
	cmd.Dir = destination
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	err := cmd.Run()
	if err != nil {
		return err
	}

	return u.uncompressorTarGz.Uncompress(filepath.Join(destination, "data.tar.gz"), fileType, destination)
}
