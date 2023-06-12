package main

import (
	"regexp"
)

const FileTypeTarGz = ".tar.gz"
const FileTypeDeb = ".deb"

var regExtension = regexp.MustCompile(`((\.[a-z]+)+)$`)

type FileType struct {
	Type string
}

func (f FileType) IsCompressed() bool {
	return f.Type == FileTypeTarGz || f.Type == FileTypeDeb
}

func (f FileType) IsTarGz() bool {
	return f.Type == FileTypeTarGz
}

func (f FileType) IsDeb() bool {
	return f.Type == FileTypeDeb
}

type FileTypeDetector interface {
	Detect(path string) (FileType, error)
}

type DefaultFileTypeDetector struct {
}

func (d DefaultFileTypeDetector) Detect(path string) (FileType, error) {
	match := regExtension.FindAllString(path, -1)
	if len(match) == 0 {
		return FileType{Type: ""}, nil
	}
	return FileType{Type: match[0]}, nil
}
