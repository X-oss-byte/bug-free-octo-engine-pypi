//go:build rust
// +build rust

package globby

import (
	"github.com/vercel/turbo/cli/internal/ffi"
	"github.com/vercel/turbo/cli/internal/ffi/proto"

	"github.com/pkg/errors"
)

// GlobAll returns an array of files and folders that match the specified set of glob patterns.
// The returned files and folders are absolute paths, assuming that basePath is an absolute path.
func GlobAll(basePath string, includePatterns []string, excludePatterns []string) ([]string, error) {
	return glob(basePath, includePatterns, excludePatterns, true)
}

// GlobFiles returns an array of files that match the specified set of glob patterns.
// The return files are absolute paths, assuming that basePath is an absolute path.
func GlobFiles(basePath string, includePatterns []string, excludePatterns []string) ([]string, error) {
	return glob(basePath, includePatterns, excludePatterns, false)
}

func glob(basePath string, includePatterns []string, excludePatterns []string, includeDirs bool) ([]string, error) {
	glob := proto.GlobReq{BasePath: basePath, IncludePatterns: includePatterns, ExcludePatterns: excludePatterns, FilesOnly: !includeDirs}
	buffer := ffi.Marshal(glob.ProtoReflect().Interface())
	buffer_out := ffi.Glob(buffer)
	resp := proto.GlobResp{}
	ffi.Unmarshal(buffer_out, resp.ProtoReflect().Interface())

	if files := resp.GetFiles(); files != nil {
		return files.Files, nil
	}

	if err := resp.GetError(); err != "" {
		return nil, errors.New(err)
	}

	return nil, errors.New("glob failed")
}
