package main

import "github.com/stretchr/testify/mock"

type FileMoverMock struct {
	mock.Mock
}

func (f *FileMoverMock) Move(src string, destination string) error {
	args := f.Called(src, destination)

	return args.Error(0)
}

func (f *FileMoverMock) ShouldMove(src string, destination string) {
	f.
		On("Move", src, destination).
		Once().
		Return(nil)
}

func (f *FileMoverMock) ShouldNotMove(src string, destination string, err error) {
	f.
		On("Move", src, destination).
		Once().
		Return(err)
}
