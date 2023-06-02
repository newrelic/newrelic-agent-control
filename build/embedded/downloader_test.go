package main

import (
	"errors"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"io"
	"net/http"
	"os"
	"strings"
	"testing"
)

func TestDownload(t *testing.T) {
	testCases := []struct {
		name              string
		url               string
		destinationFolder string
		response          http.Response
		httpError         error
		expectedContent   string
		expectedError     error
	}{
		{
			name:              "this is the first case",
			url:               "some url",
			destinationFolder: os.TempDir(),
			response:          http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader("first case"))},
			expectedContent:   "first case",
		},
		{
			name:              "http not found",
			url:               "some url",
			destinationFolder: os.TempDir(),
			response:          http.Response{StatusCode: 404, Body: io.NopCloser(strings.NewReader("not found"))},
			expectedError:     errDownloadingFile,
		},
		{
			name:              "http error",
			url:               "some url",
			destinationFolder: os.TempDir(),
			httpError:         errors.New("some error"),
			expectedError:     errDownloadingFile,
		},
	}

	var downloader = DefaultDownloader{}
	for i := range testCases {
		testCase := testCases[i]
		t.Run(testCase.name, func(t *testing.T) {

			httpGetter = func(url string) (resp *http.Response, err error) { return &testCase.response, testCase.httpError }

			fullPath, err := downloader.Download(testCase.url, testCase.destinationFolder)

			if testCase.expectedError != nil {
				assert.ErrorIs(t, err, testCase.expectedError)
			} else {
				require.NoError(t, err)
				data, err := os.ReadFile(fullPath)
				assert.NoError(t, err)
				assert.Equal(t, testCase.expectedContent, string(data))
			}
		})
	}
}
