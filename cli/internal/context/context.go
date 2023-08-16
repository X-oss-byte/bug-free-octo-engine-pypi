package context

import (
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/hashicorp/go-hclog"
	"github.com/vercel/turborepo/cli/internal/api"
	"github.com/vercel/turborepo/cli/internal/backends"
	"github.com/vercel/turborepo/cli/internal/config"
	"github.com/vercel/turborepo/cli/internal/core"
	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/globby"
	"github.com/vercel/turborepo/cli/internal/util"

	"github.com/Masterminds/semver"
	mapset "github.com/deckarep/golang-set"
	"github.com/pyr-sh/dag"
	gitignore "github.com/sabhiram/go-gitignore"
	"golang.org/x/sync/errgroup"
)

const GLOBAL_CACHE_KEY = "snozzberries"

// Context of the CLI
type Context struct {
	RootPackageInfo  *fs.PackageJSON // TODO(gsoltis): should this be included in PackageInfos?
	PackageInfos     map[interface{}]*fs.PackageJSON
	PackageNames     []string
	TopologicalGraph dag.AcyclicGraph
	RootNode         string
	TurboConfig      *fs.TurboConfigJSON
	GlobalHash       string
	Lockfile         *fs.YarnLockfile
	SCC              [][]dag.Vertex
	Backend          *api.LanguageBackend
	// Used to arbitrate access to the graph. We parallelise most build operations
	// and Go maps aren't natively threadsafe so this is needed.
	mutex sync.Mutex
}

// Option is used to configure context
type Option func(*Context) error

// New initializes run context
func New(opts ...Option) (*Context, error) {
	var m Context
	for _, opt := range opts {
		if err := opt(&m); err != nil {
			return nil, err
		}
	}

	return &m, nil
}

// Splits "npm:^1.2.3" and "github:foo/bar.git" into a protocol part and a version part.
func parseDependencyProtocol(version string) (string, string) {
	parts := strings.Split(version, ":")
	if len(parts) == 1 {
		return "", parts[0]
	}

	return parts[0], strings.Join(parts[1:], ":")
}

func isProtocolExternal(protocol string) bool {
	// The npm protocol for yarn by default still uses the workspace package if the workspace
	// version is in a compatible semver range. See https://github.com/yarnpkg/berry/discussions/4015
	// For now, we will just assume if the npm protocol is being used and the version matches
	// its an internal dependency which matches the existing behavior before this additional
	// logic was added.

	// TODO: extend this to support the `enableTransparentWorkspaces` yarn option
	return protocol != "" && protocol != "npm"
}

func isWorkspaceReference(packageVersion string, dependencyVersion string) bool {
	protocol, dependencyVersion := parseDependencyProtocol(dependencyVersion)

	if protocol == "workspace" {
		// TODO: Since support at the moment is non-existent for workspaces that contain multiple
		// versions of the same package name, just assume its a match and don't check the range
		// for an exact match.
		return true
	} else if isProtocolExternal(protocol) {
		// Other protocols are assumed to be external references ("github:", "link:", "file:" etc)
		return false
	}

	// If we got this far, then we need to check the workspace package version to see it satisfies
	// the dependencies range to determin whether or not its an internal or external dependency.

	constraint, constraintErr := semver.NewConstraint(dependencyVersion)
	pkgVersion, packageVersionErr := semver.NewVersion(packageVersion)
	if constraintErr != nil || packageVersionErr != nil {
		// For backwards compatibility with existing behavior, if we can't parse the version then we
		// treat the dependency as an internal package reference and swallow the error.

		// TODO: some package managers also support tags like "latest". Does extra handling need to be
		// added for this corner-case
		return true
	}

	return constraint.Check(pkgVersion)
}

func WithGraph(rootpath string, config *config.Config) Option {
	return func(c *Context) error {
		c.PackageInfos = make(map[interface{}]*fs.PackageJSON)
		c.RootNode = core.ROOT_NODE_NAME

		packageJSONPath := filepath.Join(rootpath, "package.json")
		rootPackageJSON, err := fs.ReadPackageJSON(packageJSONPath)
		if err != nil {
			return fmt.Errorf("package.json: %w", err)
		}

		// If turbo.json exists, we use that
		// If pkg.Turbo exists, we warn about running the migration
		// Use pkg.Turbo if turbo.json doesn't exist
		// If neither exists, it's a fatal error
		turboJSONPath := filepath.Join(rootpath, "turbo.json")
		if !fs.FileExists(turboJSONPath) {
			if rootPackageJSON.LegacyTurboConfig == nil {
				// TODO: suggestion on how to create one
				return fmt.Errorf("Could not find turbo.json. Follow directions at https://turborepo.org/docs/getting-started to create one")
			} else {
				log.Println("[WARNING] Turbo configuration now lives in \"turbo.json\". Migrate to turbo.json by running \"npx @turbo/codemod create-turbo-config\"")
				c.TurboConfig = rootPackageJSON.LegacyTurboConfig
			}
		} else {
			turbo, err := fs.ReadTurboConfigJSON(turboJSONPath)
			if err != nil {
				return fmt.Errorf("turbo.json: %w", err)
			}
			c.TurboConfig = turbo
			if rootPackageJSON.LegacyTurboConfig != nil {
				log.Println("[WARNING] Ignoring legacy \"turbo\" key in package.json, using turbo.json instead. Consider deleting the \"turbo\" key from package.json")
				rootPackageJSON.LegacyTurboConfig = nil
			}
		}

		if backend, err := backends.GetBackend(rootpath, rootPackageJSON); err != nil {
			return err
		} else {
			c.Backend = backend
		}

		// this should go into the backend abstraction
		if util.IsYarn(c.Backend.Name) {
			lockfile, err := fs.ReadLockfile(rootpath, c.Backend.Name, config.Cache.Dir)
			if err != nil {
				return fmt.Errorf("yarn.lock: %w", err)
			}
			c.Lockfile = lockfile
		}

		if c.resolveWorkspaceRootDeps(rootPackageJSON) != nil {
			return err
		}
		c.RootPackageInfo = rootPackageJSON

		spaces, err := c.Backend.GetWorkspaceGlobs(rootpath)

		if err != nil {
			return fmt.Errorf("could not detect workspaces: %w", err)
		}

		globalHash, err := calculateGlobalHash(rootpath, rootPackageJSON, c.TurboConfig.GlobalDependencies, c.Backend, config.Logger, os.Environ())
		c.GlobalHash = globalHash
		// We will parse all package.json's simultaneously. We use a
		// wait group because we cannot fully populate the graph (the next step)
		// until all parsing is complete
		parseJSONWaitGroup := new(errgroup.Group)
		justJsons := make([]string, 0, len(spaces))
		for _, space := range spaces {
			justJsons = append(justJsons, filepath.Join(space, "package.json"))
		}

		f := globby.GlobFiles(rootpath, justJsons, getWorkspaceIgnores())

		for _, val := range f {
			relativePkgPath, err := filepath.Rel(rootpath, val)
			if err != nil {
				return fmt.Errorf("non-nested package.json path %w", err)
			}
			parseJSONWaitGroup.Go(func() error {
				return c.parsePackageJSON(relativePkgPath)
			})
		}

		if err := parseJSONWaitGroup.Wait(); err != nil {
			return err
		}
		packageDepsHashGroup := new(errgroup.Group)
		populateGraphWaitGroup := new(errgroup.Group)
		for _, pkg := range c.PackageInfos {
			pkg := pkg
			populateGraphWaitGroup.Go(func() error {
				return c.populateTopologicGraphForPackageJson(pkg)
			})
			packageDepsHashGroup.Go(func() error {
				return c.loadPackageDepsHash(pkg)
			})
		}

		if err := populateGraphWaitGroup.Wait(); err != nil {
			return err
		}
		if err := packageDepsHashGroup.Wait(); err != nil {
			return err
		}

		// Only now can we get the SCC (i.e. topological order)
		c.SCC = dag.StronglyConnected(&c.TopologicalGraph.Graph)
		return nil
	}
}

func (c *Context) loadPackageDepsHash(pkg *fs.PackageJSON) error {
	pkg.Mu.Lock()
	defer pkg.Mu.Unlock()
	hashObject, pkgDepsErr := fs.GetPackageDeps(&fs.PackageDepsOptions{
		PackagePath: pkg.Dir,
	})
	if pkgDepsErr != nil {
		hashObject = make(map[string]string)
		// Instead of implementing all gitignore properly, we hack it. We only respect .gitignore in the root and in
		// the directory of a package.
		ignore, err := safeCompileIgnoreFile(".gitignore")
		if err != nil {
			return err
		}

		ignorePkg, err := safeCompileIgnoreFile(filepath.Join(pkg.Dir, ".gitignore"))
		if err != nil {
			return err
		}

		fs.Walk(pkg.Dir, func(name string, isDir bool) error {
			rootMatch := ignore.MatchesPath(name)
			otherMatch := ignorePkg.MatchesPath(name)
			if !rootMatch && !otherMatch {
				if !isDir {
					hash, err := fs.GitLikeHashFile(name)
					if err != nil {
						return fmt.Errorf("could not hash file %v. \n%w", name, err)
					}
					hashObject[strings.TrimPrefix(name, pkg.Dir+"/")] = hash
				}
			}
			return nil
		})

		// ignorefile rules matched files
	}
	hashOfFiles, otherErr := fs.HashObject(hashObject)
	if otherErr != nil {
		return otherErr
	}
	pkg.FilesHash = hashOfFiles
	return nil
}

func (c *Context) resolveWorkspaceRootDeps(rootPackageJSON *fs.PackageJSON) error {
	seen := mapset.NewSet()
	var lockfileWg sync.WaitGroup
	pkg := rootPackageJSON
	depSet := mapset.NewSet()
	pkg.UnresolvedExternalDeps = make(map[string]string)
	for dep, version := range pkg.Dependencies {
		pkg.UnresolvedExternalDeps[dep] = version
	}
	for dep, version := range pkg.DevDependencies {
		pkg.UnresolvedExternalDeps[dep] = version
	}
	for dep, version := range pkg.OptionalDependencies {
		pkg.UnresolvedExternalDeps[dep] = version
	}
	for dep, version := range pkg.PeerDependencies {
		pkg.UnresolvedExternalDeps[dep] = version
	}
	if util.IsYarn(c.Backend.Name) {
		pkg.SubLockfile = make(fs.YarnLockfile)
		c.resolveDepGraph(&lockfileWg, pkg.UnresolvedExternalDeps, depSet, seen, pkg)
		lockfileWg.Wait()
		pkg.ExternalDeps = make([]string, 0, depSet.Cardinality())
		for _, v := range depSet.ToSlice() {
			pkg.ExternalDeps = append(pkg.ExternalDeps, fmt.Sprintf("%v", v))
		}
		sort.Strings(pkg.ExternalDeps)
		hashOfExternalDeps, err := fs.HashObject(pkg.ExternalDeps)
		if err != nil {
			return err
		}
		pkg.ExternalDepsHash = hashOfExternalDeps
	} else {
		pkg.ExternalDeps = []string{}
		pkg.ExternalDepsHash = ""
	}

	return nil
}

func (c *Context) populateTopologicGraphForPackageJson(pkg *fs.PackageJSON) error {
	c.mutex.Lock()
	defer c.mutex.Unlock()
	depMap := make(map[string]string)
	internalDepsSet := make(dag.Set)
	externalUnresolvedDepsSet := make(dag.Set)
	externalDepSet := mapset.NewSet()
	pkg.UnresolvedExternalDeps = make(map[string]string)

	for dep, version := range pkg.Dependencies {
		depMap[dep] = version
	}

	for dep, version := range pkg.DevDependencies {
		depMap[dep] = version
	}

	for dep, version := range pkg.OptionalDependencies {
		depMap[dep] = version
	}

	for dep, version := range pkg.PeerDependencies {
		depMap[dep] = version
	}

	// split out internal vs. external deps
	for depName, depVersion := range depMap {
		if item, ok := c.PackageInfos[depName]; ok && isWorkspaceReference(item.Version, depVersion) {
			internalDepsSet.Add(depName)
			c.TopologicalGraph.Connect(dag.BasicEdge(pkg.Name, depName))
		} else {
			externalUnresolvedDepsSet.Add(depName)
		}
	}

	for _, name := range externalUnresolvedDepsSet.List() {
		name := name.(string)
		if item, ok := pkg.Dependencies[name]; ok {
			pkg.UnresolvedExternalDeps[name] = item
		}

		if item, ok := pkg.DevDependencies[name]; ok {
			pkg.UnresolvedExternalDeps[name] = item
		}

		if item, ok := pkg.OptionalDependencies[name]; ok {
			pkg.UnresolvedExternalDeps[name] = item
		}
	}

	pkg.SubLockfile = make(fs.YarnLockfile)
	seen := mapset.NewSet()
	var lockfileWg sync.WaitGroup
	c.resolveDepGraph(&lockfileWg, pkg.UnresolvedExternalDeps, externalDepSet, seen, pkg)
	lockfileWg.Wait()

	// when there are no internal dependencies, we need to still add these leafs to the graph
	if internalDepsSet.Len() == 0 {
		c.TopologicalGraph.Connect(dag.BasicEdge(pkg.Name, core.ROOT_NODE_NAME))
	}
	pkg.ExternalDeps = make([]string, 0, externalDepSet.Cardinality())
	for _, v := range externalDepSet.ToSlice() {
		pkg.ExternalDeps = append(pkg.ExternalDeps, fmt.Sprintf("%v", v))
	}
	pkg.InternalDeps = make([]string, 0, internalDepsSet.Len())
	for _, v := range internalDepsSet.List() {
		pkg.InternalDeps = append(pkg.InternalDeps, fmt.Sprintf("%v", v))
	}
	sort.Strings(pkg.InternalDeps)
	sort.Strings(pkg.ExternalDeps)
	hashOfExternalDeps, err := fs.HashObject(pkg.ExternalDeps)
	if err != nil {
		return err
	}
	pkg.ExternalDepsHash = hashOfExternalDeps
	return nil
}

func (c *Context) parsePackageJSON(buildFilePath string) error {
	c.mutex.Lock()
	defer c.mutex.Unlock()

	// log.Printf("[TRACE] reading package.json : %+v", buildFilePath)
	if fs.FileExists(buildFilePath) {
		pkg, err := fs.ReadPackageJSON(buildFilePath)
		if err != nil {
			return fmt.Errorf("parsing %s: %w", buildFilePath, err)
		}

		// log.Printf("[TRACE] adding %+v to graph", pkg.Name)
		c.TopologicalGraph.Add(pkg.Name)
		pkg.PackageJSONPath = buildFilePath
		pkg.Dir = filepath.Dir(buildFilePath)
		c.PackageInfos[pkg.Name] = pkg
		c.PackageNames = append(c.PackageNames, pkg.Name)
	}
	return nil
}

func (c *Context) resolveDepGraph(wg *sync.WaitGroup, unresolvedDirectDeps map[string]string, resolvedDepsSet mapset.Set, seen mapset.Set, pkg *fs.PackageJSON) {
	if !util.IsYarn(c.Backend.Name) {
		return
	}
	for directDepName, unresolvedVersion := range unresolvedDirectDeps {
		wg.Add(1)
		go func(directDepName, unresolvedVersion string) {
			defer wg.Done()
			var lockfileKey string
			lockfileKey1 := fmt.Sprintf("%v@%v", directDepName, unresolvedVersion)
			lockfileKey2 := fmt.Sprintf("%v@npm:%v", directDepName, unresolvedVersion)
			if seen.Contains(lockfileKey1) || seen.Contains(lockfileKey2) {
				return
			}

			seen.Add(lockfileKey1)
			seen.Add(lockfileKey2)

			var entry *fs.LockfileEntry
			entry1, ok1 := (*c.Lockfile)[lockfileKey1]
			entry2, ok2 := (*c.Lockfile)[lockfileKey2]
			if !ok1 && !ok2 {
				return
			}
			if ok1 {
				lockfileKey = lockfileKey1
				entry = entry1
			} else {
				lockfileKey = lockfileKey2
				entry = entry2
			}

			pkg.Mu.Lock()
			pkg.SubLockfile[lockfileKey] = entry
			pkg.Mu.Unlock()
			resolvedDepsSet.Add(fmt.Sprintf("%v@%v", directDepName, entry.Version))

			if len(entry.Dependencies) > 0 {
				c.resolveDepGraph(wg, entry.Dependencies, resolvedDepsSet, seen, pkg)
			}
			if len(entry.OptionalDependencies) > 0 {
				c.resolveDepGraph(wg, entry.OptionalDependencies, resolvedDepsSet, seen, pkg)
			}

		}(directDepName, unresolvedVersion)
	}
}

func safeCompileIgnoreFile(filepath string) (*gitignore.GitIgnore, error) {
	if fs.FileExists(filepath) {
		return gitignore.CompileIgnoreFile(filepath)
	}
	// no op
	return gitignore.CompileIgnoreLines([]string{}...), nil
}

func getWorkspaceIgnores() []string {
	return []string{
		"**/node_modules/**/*",
		"**/bower_components/**/*",
		"**/test/**/*",
		"**/tests/**/*",
	}
}

// getHashableTurboEnvVarsFromOs returns a list of environment variables names and
// that are safe to include in the global hash
func getHashableTurboEnvVarsFromOs(env []string) ([]string, []string) {
	var justNames []string
	var pairs []string
	for _, e := range env {
		kv := strings.SplitN(e, "=", 2)
		if strings.Contains(kv[0], "THASH") {
			justNames = append(justNames, kv[0])
			pairs = append(pairs, e)
		}
	}

	return justNames, pairs
}

func calculateGlobalHash(rootpath string, rootPackageJSON *fs.PackageJSON, externalGlobalDependencies []string, backend *api.LanguageBackend, logger hclog.Logger, env []string) (string, error) {
	// Calculate the global hash
	globalDeps := make(util.Set)

	globalHashableEnvNames := []string{}
	globalHashableEnvPairs := []string{}
	// Calculate global file and env var dependencies
	if len(externalGlobalDependencies) > 0 {
		var globs []string
		for _, v := range externalGlobalDependencies {
			if strings.HasPrefix(v, "$") {
				trimmed := strings.TrimPrefix(v, "$")
				globalHashableEnvNames = append(globalHashableEnvNames, trimmed)
				globalHashableEnvPairs = append(globalHashableEnvPairs, fmt.Sprintf("%v=%v", trimmed, os.Getenv(trimmed)))
			} else {
				globs = append(globs, v)
			}
		}

		if len(globs) > 0 {
			f := globby.GlobFiles(rootpath, globs, []string{})
			for _, val := range f {
				globalDeps.Add(val)
			}
		}
	}

	// get system env vars for hashing purposes, these include any variable that includes "TURBO"
	// that is NOT TURBO_TOKEN or TURBO_TEAM or TURBO_BINARY_PATH.
	names, pairs := getHashableTurboEnvVarsFromOs(env)
	globalHashableEnvNames = append(globalHashableEnvNames, names...)
	globalHashableEnvPairs = append(globalHashableEnvPairs, pairs...)
	// sort them for consistent hashing
	sort.Strings(globalHashableEnvNames)
	sort.Strings(globalHashableEnvPairs)
	logger.Debug("global hash env vars", "vars", globalHashableEnvNames)

	if !util.IsYarn(backend.Name) {
		// If we are not in Yarn, add the specfile and lockfile to global deps
		globalDeps.Add(backend.Specfile)
		globalDeps.Add(backend.Lockfile)
	}

	globalFileHashMap, err := fs.GitHashForFiles(globalDeps.UnsafeListOfStrings(), rootpath)
	if err != nil {
		return "", fmt.Errorf("error hashing files. make sure that git has been initialized %w", err)
	}
	globalHashable := struct {
		globalFileHashMap    map[string]string
		rootExternalDepsHash string
		hashedSortedEnvPairs []string
		globalCacheKey       string
	}{
		globalFileHashMap:    globalFileHashMap,
		rootExternalDepsHash: rootPackageJSON.ExternalDepsHash,
		hashedSortedEnvPairs: globalHashableEnvPairs,
		globalCacheKey:       GLOBAL_CACHE_KEY,
	}
	globalHash, err := fs.HashObject(globalHashable)
	if err != nil {
		return "", fmt.Errorf("error hashing global dependencies %w", err)
	}
	return globalHash, nil
}
