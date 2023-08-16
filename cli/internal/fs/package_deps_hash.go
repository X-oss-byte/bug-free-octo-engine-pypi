package fs

import (
	"bufio"
	"fmt"
	"io"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"

	"github.com/vercel/turborepo/cli/internal/encoding/gitoutput"
	"github.com/vercel/turborepo/cli/internal/turbopath"
	"github.com/vercel/turborepo/cli/internal/util"
)

// PackageDepsOptions are parameters for getting git hashes for a filesystem
type PackageDepsOptions struct {
	// PackagePath is the folder path to derive the package dependencies from. This is typically the folder
	// containing package.json. If omitted, the default value is the current working directory.
	PackagePath string

	InputPatterns []string
}

// GetPackageDeps Builds an object containing git hashes for the files under the specified `packagePath` folder.
func GetPackageDeps(rootPath AbsolutePath, p *PackageDepsOptions) (map[turbopath.AnchoredUnixPath]string, error) {
	// Add all the checked in hashes.
	var result map[turbopath.AnchoredUnixPath]string
	if len(p.InputPatterns) == 0 {
		gitLsTreeOutput, err := gitLsTree(rootPath.Join(p.PackagePath))
		if err != nil {
			return nil, fmt.Errorf("could not get git hashes for files in package %s: %w", p.PackagePath, err)
		}
		result = gitLsTreeOutput
	} else {
		gitLsFilesOutput, err := gitLsFiles(rootPath.Join(p.PackagePath), p.InputPatterns)
		if err != nil {
			return nil, fmt.Errorf("could not get git hashes for file patterns %v in package %s: %w", p.InputPatterns, p.PackagePath, err)
		}
		result = gitLsFilesOutput
	}

	// Update the checked in hashes with the current repo status
	gitStatusOutput, err := gitStatus(rootPath.Join(p.PackagePath), p.InputPatterns)
	if err != nil {
		return nil, fmt.Errorf("Could not get git hashes from git status: %v", err)
	}

	var filesToHash []turbopath.AnchoredUnixPath
	for filePath, status := range gitStatusOutput {
		if status.isDelete() {
			delete(result, filePath)
		} else {
			filesToHash = append(filesToHash, filePath)
		}
	}

	convertedRootPath := turbopath.AbsoluteSystemPathFromUpstream(rootPath.ToString())
	hashes, err := gitHashObject(convertedRootPath, turbopath.AnchoredUnixPathArray(filesToHash).ToSystemPathArray())
	if err != nil {
		return nil, err
	}

	// Zip up file paths and hashes together
	for filePath, hash := range hashes {
		result[filePath] = hash
	}

	return result, nil
}

// GetHashableDeps hashes the list of given files, then returns a map of normalized path to hash
// this map is suitable for cross-platform caching.
func GetHashableDeps(rootPath AbsolutePath, files []turbopath.AbsoluteSystemPath) (map[turbopath.AnchoredUnixPath]string, error) {
	output := make([]turbopath.AnchoredSystemPath, len(files))
	convertedRootPath := turbopath.AbsoluteSystemPathFromUpstream(rootPath.ToString())

	for index, file := range files {
		anchoredSystemPath, err := file.RelativeTo(convertedRootPath)
		if err != nil {
			return nil, err
		}
		output[index] = anchoredSystemPath
	}
	return gitHashObject(convertedRootPath, output)
}

// gitHashObject returns a map of paths to their SHA hashes calculated by passing the paths `git hash-object`.
// `git hash-object` expects paths to use Unix separators, even on Windows.
//
// Note: paths of files to hash passed to `git hash-object` are processed as relative to the *repository* root.
// For that reason we convert all input paths and make them relative to the rootPath prior to passing them
// to `git hash-object`.
func gitHashObject(rootPath turbopath.AbsoluteSystemPath, filesToHash []turbopath.AnchoredSystemPath) (map[turbopath.AnchoredUnixPath]string, error) {
	fileCount := len(filesToHash)
	output := make(map[turbopath.AnchoredUnixPath]string, fileCount)

	if fileCount > 0 {
		cmd := exec.Command(
			"git",           // Using `git` from $PATH,
			"hash-object",   // hash a file,
			"--stdin-paths", // using a list of newline-separated paths from stdin.
		)
		cmd.Dir = rootPath.ToString() // Start at this directory.

		// The functionality for gitHashObject is different enough that it isn't reasonable to
		// generalize the behavior for `runGitCmd`. In fact, it doesn't even use the `gitoutput`
		// encoding library, instead relying on its own separate `bufio.Scanner`.

		// We're going to send the list of files in via `stdin`, so we grab that pipe.
		// This prevents a huge number of encoding issues and shell compatibility issues
		// before they even start.
		stdinPipe, stdinPipeError := cmd.StdinPipe()
		if stdinPipeError != nil {
			return nil, stdinPipeError
		}

		// Kick the processing off in a goroutine so while that is doing its thing we can go ahead
		// and wire up the consumer of `stdout`.
		go func() {
			defer util.CloseAndIgnoreError(stdinPipe)

			// `git hash-object` understands all relative paths to be relative to the repository.
			// This function's result needs to be relative to `rootPath`.
			// We convert all files to absolute paths and assume that they will be inside of the repository.
			for _, file := range filesToHash {
				converted := file.RestoreAnchor(rootPath)

				// `git hash-object` expects paths to use Unix separators, even on Windows.
				// `git hash-object` expects paths to be one per line so we must escape newlines.
				// In order to understand the escapes, the path must be quoted.
				// In order to quote the path, the quotes in the path must be escaped.
				// Other than that, we just write everything with full Unicode.
				stringPath := converted.ToString()
				toSlashed := filepath.ToSlash(stringPath)
				escapedNewLines := strings.ReplaceAll(toSlashed, "\n", "\\n")
				escapedQuotes := strings.ReplaceAll(escapedNewLines, "\"", "\\\"")
				prepared := fmt.Sprintf("\"%s\"\n", escapedQuotes)
				_, err := io.WriteString(stdinPipe, prepared)
				if err != nil {
					return
				}
			}
		}()

		// This gives us an io.ReadCloser so that we never have to read the entire input in
		// at a single time. It is doing stream processing instead of string processing.
		stdoutPipe, stdoutPipeError := cmd.StdoutPipe()
		if stdoutPipeError != nil {
			return nil, fmt.Errorf("failed to read `git hash-object`: %w", stdoutPipeError)
		}

		startError := cmd.Start()
		if startError != nil {
			return nil, fmt.Errorf("failed to read `git hash-object`: %w", startError)
		}

		// The output of `git hash-object` is a 40-character SHA per input, then a newline.
		// We need to track the SHA that corresponds to the input file path.
		index := 0
		hashes := make([]string, len(filesToHash))
		scanner := bufio.NewScanner(stdoutPipe)

		// Read the output line-by-line (which is our separator) until exhausted.
		for scanner.Scan() {
			bytes := scanner.Bytes()

			scanError := scanner.Err()
			if scanError != nil {
				return nil, fmt.Errorf("failed to read `git hash-object`: %w", scanError)
			}

			hashError := gitoutput.CheckObjectName(bytes)
			if hashError != nil {
				return nil, fmt.Errorf("failed to read `git hash-object`: %s", "invalid hash received")
			}

			// Worked, save it off.
			hashes[index] = string(bytes)
			index++
		}

		// Waits until stdout is closed before proceeding.
		waitErr := cmd.Wait()
		if waitErr != nil {
			return nil, fmt.Errorf("failed to read `git hash-object`: %w", waitErr)
		}

		// Make sure we end up with a matching number of files and hashes.
		hashCount := len(hashes)
		if fileCount != hashCount {
			return nil, fmt.Errorf("failed to read `git hash-object`: %d files %d hashes", fileCount, hashCount)
		}

		// The API of this method specifies that we return a `map[turbopath.AnchoredUnixPath]string`.
		for i, hash := range hashes {
			filePath := filesToHash[i]
			output[filePath.ToUnixPath()] = hash
		}
	}

	return output, nil
}

// runGitCommand provides boilerplate command handling for `ls-tree`, `ls-files`, and `status`
// Rather than doing string processing, it does stream processing of `stdout`.
func runGitCommand(cmd *exec.Cmd, commandName string, handler func(io.Reader) *gitoutput.Reader) ([][]string, error) {
	stdoutPipe, pipeError := cmd.StdoutPipe()
	if pipeError != nil {
		return nil, fmt.Errorf("failed to read `git %s`: %w", commandName, pipeError)
	}

	startError := cmd.Start()
	if startError != nil {
		return nil, fmt.Errorf("failed to read `git %s`: %w", commandName, startError)
	}

	reader := handler(stdoutPipe)
	entries, readErr := reader.ReadAll()
	if readErr != nil {
		return nil, fmt.Errorf("failed to read `git %s`: %w", commandName, readErr)
	}

	waitErr := cmd.Wait()
	if waitErr != nil {
		return nil, fmt.Errorf("failed to read `git %s`: %w", commandName, waitErr)
	}

	return entries, nil
}

// gitLsTree returns a map of paths to their SHA hashes starting at a particular directory
// that are present in the `git` index at a particular revision.
func gitLsTree(rootPath AbsolutePath) (map[turbopath.AnchoredUnixPath]string, error) {
	cmd := exec.Command(
		"git",     // Using `git` from $PATH,
		"ls-tree", // list the contents of the git index,
		"-r",      // recursively,
		"-z",      // with each file path relative to the invocation directory and \000-terminated,
		"HEAD",    // at this specified version.
	)
	cmd.Dir = rootPath.ToString() // Include files only from this directory.

	entries, err := runGitCommand(cmd, "ls-tree", gitoutput.NewLSTreeReader)
	if err != nil {
		return nil, err
	}

	output := make(map[turbopath.AnchoredUnixPath]string, len(entries))

	for _, entry := range entries {
		lsTreeEntry := gitoutput.LsTreeEntry(entry)
		output[turbopath.AnchoredUnixPathFromUpstream(lsTreeEntry.GetField(gitoutput.Path))] = lsTreeEntry[2]
	}

	return output, nil
}

// gitLsTree returns a map of paths to their SHA hashes starting from a list of patterns relative to a directory
// that are present in the `git` index at a particular revision.
func gitLsFiles(rootPath AbsolutePath, patterns []string) (map[turbopath.AnchoredUnixPath]string, error) {
	cmd := exec.Command(
		"git",      // Using `git` from $PATH,
		"ls-files", // tell me about git index information of some files,
		"--stage",  // including information about the state of the object so that we can get the hashes,
		"-z",       // with each file path relative to the invocation directory and \000-terminated,
		"--",       // and any additional argument you see is a path, promise.
	)

	// FIXME: Globbing is using `git`'s globbing rules which are not consistent with `doublestar``.
	cmd.Args = append(cmd.Args, patterns...) // Pass in input patterns as arguments.
	cmd.Dir = rootPath.ToString()            // Include files only from this directory.

	entries, err := runGitCommand(cmd, "ls-files", gitoutput.NewLSFilesReader)
	if err != nil {
		return nil, err
	}

	output := make(map[turbopath.AnchoredUnixPath]string, len(entries))

	for _, entry := range entries {
		lsFilesEntry := gitoutput.LsFilesEntry(entry)
		output[turbopath.AnchoredUnixPathFromUpstream(lsFilesEntry.GetField(gitoutput.Path))] = lsFilesEntry.GetField(gitoutput.ObjectName)
	}

	return output, nil
}

// getTraversePath gets the distance of the current working directory to the repository root.
// This is used to convert repo-relative paths to cwd-relative paths.
//
// `git rev-parse --show-cdup` always returns Unix paths, even on Windows.
func getTraversePath(rootPath turbopath.AbsoluteSystemPath) (turbopath.RelativeUnixPath, error) {
	cmd := exec.Command("git", "rev-parse", "--show-cdup")
	cmd.Dir = rootPath.ToString()

	traversePath, err := cmd.Output()
	if err != nil {
		return "", err
	}

	trimmedTraversePath := strings.TrimSuffix(string(traversePath), "\n")

	return turbopath.RelativeUnixPathFromUpstream(trimmedTraversePath), nil
}

// Don't shell out if we already know where you are in the repository.
// `memoize` is a good candidate for generics.
func memoizeGetTraversePath() func(turbopath.AbsoluteSystemPath) (turbopath.RelativeUnixPath, error) {
	cacheMutex := &sync.RWMutex{}
	cachedResult := map[turbopath.AbsoluteSystemPath]turbopath.RelativeUnixPath{}
	cachedError := map[turbopath.AbsoluteSystemPath]error{}

	return func(rootPath turbopath.AbsoluteSystemPath) (turbopath.RelativeUnixPath, error) {
		cacheMutex.RLock()
		result, resultExists := cachedResult[rootPath]
		err, errExists := cachedError[rootPath]
		cacheMutex.RUnlock()

		if resultExists && errExists {
			return result, err
		}

		invokedResult, invokedErr := getTraversePath(rootPath)
		cacheMutex.Lock()
		cachedResult[rootPath] = invokedResult
		cachedError[rootPath] = invokedErr
		cacheMutex.Unlock()

		return invokedResult, invokedErr
	}
}

var memoizedGetTraversePath = memoizeGetTraversePath()

// statusCode represents the two-letter status code from `git status` with two "named" fields, x & y.
// They have different meanings based upon the actual state of the working tree. Using x & y maps
// to upstream behavior.
type statusCode struct {
	x string
	y string
}

func (s statusCode) isDelete() bool {
	return s.x == "D" || s.y == "D"
}

// gitStatus returns a map of paths to their `git` status code. This can be used to identify what should
// be done with files that do not currently match what is in the index.
//
// Note: `git status -z`'s relative path results are relative to the repository's location.
// We need to calculate where the repository's location is in order to determine what the full path is
// before we can return those paths relative to the calling directory, normalizing to the behavior of
// `ls-files` and `ls-tree`.
func gitStatus(rootPath AbsolutePath, patterns []string) (map[turbopath.AnchoredUnixPath]statusCode, error) {
	cmd := exec.Command(
		"git",               // Using `git` from $PATH,
		"status",            // tell me about the status of the working tree,
		"--untracked-files", // including information about untracked files,
		"--no-renames",      // do not detect renames,
		"-z",                // with each file path relative to the repository root and \000-terminated,
		"--",                // and any additional argument you see is a path, promise.
	)
	if len(patterns) == 0 {
		cmd.Args = append(cmd.Args, ".") // Operate in the current directory instead of the root of the working tree.
	} else {
		// FIXME: Globbing is using `git`'s globbing rules which are not consistent with `doublestar``.
		cmd.Args = append(cmd.Args, patterns...) // Pass in input patterns as arguments.
	}
	cmd.Dir = rootPath.ToString() // Include files only from this directory.

	entries, err := runGitCommand(cmd, "status", gitoutput.NewStatusReader)
	if err != nil {
		return nil, err
	}

	output := make(map[turbopath.AnchoredUnixPath]statusCode, len(entries))
	convertedRootPath := turbopath.AbsoluteSystemPathFromUpstream(rootPath.ToString())

	traversePath, err := memoizedGetTraversePath(convertedRootPath)
	if err != nil {
		return nil, err
	}

	for _, entry := range entries {
		statusEntry := gitoutput.StatusEntry(entry)
		// Anchored at repository.
		pathFromStatus := turbopath.AnchoredUnixPathFromUpstream(statusEntry.GetField(gitoutput.Path))
		var outputPath turbopath.AnchoredUnixPath

		if len(traversePath) > 0 {
			repositoryPath := convertedRootPath.Join(traversePath.ToSystemPath())
			fileFullPath := pathFromStatus.ToSystemPath().RestoreAnchor(repositoryPath)

			relativePath, err := fileFullPath.RelativeTo(convertedRootPath)
			if err != nil {
				return nil, err
			}

			outputPath = relativePath.ToUnixPath()
		} else {
			outputPath = pathFromStatus
		}

		output[outputPath] = statusCode{x: statusEntry.GetField(gitoutput.StatusX), y: statusEntry.GetField(gitoutput.StatusY)}
	}

	return output, nil
}
