package run

import (
	"fmt"
	"path/filepath"

	"github.com/hashicorp/go-hclog"
	"github.com/vercel/turbo/cli/internal/env"
	"github.com/vercel/turbo/cli/internal/fs"
	"github.com/vercel/turbo/cli/internal/globby"
	"github.com/vercel/turbo/cli/internal/hashing"
	"github.com/vercel/turbo/cli/internal/lockfile"
	"github.com/vercel/turbo/cli/internal/packagemanager"
	"github.com/vercel/turbo/cli/internal/turbopath"
	"github.com/vercel/turbo/cli/internal/util"
)

const _globalCacheKey = "You don't understand! I coulda had class. I coulda been a contender. I could've been somebody, instead of a bum, which is what I am."

// Variables that we always include
var _defaultEnvVars = []string{
	"VERCEL_ANALYTICS_ID",
}

// GlobalHashableInputs represents all the things that we use to create the global hash
type GlobalHashableInputs struct {
	globalFileHashMap    map[turbopath.AnchoredUnixPath]string
	rootExternalDepsHash string
	envVars              env.DetailedMap
	globalCacheKey       string
	envVarPassthroughs   []string
	envMode              util.EnvMode
	frameworkInference   bool
}

type globalHashable struct {
	globalFileHashMap    map[turbopath.AnchoredUnixPath]string
	rootExternalDepsHash string
	envVars              env.EnvironmentVariablePairs
	globalCacheKey       string
	envVarPassthroughs   []string
	envMode              util.EnvMode
	frameworkInference   bool
}

// calculateGlobalHash is a transformation of GlobalHashableInputs.
// It's used for the situations where we have an `EnvMode` specified
// as that is not compatible with existing global hashes.
func calculateGlobalHash(full GlobalHashableInputs) (string, error) {
	return fs.HashObject(globalHashable{
		globalFileHashMap:    full.globalFileHashMap,
		rootExternalDepsHash: full.rootExternalDepsHash,
		envVars:              full.envVars.All.ToHashable(),
		globalCacheKey:       full.globalCacheKey,
		envVarPassthroughs:   full.envVarPassthroughs,
		envMode:              full.envMode,
		frameworkInference:   full.frameworkInference,
	})
}

// calculateGlobalHashFromHashableInputs returns a hash string from the GlobalHashableInputs
func calculateGlobalHashFromHashableInputs(full GlobalHashableInputs) (string, error) {
	switch full.envMode {
	case util.Infer:
		if full.envVarPassthroughs != nil {
			// In infer mode, if there is any passThru config (even if it is an empty array)
			// we'll hash the whole object, so we can detect changes to that config
			// Further, resolve the envMode to the concrete value.
			full.envMode = util.Strict
		}

		return calculateGlobalHash(full)
	case util.Loose:
		// Remove the passthroughs from hash consideration if we're explicitly loose.
		full.envVarPassthroughs = nil
		return calculateGlobalHash(full)
	case util.Strict:
		// Collapse `nil` and `[]` in strict mode.
		if full.envVarPassthroughs == nil {
			full.envVarPassthroughs = make([]string, 0)
		}
		return calculateGlobalHash(full)
	default:
		panic("unimplemented environment mode")
	}
}

func getGlobalHashInputs(
	rootpath turbopath.AbsoluteSystemPath,
	rootPackageJSON *fs.PackageJSON,
	envVarDependencies []string,
	globalFileDependencies []string,
	packageManager *packagemanager.PackageManager,
	lockFile lockfile.Lockfile,
	envVarPassthroughs []string,
	envMode util.EnvMode,
	frameworkInference bool,
	logger hclog.Logger,
) (GlobalHashableInputs, error) {
	// Calculate env var dependencies
	envVars := []string{}
	envVars = append(envVars, envVarDependencies...)
	envVars = append(envVars, _defaultEnvVars...)
	globalHashableEnvVars, err := env.GetHashableEnvVars(envVars, nil, "")
	if err != nil {
		return GlobalHashableInputs{}, err
	}

	logger.Debug("global hash env vars", "vars", globalHashableEnvVars.All.Names())

	// Calculate global file dependencies
	globalDeps := make(util.Set)
	if len(globalFileDependencies) > 0 {
		ignores, err := packageManager.GetWorkspaceIgnores(rootpath)
		if err != nil {
			return GlobalHashableInputs{}, err
		}

		f, err := globby.GlobFiles(rootpath.ToStringDuringMigration(), globalFileDependencies, ignores)
		if err != nil {
			return GlobalHashableInputs{}, err
		}

		for _, val := range f {
			globalDeps.Add(val)
		}
	}

	if lockFile == nil {
		// If we don't have lockfile information available, add the specfile and lockfile to global deps
		globalDeps.Add(filepath.Join(rootpath.ToStringDuringMigration(), packageManager.Specfile))
		if rootpath.UntypedJoin(packageManager.Lockfile).Exists() {
			globalDeps.Add(filepath.Join(rootpath.ToStringDuringMigration(), packageManager.Lockfile))
		}
	}

	// No prefix, global deps already have full paths
	globalDepsArray := globalDeps.UnsafeListOfStrings()
	globalDepsPaths := make([]turbopath.AnchoredSystemPath, len(globalDepsArray))
	for i, path := range globalDepsArray {
		fullyQualifiedPath := turbopath.AbsoluteSystemPathFromUpstream(path)
		anchoredPath, err := fullyQualifiedPath.RelativeTo(rootpath)
		if err != nil {
			return GlobalHashableInputs{}, err
		}

		globalDepsPaths[i] = anchoredPath
	}

	globalFileHashMap, err := hashing.GetHashesForFiles(rootpath, globalDepsPaths)
	if err != nil {
		return GlobalHashableInputs{}, fmt.Errorf("error hashing files: %w", err)
	}

	return GlobalHashableInputs{
		globalFileHashMap:    globalFileHashMap,
		rootExternalDepsHash: rootPackageJSON.ExternalDepsHash,
		envVars:              globalHashableEnvVars,
		globalCacheKey:       _globalCacheKey,
		envVarPassthroughs:   envVarPassthroughs,
		envMode:              envMode,
		frameworkInference:   frameworkInference,
	}, nil
}
