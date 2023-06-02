package main

import "github.com/stretchr/testify/mock"

type UncompressorMock struct {
	mock.Mock
}

func (u *UncompressorMock) Uncompress(filepath string, fileType FileType, destination string) error {
	args := u.Called(filepath, fileType, destination)

	return args.Error(0)
}

func (u *UncompressorMock) ShouldUncompress(filepath string, fileType FileType, destination string) {
	u.
		On("Uncompress", filepath, fileType, destination).
		Once().
		Return(nil)
}

func (u *UncompressorMock) ShouldNotUncompress(filepath string, fileType FileType, destination string, err error) {
	u.
		On("Uncompress", filepath, fileType, destination).
		Once().
		Return(err)
}
