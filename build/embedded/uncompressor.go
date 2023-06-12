package main

import (
	"errors"
)

var errNotSupported = errors.New("compression not supported")

type Uncompressor interface {
	Uncompress(compressedFilePath string, fileType FileType, destination string) error
}

type UncompressorSystem struct {
	uncompressorTarGz UncompressorTarGz
	uncompressorDeb   UncompressorDeb
}

func NewUncompressorSystem(uncompressorTarGz UncompressorTarGz, uncompressorDeb UncompressorDeb) *UncompressorSystem {
	return &UncompressorSystem{uncompressorTarGz: uncompressorTarGz, uncompressorDeb: uncompressorDeb}
}

func (u UncompressorSystem) Uncompress(compressedFilePath string, fileType FileType, destination string) error {
	switch true {
	case fileType.IsTarGz():
		return u.uncompressorTarGz.Uncompress(compressedFilePath, fileType, destination)
	case fileType.IsDeb():
		return u.uncompressorDeb.Uncompress(compressedFilePath, fileType, destination)
	default:
		return errNotSupported
	}
}
