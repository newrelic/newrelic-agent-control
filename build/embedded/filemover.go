package main

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
)

var errMovingFile = errors.New("error moving file")

type FileMover interface {
	Move(src string, destination string) error
}

type DefaultFileMover struct {
}

func (m DefaultFileMover) Move(src string, destination string) error {
	err := os.MkdirAll(filepath.Dir(destination), 0700)
	if err != nil {
		return errors.Join(errMovingFile, err)
	}

	cmd := exec.Command("cp", []string{"-a", src, destination}...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr

	err = cmd.Run()
	if err != nil {
		return errors.Join(errMovingFile, err)
	}

	return nil
}
