package main

import (
	"errors"
	"fmt"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"path/filepath"
	"testing"
)

func Test_downloadArtifactHappyPath(t *testing.T) {
	downloader := &DownloaderMock{}
	fileMover := &FileMoverMock{}
	uncompressor := &UncompressorMock{}
	fileTypeDetector := DefaultFileTypeDetector{}

	artifact := testArtifact()

	downloadedFilePath := "some/path/for/compressed/file.tar.gz"
	downloader.ShouldDownload(artifact.URL, downloadedFilePath)

	uncompressedFilesPath := "some/path/for/compressed"
	uncompressor.ShouldUncompress(downloadedFilePath, FileType{FileTypeTarGz}, uncompressedFilesPath)

	for i := range artifact.Files {
		file := artifact.Files[i]
		fileMover.ShouldMove(fmt.Sprintf("%s/%s", uncompressedFilesPath, file.Src), fmt.Sprintf("%s/%s", file.Dest, filepath.Base(file.Src)))
	}

	err := downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
	assert.NoError(t, err)

	mock.AssertExpectationsForObjects(t, downloader, fileMover, uncompressor)
}

func Test_downloadArtifact_DownloadError(t *testing.T) {
	downloader := &DownloaderMock{}
	fileMover := &FileMoverMock{}
	uncompressor := &UncompressorMock{}
	fileTypeDetector := DefaultFileTypeDetector{}

	artifact := testArtifact()

	derr := errors.New("some error")
	downloader.ShouldNotDownload(artifact.URL, derr)

	err := downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
	assert.ErrorIs(t, err, derr)

	mock.AssertExpectationsForObjects(t, downloader, fileMover, uncompressor)
}

func Test_downloadArtifact_UncompressError(t *testing.T) {
	downloader := &DownloaderMock{}
	fileMover := &FileMoverMock{}
	uncompressor := &UncompressorMock{}
	fileTypeDetector := DefaultFileTypeDetector{}

	artifact := testArtifact()

	downloadedFilePath := "some/path/for/compressed/file.deb"
	downloader.ShouldDownload(artifact.URL, downloadedFilePath)

	uerr := errors.New("some uncompress error")
	uncompressedFilesPath := "some/path/for/compressed"
	uncompressor.ShouldNotUncompress(downloadedFilePath, FileType{FileTypeDeb}, uncompressedFilesPath, uerr)

	err := downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
	assert.ErrorIs(t, err, uerr)

	mock.AssertExpectationsForObjects(t, downloader, fileMover, uncompressor)
}

func Test_downloadArtifact_MoveError(t *testing.T) {
	downloader := &DownloaderMock{}
	fileMover := &FileMoverMock{}
	uncompressor := &UncompressorMock{}
	fileTypeDetector := DefaultFileTypeDetector{}

	artifact := testArtifact()

	downloadedFilePath := "some/path/for/compressed/file.tar.gz"
	downloader.ShouldDownload(artifact.URL, downloadedFilePath)

	uncompressedFilesPath := "some/path/for/compressed"
	uncompressor.ShouldUncompress(downloadedFilePath, FileType{FileTypeTarGz}, uncompressedFilesPath)

	// only the second file will return error
	merr := errors.New("some error moving")
	for i := range artifact.Files {
		file := artifact.Files[i]
		if i == 0 {
			fileMover.ShouldMove(fmt.Sprintf("%s/%s", uncompressedFilesPath, file.Src), fmt.Sprintf("%s/%s", file.Dest, filepath.Base(file.Src)))
		} else {

			fileMover.ShouldNotMove(fmt.Sprintf("%s/%s", uncompressedFilesPath, file.Src), fmt.Sprintf("%s/%s", file.Dest, filepath.Base(file.Src)), merr)
		}
	}

	err := downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
	assert.ErrorIs(t, err, merr)

	mock.AssertExpectationsForObjects(t, downloader, fileMover, uncompressor)
}

func testArtifact() Artifact {
	return Artifact{
		Name:    "some artifact",
		URL:     "some/url/for/compressed/file.tar.gz",
		Version: "1.2.3",
		Files: []File{
			{
				Name: "file name",
				Src:  "some file",
				Dest: "file/dest",
			},
			{
				Name: "file name 2",
				Src:  "some file 2",
				Dest: "file/dest",
			},
		},
		Arch: "some arch",
	}
}
