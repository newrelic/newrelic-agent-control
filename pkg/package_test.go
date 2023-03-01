package pkg_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/google/go-cmp/cmp"
	"github.com/google/go-cmp/cmp/cmpopts"
	"github.com/newrelic/supervisor/pkg"
	"github.com/open-telemetry/opamp-go/client/types"
	"github.com/open-telemetry/opamp-go/protobufs"
)

func TestManager_AllPackagesHash(t *testing.T) {
	t.Parallel()

	t.Run("Existing_File", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()

		err := os.WriteFile(filepath.Join(tDir, "_all.hash"), []byte("44e0a6799874aa5258fec7ad170e26ec"), 0666)
		if err != nil {
			t.Fatalf("writing test file: %v", err)
		}

		pacman := pkg.Manager{Root: tDir}
		hash, err := pacman.AllPackagesHash()
		if err != nil {
			t.Fatalf("pacman returned error: %v", err)
		}

		if !bytes.Equal(b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="), hash) {
			t.Fatalf("Returned has is not as expected")
		}
	})

	t.Run("Empty_File", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()

		pacman := pkg.Manager{Root: tDir}
		hash, err := pacman.AllPackagesHash()
		if err != nil {
			t.Fatalf("should have returned a nil error, got %v", err)
		}

		if hash != nil {
			t.Fatalf("should have returned a nil hash, got %v", hash)

		}
	})

	t.Run("Remembers_Set_Hash", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}

		err := pacman.SetAllPackagesHash(b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="))
		if err != nil {
			t.Fatalf("error saving packages hash: %v", err)
		}

		hash, err := pacman.AllPackagesHash()
		if err != nil {
			t.Fatalf("error retrieving packages hash: %v", err)
		}

		if !bytes.Equal(hash, b64MustDecode("ROCmeZh0qlJY/setFw4m7A==")) {
			t.Fatalf("retrieved hash is not the stored hash")
		}
	})
}

func TestManager_Packages(t *testing.T) {
	t.Parallel()

	t.Run("Lists_Folders", func(t *testing.T) {
		t.Parallel()

		// List of packages should be just a list of the folders in the root dir.
		tDir := t.TempDir()
		_ = os.MkdirAll(filepath.Join(tDir, "1one"), 0777)
		_ = os.MkdirAll(filepath.Join(tDir, "2two"), 0777)
		_ = os.MkdirAll(filepath.Join(tDir, "3three"), 0777)

		_ = os.WriteFile(filepath.Join(tDir, "extraneous"), []byte("file"), 0666)

		pacman := pkg.Manager{Root: tDir}
		packages, err := pacman.Packages()
		if err != nil {
			t.Fatalf("listing packages: %v", err)
		}

		if diff := cmp.Diff([]string{"1one", "2two", "3three"}, packages); diff != "" {
			t.Fatalf("package list is not as expected:\n%s", diff)
		}
	})

	t.Run("Lists_Created_Packages", func(t *testing.T) {
		t.Parallel()

		// List of packages should be just a list of the folders in the root dir.
		tDir := t.TempDir()

		pacman := pkg.Manager{Root: tDir}
		_ = pacman.CreatePackage("1one", 0)
		_ = pacman.CreatePackage("2two", 0)
		_ = pacman.CreatePackage("3three", 0)

		packages, err := pacman.Packages()
		if err != nil {
			t.Fatalf("listing packages: %v", err)
		}

		if diff := cmp.Diff([]string{"1one", "2two", "3three"}, packages); diff != "" {
			t.Fatalf("package list is not as expected:\n%s", diff)
		}
	})
}

func TestManager_CreatePackage_Errors(t *testing.T) {
	t.Parallel()

	t.Run("With_Unsupported_Type", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}
		err := pacman.CreatePackage("illegal", protobufs.PackageType_PackageType_Addon)
		if !errors.Is(err, pkg.ErrUnimplementedType) {
			t.Fatalf("expected %v for unimplemented package, got %v", pkg.ErrUnimplementedType, err)
		}
	})

	t.Run("With_Invalid_Name", func(t *testing.T) {
		t.Parallel()

		for _, name := range []string{"_all.hash", "package.hash", "package.version"} {
			name := name
			t.Run(name, func(t *testing.T) {
				t.Parallel()

				tDir := t.TempDir()
				pacman := pkg.Manager{Root: tDir}

				err := pacman.CreatePackage(name, 0)
				if !errors.Is(err, pkg.ErrIllegalName) {
					t.Fatalf("expected error for illegal name %q", name)
				}
			})
		}
	})
}

func TestManager_PackageState(t *testing.T) {
	t.Parallel()

	t.Run("Missing_Package", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}
		state, err := pacman.PackageState("myPackage")
		if err != nil {
			t.Fatalf("expected nil error for missing package, got: %v", err)
		}

		if state.Exists != false {
			t.Fatalf("expected state.Exists == fale for missing package")
		}
	})

	t.Run("Manually_Created_Package", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()

		pkgDir := filepath.Join(tDir, "myPackage")
		_ = os.MkdirAll(pkgDir, 0777)
		_ = os.WriteFile(pkgDir+".hash", []byte("44e0a6799874aa5258fec7ad170e26ec"), 0666)
		_ = os.WriteFile(pkgDir+".version", []byte("1.2.3"), 0666)
		_ = os.WriteFile(filepath.Join(pkgDir, "myPackage"), []byte("ignored"), 0666)

		pacman := pkg.Manager{Root: tDir}
		state, err := pacman.PackageState("myPackage")
		if err != nil {
			t.Fatalf("error retrieving package state: %v", err)
		}

		if diff := cmp.Diff(types.PackageState{
			Exists:  true,
			Type:    0,
			Hash:    b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
			Version: "1.2.3",
		}, state); diff != "" {
			t.Fatalf("state does not match expected:\n%s", diff)
		}
	})

	t.Run("Remembers_Set_State", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}

		name := "myPackage"

		err := pacman.CreatePackage(name, 0)
		if err != nil {
			t.Fatalf("creating package: %v", err)
		}

		state := types.PackageState{
			Exists:  true,
			Type:    0,
			Hash:    b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
			Version: "1.2.3",
		}

		err = pacman.SetPackageState(name, state)
		if err != nil {
			t.Fatalf("setting pakcage state: %v", err)
		}

		receivedState, err := pacman.PackageState(name)
		if err != nil {
			t.Fatalf("retrieving package state: %v", err)
		}

		if diff := cmp.Diff(state, receivedState); diff != "" {
			t.Fatalf("received state does not match expected:\n%s", diff)
		}
	})
}

func TestManager_SetPackageState_Errors(t *testing.T) {
	t.Parallel()

	t.Run("On_Mismatching_Type", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}

		err := pacman.CreatePackage("myPackage", 0)
		if err != nil {
			t.Fatalf("creating package: %v", err)
		}

		err = pacman.SetPackageState("myPackage", types.PackageState{
			Exists: true,
			Type:   1,
		})

		if !errors.Is(err, pkg.ErrUnimplementedType) {
			t.Fatalf("expected error when changing package type, got %v", err)
		}
	})

	t.Run("On_Missing_Package", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}

		err := pacman.SetPackageState("myPackage", types.PackageState{
			Exists: true,
			Type:   1,
		})

		if !errors.Is(err, pkg.ErrPackageDoesNotExist) {
			t.Fatalf("expected error when updating a missing package")
		}
	})

	t.Run("On_ExistsFalse", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}

		_ = pacman.CreatePackage("myPackage", 0)
		err := pacman.SetPackageState("myPackage", types.PackageState{
			Exists: false,
		})

		if !errors.Is(err, pkg.ErrExistsFalse) {
			t.Fatalf("expected error when updating a package with Exists: false")
		}
	})
}

func TestManager_DeletePackage(t *testing.T) {
	t.Parallel()

	tDir := t.TempDir()
	pacman := pkg.Manager{Root: tDir}

	err := pacman.CreatePackage("foobar", 0)
	if err != nil {
		t.Fatalf("creating package: %v", err)
	}

	err = pacman.DeletePackage("foobar")
	if err != nil {
		t.Fatalf("deleting package: %v", err)
	}
}

func TestManager_DeletePackage_Errors(t *testing.T) {
	t.Parallel()

	tDir := t.TempDir()
	pacman := pkg.Manager{Root: tDir}

	err := pacman.DeletePackage("foobar")
	if !errors.Is(err, pkg.ErrPackageDoesNotExist) {
		t.Fatalf("expected error deleting non-existin package, got: %v", err)
	}
}

func TestManager_FileContentHash(t *testing.T) {
	t.Parallel()

	t.Run("Returns_Manually_Created_Hash", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()

		_ = os.MkdirAll(filepath.Join(tDir, "myPackage"), 0777)
		hashPath := filepath.Join(tDir, "myPackage", "myPackage.hash")
		_ = os.WriteFile(hashPath, []byte("44e0a6799874aa5258fec7ad170e26ec"), 0666)

		pacman := pkg.Manager{Root: tDir}
		hash, err := pacman.FileContentHash("myPackage")
		if err != nil {
			t.Fatalf("reading hash: %v", err)
		}

		if !bytes.Equal(hash, b64MustDecode("ROCmeZh0qlJY/setFw4m7A==")) {
			t.Fatalf("unexpected hash returned")
		}
	})

	t.Run("Returns_Previously_Created_File", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}
		err := pacman.CreatePackage("myPackage", 0)
		if err != nil {
			t.Fatalf("creating package: %v", err)
		}

		err = pacman.UpdateContent(context.Background(), "myPackage", strings.NewReader("irrelevant"), b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="))
		if err != nil {
			t.Fatalf("creating package file: %v", err)
		}

		hash, err := pacman.FileContentHash("myPackage")
		if err != nil {
			t.Fatalf("retrieving hash for created file: %v", err)
		}

		if !bytes.Equal(hash, b64MustDecode("ROCmeZh0qlJY/setFw4m7A==")) {
			t.Fatalf("unexpected hash returned")
		}
	})
}

func TestManager_UpdateContent_Fails(t *testing.T) {
	t.Parallel()

	t.Run("On_Non_Existing_Package", func(t *testing.T) {
		t.Parallel()

		tDir := t.TempDir()
		pacman := pkg.Manager{Root: tDir}
		err := pacman.UpdateContent(context.Background(), "myPackage", strings.NewReader("irrelevant"), b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="))
		if !errors.Is(err, pkg.ErrPackageDoesNotExist) {
			t.Fatalf("expected error for non-existing package, got: %v", err)
		}
	})
}

func TestManager_LastReportedStatuses(t *testing.T) {
	t.Parallel()

	tDir := t.TempDir()
	pacman := pkg.Manager{Root: tDir}

	statuses := &protobufs.PackageStatuses{
		Packages: map[string]*protobufs.PackageStatus{
			"foo": {
				Name:                 "foo",
				AgentHasVersion:      "0.0.1",
				AgentHasHash:         b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
				ServerOfferedVersion: "0.1.0",
				ServerOfferedHash:    b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
				Status:               0,
				ErrorMessage:         "something something",
			},
		},
		ServerProvidedAllPackagesHash: b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
		ErrorMessage:                  "not an actual error",
	}

	err := pacman.SetLastReportedStatuses(statuses)
	if err != nil {
		t.Fatalf("error storing statuses: %v", err)
	}

	retrieved, err := pacman.LastReportedStatuses()
	if err != nil {
		t.Fatalf("error retriving statuses: %v", err)
	}

	// Ignore unexported fields of protobufs.PackageStatus for comparison.
	// Implementation is backed by json.Marshal, so unexported fields will not be stored or retrieved.
	cmpOpts := []cmp.Option{
		cmpopts.IgnoreUnexported(protobufs.PackageStatuses{}),
		cmpopts.IgnoreUnexported(protobufs.PackageStatus{}),
	}

	if diff := cmp.Diff(statuses, retrieved, cmpOpts...); diff != "" {
		t.Fatalf("retrieved statuses do not match stored:\n%s", diff)
	}

	// Change some bits to ensure we are overwriting stuff
	statuses.Packages = map[string]*protobufs.PackageStatus{
		"bar": {
			Name:                 "bar",
			AgentHasVersion:      "0.0.1",
			AgentHasHash:         b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
			ServerOfferedVersion: "0.1.0",
			ServerOfferedHash:    b64MustDecode("ROCmeZh0qlJY/setFw4m7A=="),
			Status:               0,
			ErrorMessage:         "something something",
		},
	}

	err = pacman.SetLastReportedStatuses(statuses)
	if err != nil {
		t.Fatalf("error storing statuses: %v", err)
	}

	retrieved, err = pacman.LastReportedStatuses()
	if err != nil {
		t.Fatalf("error retriving statuses: %v", err)
	}

	if diff := cmp.Diff(statuses, retrieved, cmpOpts...); diff != "" {
		t.Fatalf("retrieved statuses do not match stored:\n%s", diff)
	}
}

func b64MustDecode(in string) []byte {
	b, err := base64.StdEncoding.DecodeString(in)
	if err != nil {
		panic(err)
	}

	return b
}
