package main

import "testing"
import "github.com/stretchr/testify/assert"

func Test_DefaultFileTypeDetector(t *testing.T) {
	testCases := []struct {
		name              string
		path              string
		expectedExtension string
	}{
		{
			name:              "no extension",
			path:              "this/is/a/path",
			expectedExtension: "",
		},
		{
			name:              "one extension",
			path:              "this/is/a/path/with_extension.zip",
			expectedExtension: ".zip",
		},
		{
			name:              "two extensions",
			path:              "this/is/a/path/with_extension.tar.gz",
			expectedExtension: ".tar.gz",
		},
	}

	det := DefaultFileTypeDetector{}
	for i := range testCases {
		testCase := testCases[i]
		t.Run(testCase.name, func(t *testing.T) {
			extension, err := det.Detect(testCase.path)
			assert.NoError(t, err)
			assert.Equal(t, FileType{Type: testCase.expectedExtension}, extension)
		})
	}
}
