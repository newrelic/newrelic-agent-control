package pkg

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/open-telemetry/opamp-go/client/types"
	"github.com/open-telemetry/opamp-go/protobufs"
	log "github.com/sirupsen/logrus"
)

const (
	statusesJson  = "statuses.json"
	allHashPath   = "_all.hash"
	hashSuffix    = ".hash"
	versionSuffix = ".version"
)

var (
	ErrUnimplementedType   = errors.New("unimplemented package type")
	ErrExistsFalse         = errors.New("cannot set existence to false")
	ErrPackageExists       = errors.New("package already exists")
	ErrIllegalName         = errors.New("invalid package name")
	ErrPackageDoesNotExist = errors.New("package does not exist")
)

// Manager implements PackagesStateProvider, managing packages under the tree specified by Root.
// Manager stores packages on disk using the following structure (relative to Root):
// /statuses.json
// /_all.hash
// /package1/package1
// /package1/package1.hash
// /package1.version
// /package1.hash
type Manager struct {
	Root string
}

func (m Manager) AllPackagesHash() ([]byte, error) {
	path := filepath.Join(m.Root, allHashPath)

	hash, err := readHashFile(path)
	if err != nil {
		return nil, fmt.Errorf("reading hash from %q: %w", path, err)
	}

	return hash, nil
}

func (m Manager) SetAllPackagesHash(hash []byte) error {
	path := filepath.Join(m.Root, allHashPath)
	err := writeHashFile(path, hash)
	if err != nil {
		return err
	}

	return nil
}

func (m Manager) Packages() ([]string, error) {
	var packages []string

	err := filepath.WalkDir(m.Root, func(path string, d fs.DirEntry, err error) error {
		if path == m.Root {
			return nil
		}

		if !d.IsDir() {
			if filepath.Base(path) != allHashPath {
				log.Warnf("Extraneous file %q in package root", path)
			}

			return nil
		}

		packages = append(packages, filepath.Base(path))
		return nil
	})

	if err != nil {
		return nil, fmt.Errorf("listing packages directory: %w", err)
	}

	return packages, nil
}

func (m Manager) PackageState(name string) (types.PackageState, error) {
	emptyState := types.PackageState{}

	pkgPath := filepath.Join(m.Root, name)
	_, err := os.Stat(pkgPath)
	if errors.Is(err, os.ErrNotExist) {
		return types.PackageState{Exists: false}, nil
	}

	if err != nil {
		return emptyState, fmt.Errorf("cannot stat package file at %q", pkgPath)
	}

	hash, err := readHashFile(pkgPath + hashSuffix)
	if err != nil {
		return emptyState, err
	}

	versionFile := pkgPath + versionSuffix
	version, err := os.ReadFile(versionFile)
	if err != nil {
		return emptyState, fmt.Errorf("reading version file %q: %w", versionFile, err)
	}

	return types.PackageState{
		Exists:  true,
		Type:    0, // TODO: Unimplemented
		Hash:    hash,
		Version: string(version),
	}, nil
}

func (m Manager) SetPackageState(name string, state types.PackageState) error {
	pkgPath := filepath.Join(m.Root, name)

	info, err := os.Stat(pkgPath)
	if err != nil {
		return fmt.Errorf("cannot set state for package %q: %w", pkgPath, ErrPackageDoesNotExist)
	}

	if !info.IsDir() {
		return fmt.Errorf("internal error: package folder %q is not a folder: %w", pkgPath, ErrPackageDoesNotExist)
	}

	if !state.Exists {
		return fmt.Errorf("updating %q: %w", pkgPath, ErrExistsFalse)
	}

	if state.Type != 0 {
		return fmt.Errorf("updating %q: %w", pkgPath, ErrUnimplementedType)
	}

	err = writeHashFile(pkgPath+hashSuffix, state.Hash)
	if err != nil {
		return err
	}

	versionFile := pkgPath + versionSuffix
	err = os.WriteFile(versionFile, []byte(state.Version), 0666)
	if err != nil {
		return fmt.Errorf("writing version file %q: %w", versionFile, err)
	}

	return nil
}

func (m Manager) CreatePackage(name string, t protobufs.PackageType) error {
	if t != 0 {
		return fmt.Errorf("updating %q: %w", name, ErrUnimplementedType)
	}

	if name == strings.TrimSuffix(allHashPath, hashSuffix) {
		return fmt.Errorf("%w: package name cannot be \"_all\"", ErrIllegalName)
	}

	if strings.HasSuffix(name, hashSuffix) {
		return fmt.Errorf("%w: package name cannot end in %q", ErrIllegalName, hashSuffix)
	}

	if strings.HasSuffix(name, versionSuffix) {
		return fmt.Errorf("%w: package name cannot end in %q", ErrIllegalName, versionSuffix)
	}

	pkgPath := filepath.Join(m.Root, name)

	_, err := os.Stat(pkgPath)
	if err == nil {
		return fmt.Errorf("creating package %q: %w", pkgPath, ErrPackageExists)
	}

	if !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("checking for existence of package %q: %w", pkgPath, err)
	}

	err = os.MkdirAll(pkgPath, 0777)
	if err != nil {
		return fmt.Errorf("creating empty package %q: %w", pkgPath, err)
	}

	return nil
}

func (m Manager) FileContentHash(name string) ([]byte, error) {
	// /package1/package1.hash
	hashFile := filepath.Join(m.Root, name, name) + hashSuffix
	return readHashFile(hashFile)
}

func (m Manager) UpdateContent(_ context.Context, name string, data io.Reader, contentHash []byte) error {
	pkgPath := filepath.Join(m.Root, name)

	info, err := os.Stat(pkgPath)
	if err != nil {
		return fmt.Errorf("cannot set state for package %q: %w", pkgPath, ErrPackageDoesNotExist)
	}

	if !info.IsDir() {
		return fmt.Errorf("internal error: package folder %q is not a folder: %w", pkgPath, ErrPackageDoesNotExist)
	}

	// /package1/package1
	packageFilePath := filepath.Join(pkgPath, name)

	// Package directory should exist already as per call to CreatePackage
	file, err := os.Create(packageFilePath)
	if err != nil {
		return fmt.Errorf("creating package file %q: %w", packageFilePath, err)
	}

	defer func() {
		_ = file.Close()
	}()

	_, err = io.Copy(file, data)
	if err != nil {
		return fmt.Errorf("writing package file %q: %w", packageFilePath, err)
	}

	hashFile := filepath.Join(m.Root, name, name) + hashSuffix
	err = writeHashFile(hashFile, contentHash)
	if err != nil {
		return err
	}

	return nil
}

func (m Manager) DeletePackage(name string) error {
	pkgPath := filepath.Join(m.Root, name)

	info, err := os.Stat(pkgPath)
	if err != nil {
		return fmt.Errorf("removing package %q: %w", pkgPath, ErrPackageDoesNotExist)
	}

	if !info.IsDir() {
		return fmt.Errorf("internal error: package folder %q is not a folder: %w", pkgPath, ErrPackageDoesNotExist)
	}

	log.Infof("Removing package %q", pkgPath)
	err = os.RemoveAll(pkgPath)
	if err != nil {
		return fmt.Errorf("deleting %q: %w", pkgPath, err)
	}

	return nil
}

func (m Manager) LastReportedStatuses() (*protobufs.PackageStatuses, error) {
	statuses := protobufs.PackageStatuses{}

	jsonFilePath := filepath.Join(m.Root, statusesJson)
	jsonFile, err := os.Open(jsonFilePath)
	if err != nil {
		return nil, fmt.Errorf("opening %q: %w", jsonFilePath, err)
	}

	defer func() {
		_ = jsonFile.Close()
	}()

	err = json.NewDecoder(jsonFile).Decode(&statuses)
	if err != nil {
		return nil, fmt.Errorf("decoding %q: %w", jsonFilePath, err)
	}

	return &statuses, nil
}

func (m Manager) SetLastReportedStatuses(statuses *protobufs.PackageStatuses) error {
	jsonFilePath := filepath.Join(m.Root, statusesJson)

	jsonFile, err := os.Create(jsonFilePath)
	if err != nil {
		return fmt.Errorf("creating %q: %w", jsonFilePath, err)
	}

	defer func() {
		_ = jsonFile.Close()
	}()

	err = json.NewEncoder(jsonFile).Encode(statuses)
	if err != nil {
		return fmt.Errorf("encoding statuses to %q: %w", jsonFilePath, err)
	}

	return nil
}

// readHashFile reads a hex-encoded string from a specified path, and returns its decoded contents.
// readHashFile return (nil, nil) if the file is empty or does not exist.
func readHashFile(path string) ([]byte, error) {
	hexHash, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}

	if err != nil {
		return nil, fmt.Errorf("reading file: %w", err)
	}

	if len(hexHash) == 0 {
		return nil, nil
	}

	rawHash := make([]byte, hex.DecodedLen(len(hexHash)))
	_, err = hex.Decode(rawHash, hexHash)
	if err != nil {
		return nil, fmt.Errorf("malformed hash: %w", err)
	}

	return rawHash, nil
}

func writeHashFile(path string, rawHash []byte) error {
	file, err := os.Create(path)
	if err != nil {
		return fmt.Errorf("creating hash file %q: %w", path, err)
	}

	defer func() {
		_ = file.Close()
	}()

	_, err = hex.NewEncoder(file).Write(rawHash)
	if err != nil {
		return fmt.Errorf("writing hash file %q: %w", path, err)
	}

	return nil
}
