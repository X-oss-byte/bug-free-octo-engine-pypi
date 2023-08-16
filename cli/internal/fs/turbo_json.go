package fs

import (
	"encoding/json"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"sort"
	"strings"

	"github.com/pkg/errors"
	"github.com/vercel/turbo/cli/internal/turbopath"
	"github.com/vercel/turbo/cli/internal/util"
	"muzzammil.xyz/jsonc"
)

const (
	configFile                   = "turbo.json"
	envPipelineDelimiter         = "$"
	topologicalPipelineDelimiter = "^"
)

type rawTurboJSON struct {
	// Global root filesystem dependencies
	GlobalDependencies []string `json:"globalDependencies,omitempty"`
	// Global env
	GlobalEnv []string `json:"globalEnv,omitempty"`
	// Pipeline is a map of Turbo pipeline entries which define the task graph
	// and cache behavior on a per task or per package-task basis.
	Pipeline Pipeline
	// Configuration options when interfacing with the remote cache
	RemoteCacheOptions RemoteCacheOptions `json:"remoteCache,omitempty"`
}

// TurboJSON is the root turborepo configuration
type TurboJSON struct {
	GlobalDeps         []string
	GlobalEnv          []string
	Pipeline           Pipeline
	RemoteCacheOptions RemoteCacheOptions
}

// RemoteCacheOptions is a struct for deserializing .remoteCache of configFile
type RemoteCacheOptions struct {
	TeamID    string `json:"teamId,omitempty"`
	Signature bool   `json:"signature,omitempty"`
}

type rawTask struct {
	Outputs *[]string `json:"outputs,omitempty"`

	Cache      *bool               `json:"cache,omitempty"`
	DependsOn  []string            `json:"dependsOn,omitempty"`
	Inputs     []string            `json:"inputs,omitempty"`
	OutputMode util.TaskOutputMode `json:"outputMode,omitempty"`
	Env        []string            `json:"env,omitempty"`
	Persistent bool                `json:"persistent,omitempty"`
}

// Pipeline is a struct for deserializing .pipeline in configFile
type Pipeline map[string]TaskDefinition

// TaskDefinition is a representation of the configFile pipeline for further computation.
type TaskDefinition struct {
	Outputs     TaskOutputs
	ShouldCache bool

	// This field is custom-marshalled from rawTask.Env and rawTask.DependsOn
	EnvVarDependencies []string

	// TopologicalDependencies are tasks from package dependencies.
	// E.g. "build" is a topological dependency in:
	// dependsOn: ['^build'].
	// This field is custom-marshalled from rawTask.DependsOn
	TopologicalDependencies []string

	// TaskDependencies are anything that is not a topological dependency
	// E.g. both something and //whatever are TaskDependencies in:
	// dependsOn: ['something', '//whatever']
	// This field is custom-marshalled from rawTask.DependsOn
	TaskDependencies []string

	// Inputs indicate the list of files this Task depends on. If any of those files change
	// we can conclude that any cached outputs or logs for this Task should be invalidated.
	Inputs []string

	// OutputMode determins how we should log the output.
	OutputMode util.TaskOutputMode

	// Persistent indicates whether the Task is expected to exit or not
	// Tasks marked Persistent do not exit (e.g. --watch mode or dev servers)
	Persistent bool
}

// LoadTurboConfig loads, or optionally, synthesizes a TurboJSON instance
func LoadTurboConfig(rootPath turbopath.AbsoluteSystemPath, rootPackageJSON *PackageJSON, includeSynthesizedFromRootPackageJSON bool) (*TurboJSON, error) {
	var turboJSON *TurboJSON
	turboFromFiles, err := ReadTurboConfig(rootPath, rootPackageJSON)
	if !includeSynthesizedFromRootPackageJSON && err != nil {
		// There was an error, and we don't have any chance of recovering
		// because we aren't synthesizing anything
		return nil, err
	} else if !includeSynthesizedFromRootPackageJSON {
		// We're not synthesizing anything and there was no error, we're done
		return turboFromFiles, nil
	} else if errors.Is(err, os.ErrNotExist) {
		// turbo.json doesn't exist, but we're going try to synthesize something
		turboJSON = &TurboJSON{
			Pipeline: make(Pipeline),
		}
	} else if err != nil {
		// some other happened, we can't recover
		return nil, err
	} else {
		// we're synthesizing, but we have a starting point
		// Note: this will have to change to support task inference in a monorepo
		// for now, we're going to error on any "root" tasks and turn non-root tasks into root tasks
		pipeline := make(Pipeline)
		for taskID, taskDefinition := range turboFromFiles.Pipeline {
			if util.IsPackageTask(taskID) {
				return nil, fmt.Errorf("Package tasks (<package>#<task>) are not allowed in single-package repositories: found %v", taskID)
			}
			pipeline[util.RootTaskID(taskID)] = taskDefinition
		}
		turboJSON = turboFromFiles
		turboJSON.Pipeline = pipeline
	}

	for scriptName := range rootPackageJSON.Scripts {
		if !turboJSON.Pipeline.HasTask(scriptName) {
			taskName := util.RootTaskID(scriptName)
			turboJSON.Pipeline[taskName] = TaskDefinition{}
		}
	}
	return turboJSON, nil
}

// TaskOutputs represents the patterns for including and excluding files from outputs
type TaskOutputs struct {
	Inclusions []string
	Exclusions []string
}

// Sort contents of task outputs
func (to TaskOutputs) Sort() TaskOutputs {
	var inclusions []string
	var exclusions []string
	copy(inclusions, to.Inclusions)
	copy(exclusions, to.Exclusions)
	sort.Strings(inclusions)
	sort.Strings(exclusions)
	return TaskOutputs{Inclusions: inclusions, Exclusions: exclusions}
}

// ReadTurboConfig toggles between reading from package.json or the configFile to support early adopters.
func ReadTurboConfig(rootPath turbopath.AbsoluteSystemPath, rootPackageJSON *PackageJSON) (*TurboJSON, error) {

	turboJSONPath := rootPath.UntypedJoin(configFile)

	// Check if turbo key in package.json exists
	hasLegacyConfig := rootPackageJSON.LegacyTurboConfig != nil

	// If the configFile exists, use that
	if turboJSONPath.FileExists() {
		turboJSON, err := readTurboJSON(turboJSONPath)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", configFile, err)
		}

		// If pkg.Turbo exists, log a warning and delete it from the representation
		// TODO: turn off this warning eventually
		if hasLegacyConfig {
			log.Printf("[WARNING] Ignoring \"turbo\" key in package.json, using %s instead.", configFile)
			rootPackageJSON.LegacyTurboConfig = nil
		}

		return turboJSON, nil
	}

	// Use pkg.Turbo if the configFile doesn't exist and we want the fallback feature
	// TODO: turn this fallback off eventually
	if hasLegacyConfig {
		log.Printf("[DEPRECATED] \"turbo\" in package.json is deprecated. Migrate to %s by running \"npx @turbo/codemod create-turbo-config\"\n", configFile)
		return rootPackageJSON.LegacyTurboConfig, nil
	}

	// If there's no turbo.json and no turbo key in package.json, return an error.
	return nil, errors.Wrapf(os.ErrNotExist, "Could not find %s. Follow directions at https://turbo.build/repo/docs to create one", configFile)
}

// readTurboJSON reads the configFile in to a struct
func readTurboJSON(path turbopath.AbsoluteSystemPath) (*TurboJSON, error) {
	file, err := path.Open()
	if err != nil {
		return nil, err
	}
	var turboJSON *TurboJSON
	data, err := ioutil.ReadAll(file)
	if err != nil {
		return nil, err
	}

	err = jsonc.Unmarshal(data, &turboJSON)

	if err != nil {
		return nil, err
	}

	return turboJSON, nil
}

// GetTaskDefinition returns a TaskDefinition from a serialized definition in configFile
func (pc Pipeline) GetTaskDefinition(taskID string) (TaskDefinition, bool) {
	if entry, ok := pc[taskID]; ok {
		return entry, true
	}
	_, task := util.GetPackageTaskFromId(taskID)
	entry, ok := pc[task]
	return entry, ok
}

// HasTask returns true if the given task is defined in the pipeline, either directly or
// via a package task (`pkg#task`)
func (pc Pipeline) HasTask(task string) bool {
	for key := range pc {
		if key == task {
			return true
		}
		if util.IsPackageTask(key) {
			_, taskName := util.GetPackageTaskFromId(key)
			if taskName == task {
				return true
			}
		}
	}
	return false
}

// UnmarshalJSON deserializes JSON into a TaskDefinition
func (c *TaskDefinition) UnmarshalJSON(data []byte) error {
	task := rawTask{}
	if err := json.Unmarshal(data, &task); err != nil {
		return err
	}

	var inclusions []string
	var exclusions []string

	if task.Outputs != nil {
		for _, glob := range *task.Outputs {
			if strings.HasPrefix(glob, "!") {
				exclusions = append(exclusions, glob[1:])
			} else {
				inclusions = append(inclusions, glob)
			}
		}
	}

	c.Outputs = TaskOutputs{
		Inclusions: inclusions,
		Exclusions: exclusions,
	}

	sort.Strings(c.Outputs.Inclusions)
	sort.Strings(c.Outputs.Exclusions)
	if task.Cache == nil {
		c.ShouldCache = true
	} else {
		c.ShouldCache = *task.Cache
	}

	envVarDependencies := make(util.Set)

	c.TopologicalDependencies = []string{} // TODO @mehulkar: this should be a set
	c.TaskDependencies = []string{}        // TODO @mehulkar: this should be a set

	for _, dependency := range task.DependsOn {
		if strings.HasPrefix(dependency, envPipelineDelimiter) {
			log.Printf("[DEPRECATED] Declaring an environment variable in \"dependsOn\" is deprecated, found %s. Use the \"env\" key or use `npx @turbo/codemod migrate-env-var-dependencies`.\n", dependency)
			envVarDependencies.Add(strings.TrimPrefix(dependency, envPipelineDelimiter))
		} else if strings.HasPrefix(dependency, topologicalPipelineDelimiter) {
			c.TopologicalDependencies = append(c.TopologicalDependencies, strings.TrimPrefix(dependency, topologicalPipelineDelimiter))
		} else {
			c.TaskDependencies = append(c.TaskDependencies, dependency)
		}
	}
	sort.Strings(c.TaskDependencies)
	sort.Strings(c.TopologicalDependencies)

	// Append env key into EnvVarDependencies
	for _, value := range task.Env {
		if strings.HasPrefix(value, envPipelineDelimiter) {
			// Hard error to help people specify this correctly during migration.
			// TODO: Remove this error after we have run summary.
			return fmt.Errorf("You specified \"%s\" in the \"env\" key. You should not prefix your environment variables with \"$\"", value)
		}

		envVarDependencies.Add(value)
	}

	c.EnvVarDependencies = envVarDependencies.UnsafeListOfStrings()
	sort.Strings(c.EnvVarDependencies)
	// Note that we don't require Inputs to be sorted, we're going to
	// hash the resulting files and sort that instead
	c.Inputs = task.Inputs
	c.OutputMode = task.OutputMode
	c.Persistent = task.Persistent
	return nil
}

// UnmarshalJSON deserializes TurboJSON objects into struct
func (c *TurboJSON) UnmarshalJSON(data []byte) error {
	raw := &rawTurboJSON{}
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}

	envVarDependencies := make(util.Set)
	globalFileDependencies := make(util.Set)

	for _, value := range raw.GlobalEnv {
		if strings.HasPrefix(value, envPipelineDelimiter) {
			// Hard error to help people specify this correctly during migration.
			// TODO: Remove this error after we have run summary.
			return fmt.Errorf("You specified \"%s\" in the \"env\" key. You should not prefix your environment variables with \"%s\"", value, envPipelineDelimiter)
		}

		envVarDependencies.Add(value)
	}

	for _, value := range raw.GlobalDependencies {
		if strings.HasPrefix(value, envPipelineDelimiter) {
			log.Printf("[DEPRECATED] Declaring an environment variable in \"globalDependencies\" is deprecated, found %s. Use the \"globalEnv\" key or use `npx @turbo/codemod migrate-env-var-dependencies`.\n", value)
			envVarDependencies.Add(strings.TrimPrefix(value, envPipelineDelimiter))
		} else {
			globalFileDependencies.Add(value)
		}
	}

	// turn the set into an array and assign to the TurboJSON struct fields.
	c.GlobalEnv = envVarDependencies.UnsafeListOfStrings()
	sort.Strings(c.GlobalEnv)
	c.GlobalDeps = globalFileDependencies.UnsafeListOfStrings()
	sort.Strings(c.GlobalDeps)

	// copy these over, we don't need any changes here.
	c.Pipeline = raw.Pipeline
	c.RemoteCacheOptions = raw.RemoteCacheOptions

	return nil
}
