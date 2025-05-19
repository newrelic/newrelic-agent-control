package main

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
)

const downloaderTempFolderPrefix = "agent-control-downloader-"

var errDownloadingFile = errors.New("error downloading file")
var errCreatingDestinationFile = errors.New("error creating destination file")
var errWritingFile = errors.New("error writing destination file")

type Downloader interface {
	Download(url string) (string, error)
}

type HttpGetter func(url string) (resp *http.Response, err error)

var httpGetter HttpGetter = http.Get

type DefaultDownloader struct {
}

func (d DefaultDownloader) Download(url string) (string, error) {

	destinationFolder, err := createTemporaryFolder()
	if err != nil {
		return "", errors.Join(errDownloadingArtifact, err)
	}

    fmt.Println("URLS:", url)

	response, err := httpGetter(url)
	if err != nil {
		return "", errors.Join(errDownloadingFile, err)
	}

	if response.StatusCode >= 400 {
		return "", errors.Join(errDownloadingFile, fmt.Errorf("error downloading file from %s: %d", url, response.StatusCode))
	}

	urlParts := strings.Split(url, "/")[:]
	filename := urlParts[len(urlParts)-1]
	fullPath := fmt.Sprintf("%s/%s", strings.TrimRight(destinationFolder, "/"), filename)

	f, err := os.Create(fullPath)
	if err != nil {
		return "", errors.Join(errCreatingDestinationFile, err)
	}

	_, err = io.Copy(f, response.Body)
	if err != nil {
		return "", errors.Join(errWritingFile, err)
	}

	return fullPath, f.Close()
}

func createTemporaryFolder() (string, error) {

	path, err := os.MkdirTemp("", downloaderTempFolderPrefix)
	if err != nil {
		return "", errors.Join(errTemporaryFolderCreation, err)
	}

	return path, nil
}
