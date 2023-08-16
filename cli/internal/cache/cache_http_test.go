package cache

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"errors"
	"net/http"
	"testing"

	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/util"
	"gotest.tools/v3/assert"
)

type errorResp struct {
	err error
}

func (sr *errorResp) PutArtifact(hash string, body []byte, duration int, tag string) error {
	return sr.err
}

func (sr *errorResp) FetchArtifact(hash string) (*http.Response, error) {
	return nil, sr.err
}

func (sr *errorResp) GetTeamID() string {
	return ""
}

func TestRemoteCachingDisabled(t *testing.T) {
	clientErr := &util.CacheDisabledError{
		Status:  util.CachingStatusDisabled,
		Message: "Remote Caching has been disabled for this team. A team owner can enable it here: $URL",
	}
	client := &errorResp{err: clientErr}
	cache := &httpCache{
		client:         client,
		requestLimiter: make(limiter, 20),
	}
	cd := &util.CacheDisabledError{}
	_, _, _, err := cache.Fetch("unused-target", "some-hash", []string{"unused", "outputs"})
	if !errors.As(err, &cd) {
		t.Errorf("cache.Fetch err got %v, want a CacheDisabled error", err)
	}
	if cd.Status != util.CachingStatusDisabled {
		t.Errorf("CacheDisabled.Status got %v, want %v", cd.Status, util.CachingStatusDisabled)
	}
}

func makeValidTar(t *testing.T) *bytes.Buffer {
	// <repoRoot>
	//   my-pkg/
	//     some-file
	//     link-to-extra-file -> ../extra-file
	//     broken-link -> ../../global-dep
	//   extra-file

	t.Helper()
	buf := &bytes.Buffer{}
	gzw := gzip.NewWriter(buf)
	defer func() {
		if err := gzw.Close(); err != nil {
			t.Fatalf("failed to close gzip: %v", err)
		}
	}()
	tw := tar.NewWriter(gzw)
	defer func() {
		if err := tw.Close(); err != nil {
			t.Fatalf("failed to close tar: %v", err)
		}
	}()

	// my-pkg
	h := &tar.Header{
		Name:     "my-pkg/",
		Mode:     int64(0644),
		Typeflag: tar.TypeDir,
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	// my-pkg/some-file
	contents := []byte("some-file-contents")
	h = &tar.Header{
		Name:     "my-pkg/some-file",
		Mode:     int64(0644),
		Typeflag: tar.TypeReg,
		Size:     int64(len(contents)),
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	if _, err := tw.Write(contents); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}
	// my-pkg/link-to-extra-file
	h = &tar.Header{
		Name:     "my-pkg/link-to-extra-file",
		Mode:     int64(0644),
		Typeflag: tar.TypeSymlink,
		Linkname: "../extra-file",
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	// my-pkg/broken-link
	h = &tar.Header{
		Name:     "my-pkg/broken-link",
		Mode:     int64(0644),
		Typeflag: tar.TypeSymlink,
		Linkname: "../../global-dep",
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	// extra-file
	contents = []byte("extra-file-contents")
	h = &tar.Header{
		Name:     "extra-file",
		Mode:     int64(0644),
		Typeflag: tar.TypeReg,
		Size:     int64(len(contents)),
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	if _, err := tw.Write(contents); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}

	return buf
}

func makeInvalidTar(t *testing.T) *bytes.Buffer {
	// contains a single file that traverses out
	// ../some-file

	t.Helper()
	buf := &bytes.Buffer{}
	gzw := gzip.NewWriter(buf)
	defer func() {
		if err := gzw.Close(); err != nil {
			t.Fatalf("failed to close gzip: %v", err)
		}
	}()
	tw := tar.NewWriter(gzw)
	defer func() {
		if err := tw.Close(); err != nil {
			t.Fatalf("failed to close tar: %v", err)
		}
	}()

	// my-pkg/some-file
	contents := []byte("some-file-contents")
	h := &tar.Header{
		Name:     "../some-file",
		Mode:     int64(0644),
		Typeflag: tar.TypeReg,
		Size:     int64(len(contents)),
	}
	if err := tw.WriteHeader(h); err != nil {
		t.Fatalf("failed to write header: %v", err)
	}
	if _, err := tw.Write(contents); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}
	return buf
}

func TestRestoreTar(t *testing.T) {
	root := fs.AbsolutePathFromUpstream(t.TempDir())

	tar := makeValidTar(t)

	expectedFiles := []string{
		"extra-file",
		"my-pkg/",
		"my-pkg/some-file",
		"my-pkg/link-to-extra-file",
		"my-pkg/broken-link",
	}
	files, err := restoreTar(root, tar)
	assert.NilError(t, err, "readTar")

	expectedSet := util.SetFromStrings(expectedFiles)
	gotSet := util.SetFromStrings(files)
	extraFiles := gotSet.Difference(expectedSet)
	if extraFiles.Len() > 0 {
		t.Errorf("got extra files: %v", extraFiles.UnsafeListOfStrings())
	}
	missingFiles := expectedSet.Difference(gotSet)
	if missingFiles.Len() > 0 {
		t.Errorf("missing expected files: %v", missingFiles.UnsafeListOfStrings())
	}

	// Verify file contents
	extraFile := root.Join("extra-file")
	contents, err := extraFile.ReadFile()
	assert.NilError(t, err, "ReadFile")
	assert.DeepEqual(t, contents, []byte("extra-file-contents"))

	someFile := root.Join("my-pkg", "some-file")
	contents, err = someFile.ReadFile()
	assert.NilError(t, err, "ReadFile")
	assert.DeepEqual(t, contents, []byte("some-file-contents"))
}

func TestRestoreInvalidTar(t *testing.T) {
	root := fs.AbsolutePathFromUpstream(t.TempDir())
	expectedContents := []byte("important-data")
	someFile := root.Join("some-file")
	err := someFile.WriteFile(expectedContents, 0644)
	assert.NilError(t, err, "WriteFile")

	tar := makeInvalidTar(t)
	// use a child directory so that blindly untarring will squash the file
	// that we just wrote above.
	repoRoot := root.Join("repo")
	_, err = restoreTar(repoRoot, tar)
	if err == nil {
		t.Error("expected error untarring invalid tar")
	}

	contents, err := someFile.ReadFile()
	assert.NilError(t, err, "ReadFile")
	assert.Equal(t, string(contents), string(expectedContents), "expected to not overwrite file")
}

// Note that testing Put will require mocking the filesystem and is not currently the most
// interesting test. The current implementation directly returns the error from PutArtifact.
// We should still add the test once feasible to avoid future breakage.
