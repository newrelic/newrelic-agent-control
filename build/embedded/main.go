package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"log"
	"os"
	"path/filepath"
)

var errTemporaryFolderCreation = errors.New("error creating temporary folder for artifacts")
var errDownloadingArtifact = errors.New("error download artifact")
var errParsingUrlTemplate = errors.New("error parsing url template")

func main() {
	// Flags
	staging := flag.Bool("staging", false, "use stagingUrl")
	arch := flag.String("arch", "amd64", "architecture")
	flag.Parse()

	var versionMap map[string]string
	err := json.Unmarshal([]byte(os.Getenv("ARTIFACTS_VERSIONS")), &versionMap)
	if err != nil {
		log.Fatalf("cannot parse ARTIFACTS_VERSIONS env var: %v", err)
	}

	// Config
	cnf, err := configFromFile(
		*staging,
		*arch,
		versionMap,
	)
	if err != nil {
		log.Fatalf("cannot parse config: %v", err)
	}

	// Download artifacts
	downloader := DefaultDownloader{}
	fileMover := DefaultFileMover{}
	uncompressorTarGz := UncompressorTarGz{}
	uncompressorDeb := NewUncompressorDeb(uncompressorTarGz)
	uncompressor := NewUncompressorSystem(uncompressorTarGz, *uncompressorDeb)
	fileTypeDetector := DefaultFileTypeDetector{}

	errCh := make(chan error)
	for i := range cnf.Artifacts {
		artifact := cnf.Artifacts[i]
		go func(artifact Artifact) {
			errCh <- downloadArtifact(artifact, downloader, fileMover, uncompressor, fileTypeDetector)
		}(artifact)
	}

	// Error control
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
		return fmt.Errorf("cannot download artifact %s: %w", artifact.Name, err)
	}

	artifactPath, err := downloader.Download(url)
	if err != nil {
		return fmt.Errorf("cannot download artifact %s: %w", artifact.Name, err)
	}

	//temporary folder where artifacts (compressed or not) are stored
	tempFolder := filepath.Dir(artifactPath)

	//get filetype to decide later if we uncompress it or not
	filetype, err := fileTypeDetector.Detect(artifactPath)
	if err != nil {
		return fmt.Errorf("cannot detect file type %s: %w", artifact.Name, err)
	}

	//uncompress if necessary
	if filetype.IsCompressed() {
		err = uncompressor.Uncompress(artifactPath, filetype, tempFolder)
		if err != nil {
			return fmt.Errorf("cannot uncompress file %s: %w", artifact.Name, err)
		}
	}

	// copy all the files in the artifact to the destination folder
	for j := range artifact.Files {
		artifactFile := artifact.Files[j]

		dest, err := artifactFile.parseDest(artifact)
		if err != nil {
			return fmt.Errorf("cannot render dest template %s: %w", artifactFile, err)
		}

		src, err := artifactFile.parseSrc(artifact)
		if err != nil {
			return fmt.Errorf("cannot render srf template %s: %w", artifactFile, err)
		}

		src = filepath.Join(tempFolder, src)
		dest = filepath.Join(dest, filepath.Base(src))

		//ensure dest exists
		err = os.MkdirAll(filepath.Dir(dest), 0700)
		if err != nil {
			return fmt.Errorf("cannot create destination folder for artifact %s to %s : %w", src, dest, err)
		}

		err = fileMover.Move(src, dest)
		if err != nil {
			return fmt.Errorf("cannot move artifact file from %s to %s : %w", src, dest, err)
		}
	}

	return nil
}
