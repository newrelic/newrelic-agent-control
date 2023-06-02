package main

import (
	"bytes"
	"errors"
	"flag"
	"fmt"
	"html/template"
	"log"
	"os"
	"path/filepath"
	"strings"
)

var errTemporaryFolderCreation = errors.New("error creating temporary folder for artifacts")
var errDownloadingArtifact = errors.New("error download artifact")
var errParsingUrlTemplate = errors.New("error parsing url template")

func main() {

	// Flags
	staging := flag.Bool("staging", false, "use stagingUrl")
	arch := flag.String("arch", "amd64", "architecture")

	flag.Parse()

	// Config
	cnf, err := configFromFile(*staging, *arch)
	if err != nil {
		log.Fatalf("cannot parse config: %v", err)
	}

	// Download artifacts
	downloader := DefaultDownloader{}
	fileMover := DefaultFileMover{}
	uncompressor := UncompressorSystem{}
	fileTypeDetector := DefaultFileTypeDetector{}

	errCh := make(chan error)
	for i := range cnf.Artifacts {
		artifact := cnf.Artifacts[i]
		go func(artifact Artifact) {
			errCh <- downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
		}(artifact)
	}

	var errs error
	for _ = range cnf.Artifacts {
		if err = <-errCh; err != nil {
			errs = errors.Join(errs, err)
		}
	}
	if errs != nil {
		log.Fatalf("error downloading artifact: %v", errs)
	}
	fmt.Println("all good :)")

}

func downloadArtifact(artifact Artifact, downloader Downloader, fileMover FileMover, uncompressor Uncompressor, fileTypeDetector FileTypeDetector) error {
	url, err := artifact.renderedUrl()
	if err != nil {
		return fmt.Errorf("cannot download artifact %s: %v", artifact.Name, err)
	}

	artPath, err := downloadFile(url, downloader)
	if err != nil {
		return fmt.Errorf("cannot download artifact %s: %v", artifact.Name, err)
	}

	filetype, err := fileTypeDetector.Detect(artPath)
	if err != nil {
		return fmt.Errorf("cannot detect file type %s: %v", artifact.Name, err)
	}

	tempFolder := filepath.Dir(artPath)
	if filetype.IsCompressed() {
		err = uncompressor.Uncompress(artPath, tempFolder)
		if err != nil {
			return fmt.Errorf("cannot uncompress file %s: %v", artifact.Name, err)
		}
	}
	for j := range artifact.Files {
		artifactFile := artifact.Files[j]

		dest, err := artifactFile.parseDest(artifact)
		if err != nil {
			return fmt.Errorf("cannot render dest template %s: %v", artifactFile, err)
		}

		src, err := artifactFile.parseSrc(artifact)
		if err != nil {
			return fmt.Errorf("cannot render srf template %s: %v", artifactFile, err)
		}

		src = filepath.Join(tempFolder, src)
		dest = filepath.Join(dest, filepath.Base(src))
		err = fileMover.Move(src, dest)
		if err != nil {
			return fmt.Errorf("cannot move artifact file from %s to %s : %v", src, dest, err)
		}
	}

	return nil
}

// downloadFile creates a temporary folder and downloads the artifact file to the created folder
// it returns the absolute path to the downloaded artifact file
func downloadFile(url string, downloader Downloader) (string, error) {
	tempPath, err := createTemporaryFolder()
	if err != nil {
		return "", errors.Join(errDownloadingArtifact, err)
	}

	artifactPath, err := downloader.Download(url, tempPath)
	if err != nil {
		return "", errors.Join(errDownloadingArtifact, err)
	}

	return artifactPath, nil
}

func renderTemplate(tpl *template.Template, artifact Artifact) (string, error) {
	urlbuf := &bytes.Buffer{}
	err := tpl.Execute(urlbuf, artifact)
	if err != nil {
		return "", err
	}
	return urlbuf.String(), nil
}

func createTemporaryFolder() (string, error) {
	path, err := os.MkdirTemp("", "meta-agent-downloader-")
	if err != nil {
		return "", errors.Join(errTemporaryFolderCreation, err)
	}

	return path, nil
}

// newTemplate creates a new template and adds the helper trimv function
func newTemplate(name string) *template.Template {
	return template.New(name).Funcs(
		template.FuncMap{
			// trimv is a helper template function that removes leading v from the input string, typically a version
			"trimv": func(str string) string {
				return strings.TrimPrefix(str, "v")
			},
		},
	)
}
