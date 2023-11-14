package main

import (
	"os"
	"errors"
	"os/exec"
	"path/filepath"
)

type UncompressorDeb struct {
	uncompressorTarGz UncompressorTarGz
}

func NewUncompressorDeb(uncompressorTarGz UncompressorTarGz) *UncompressorDeb {
	return &UncompressorDeb{uncompressorTarGz: uncompressorTarGz}
}

func filePathExists(filePath string) bool {
	if _, err := os.Stat(filePath); err == nil {
		return true
	}
	return false
}

func uncompressDeb(debFilePath string, destination string) error {
	cmd := exec.Command("ar", []string{"x", debFilePath}...)
	cmd.Dir = destination
	cmd.Stdout = os.Stderr
	cmd.Stderr = os.Stderr

	err := cmd.Run()
	return err
}

func (u UncompressorDeb) Uncompress(compressedFilePath string, fileType FileType, destination string) error {
	err := uncompressDeb(compressedFilePath, destination)
	if err != nil {
		return err
	}

	// A list of expected filenames for the data tarball.
	tarFileNames := []string{"data.tar.gz", "data.tar"}
	var errs error

	// Try each filename in the list tarFileNames.
	for _, tarFileName := range tarFileNames {
		tarFilePath := filepath.Join(destination, tarFileName)
		if filePathExists(tarFilePath) {
			err := u.uncompressorTarGz.Uncompress(tarFilePath, fileType, destination)
			if err == nil {
				// We only need one data tarball to uncompress.
				// If we get here, any previous errors can be discarded.
				return nil
			}
			errs = errors.Join(errs, err)
		} else {
			errs = errors.Join(errs, errors.New("Could not find file path: " + tarFilePath))
		}
	}
	return errs
}
