package main

import (
	"github.com/stretchr/testify/mock"
)

type DownloaderMock struct {
	mock.Mock
}

func (d *DownloaderMock) Download(url string) (string, error) {
	args := d.Called(url)

	return args.String(0), args.Error(1)
}

func (d *DownloaderMock) ShouldDownload(url string, destination string) {
	d.
		On("Download", url).
		Once().
		Return(destination, nil)
}

func (d *DownloaderMock) ShouldNotDownload(url string, err error) {
	d.
		On("Download", url).
		Once().
		Return("", err)
}
