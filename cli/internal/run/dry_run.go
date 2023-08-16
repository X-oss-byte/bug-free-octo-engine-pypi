// Package run implements `turbo run`
// This file implements the logic for `turbo run --dry`
package run

import (
	gocontext "context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"text/tabwriter"

	"github.com/mitchellh/cli"
	"github.com/pkg/errors"
	"github.com/vercel/turbo/cli/internal/cache"
	"github.com/vercel/turbo/cli/internal/cmdutil"
	"github.com/vercel/turbo/cli/internal/core"
	"github.com/vercel/turbo/cli/internal/fs"
	"github.com/vercel/turbo/cli/internal/graph"
	"github.com/vercel/turbo/cli/internal/nodes"
	"github.com/vercel/turbo/cli/internal/taskhash"
	"github.com/vercel/turbo/cli/internal/turbopath"
	"github.com/vercel/turbo/cli/internal/util"
	"github.com/vercel/turbo/cli/internal/workspace"
)

// missingTaskLabel is printed when a package is missing a definition for a task that is supposed to run
// E.g. if `turbo run build --dry` is run, and package-a doesn't define a `build` script in package.json,
// the DryRunSummary will print this, instead of the script (e.g. `next build`).
const missingTaskLabel = "<NONEXISTENT>"
const missingFrameworkLabel = "<NO FRAMEWORK DETECTED>"

// DryRunSummary contains a summary of the packages and tasks that would run
// if the --dry flag had not been passed
type dryRunSummary struct {
	GlobalHashSummary *globalHashSummary `json:"globalHashSummary"`
	Packages          []string           `json:"packages"`
	Tasks             []taskSummary      `json:"tasks"`
}

type globalHashSummary struct {
	GlobalFileHashMap    map[turbopath.AnchoredUnixPath]string `json:"globalFileHashMap"`
	RootExternalDepsHash string                                `json:"rootExternalDepsHash"`
	GlobalCacheKey       string                                `json:"globalCacheKey"`
	Pipeline             fs.PristinePipeline                   `json:"pipeline"`
}

func newGlobalHashSummary(ghInputs struct {
	globalFileHashMap    map[turbopath.AnchoredUnixPath]string
	rootExternalDepsHash string
	hashedSortedEnvPairs []string
	globalCacheKey       string
	pipeline             fs.PristinePipeline
}) *globalHashSummary {
	// TODO(mehulkar): Add ghInputs.hashedSortedEnvPairs in here, but redact the values
	return &globalHashSummary{
		GlobalFileHashMap:    ghInputs.globalFileHashMap,
		RootExternalDepsHash: ghInputs.rootExternalDepsHash,
		GlobalCacheKey:       ghInputs.globalCacheKey,
		Pipeline:             ghInputs.pipeline,
	}
}

// DryRunSummarySinglePackage is the same as DryRunSummary with some adjustments
// to the internal struct for a single package. It's likely that we can use the
// same struct for Single Package repos in the future.
type singlePackageDryRunSummary struct {
	Tasks []singlePackageTaskSummary `json:"tasks"`
}

// DryRun gets all the info needed from tasks and prints out a summary, but doesn't actually
// execute the task.
func DryRun(
	ctx gocontext.Context,
	g *graph.CompleteGraph,
	rs *runSpec,
	engine *core.Engine,
	taskHashTracker *taskhash.Tracker,
	turboCache cache.Cache,
	base *cmdutil.CmdBase,
	summary *dryRunSummary,
) error {
	defer turboCache.Shutdown()

	dryRunJSON := rs.Opts.runOpts.dryRunJSON
	singlePackage := rs.Opts.runOpts.singlePackage

	taskSummaries, err := executeDryRun(
		ctx,
		engine,
		g,
		taskHashTracker,
		rs,
		base,
		turboCache,
	)

	if err != nil {
		return err
	}

	// Assign the Task Summaries to the main summary
	summary.Tasks = taskSummaries

	// Render the dry run as json
	if dryRunJSON {
		rendered, err := renderDryRunFullJSON(summary, singlePackage)
		if err != nil {
			return err
		}
		base.UI.Output(rendered)
		return nil
	}

	// Render the dry run as text
	if err := displayDryTextRun(base.UI, summary, g.WorkspaceInfos, singlePackage); err != nil {
		return err
	}

	return nil
}

func executeDryRun(ctx gocontext.Context, engine *core.Engine, g *graph.CompleteGraph, taskHashTracker *taskhash.Tracker, rs *runSpec, base *cmdutil.CmdBase, turboCache cache.Cache) ([]taskSummary, error) {
	taskIDs := []taskSummary{}

	dryRunExecFunc := func(ctx gocontext.Context, packageTask *nodes.PackageTask) error {
		hash := packageTask.Hash

		command := missingTaskLabel
		if packageTask.Command != "" {
			command = packageTask.Command
		}

		framework := missingFrameworkLabel
		if taskHashTracker.PackageTaskFramework[packageTask.TaskID] != "" {
			framework = taskHashTracker.PackageTaskFramework[packageTask.TaskID]
		}

		isRootTask := packageTask.PackageName == util.RootPkgName
		if isRootTask && commandLooksLikeTurbo(command) {
			return fmt.Errorf("root task %v (%v) looks like it invokes turbo and might cause a loop", packageTask.Task, command)
		}

		ancestors, err := engine.GetTaskGraphAncestors(packageTask.TaskID)
		if err != nil {
			return err
		}

		descendents, err := engine.GetTaskGraphDescendants(packageTask.TaskID)
		if err != nil {
			return err
		}

		itemStatus, err := turboCache.Exists(hash)
		if err != nil {
			return err
		}

		taskIDs = append(taskIDs, taskSummary{
			TaskID:                 packageTask.TaskID,
			Task:                   packageTask.Task,
			Package:                packageTask.PackageName,
			Dir:                    packageTask.Dir,
			Outputs:                packageTask.Outputs,
			ExcludedOutputs:        packageTask.ExcludedOutputs,
			LogFile:                packageTask.LogFile,
			ResolvedTaskDefinition: packageTask.TaskDefinition,
			Command:                command,
			Framework:              framework,
			ExpandedInputs:         packageTask.ExpandedInputs,

			Hash:         hash,        // TODO(mehulkar): Move this to PackageTask
			CacheState:   itemStatus,  // TODO(mehulkar): Move this to PackageTask
			Dependencies: ancestors,   // TODO(mehulkar): Move this to PackageTask
			Dependents:   descendents, // TODO(mehulkar): Move this to PackageTask
		})

		return nil
	}

	// This setup mirrors a real run. We call engine.execute() with
	// a visitor function and some hardcoded execOpts.
	// Note: we do not currently attempt to parallelize the graph walking
	// (as we do in real execution)
	getArgs := func(taskID string) []string {
		return rs.ArgsForTask(taskID)
	}
	visitorFn := g.GetPackageTaskVisitor(ctx, engine.TaskGraph, getArgs, base.Logger, dryRunExecFunc)
	execOpts := core.EngineExecutionOptions{
		Concurrency: 1,
		Parallel:    false,
	}
	errs := engine.Execute(visitorFn, execOpts)

	if len(errs) > 0 {
		for _, err := range errs {
			base.UI.Error(err.Error())
		}
		return nil, errors.New("errors occurred during dry-run graph traversal")
	}

	return taskIDs, nil
}

func renderDryRunSinglePackageJSON(summary *dryRunSummary) (string, error) {
	singlePackageTasks := make([]singlePackageTaskSummary, len(summary.Tasks))

	for i, ht := range summary.Tasks {
		singlePackageTasks[i] = ht.toSinglePackageTask()
	}

	dryRun := &singlePackageDryRunSummary{singlePackageTasks}

	bytes, err := json.MarshalIndent(dryRun, "", "  ")
	if err != nil {
		return "", errors.Wrap(err, "failed to render JSON")
	}
	return string(bytes), nil
}

func renderDryRunFullJSON(summary *dryRunSummary, singlePackage bool) (string, error) {
	if singlePackage {
		return renderDryRunSinglePackageJSON(summary)
	}

	bytes, err := json.MarshalIndent(summary, "", "  ")
	if err != nil {
		return "", errors.Wrap(err, "failed to render JSON")
	}
	return string(bytes), nil
}

func displayDryTextRun(ui cli.Ui, summary *dryRunSummary, workspaceInfos workspace.Catalog, isSinglePackage bool) error {
	if !isSinglePackage {
		ui.Output("")
		ui.Info(util.Sprintf("${CYAN}${BOLD}Packages in Scope${RESET}"))
		p := tabwriter.NewWriter(os.Stdout, 0, 0, 1, ' ', 0)
		fmt.Fprintln(p, "Name\tPath\t")
		for _, pkg := range summary.Packages {
			fmt.Fprintf(p, "%s\t%s\t\n", pkg, workspaceInfos.PackageJSONs[pkg].Dir)
		}
		if err := p.Flush(); err != nil {
			return err
		}
	}

	fileCount := 0
	for range summary.GlobalHashSummary.GlobalFileHashMap {
		fileCount = fileCount + 1
	}
	w1 := tabwriter.NewWriter(os.Stdout, 0, 0, 1, ' ', 0)
	ui.Output("")
	ui.Info(util.Sprintf("${CYAN}${BOLD}Global Hash Inputs${RESET}"))
	fmt.Fprintln(w1, util.Sprintf("  ${GREY}Global Files\t=\t%d${RESET}", fileCount))
	fmt.Fprintln(w1, util.Sprintf("  ${GREY}External Dependencies Hash\t=\t%s${RESET}", summary.GlobalHashSummary.RootExternalDepsHash))
	fmt.Fprintln(w1, util.Sprintf("  ${GREY}Global Cache Key\t=\t%s${RESET}", summary.GlobalHashSummary.GlobalCacheKey))
	if bytes, err := json.Marshal(summary.GlobalHashSummary.Pipeline); err == nil {
		fmt.Fprintln(w1, util.Sprintf("  ${GREY}Root pipeline\t=\t%s${RESET}", bytes))
	}
	if err := w1.Flush(); err != nil {
		return err
	}

	ui.Output("")
	ui.Info(util.Sprintf("${CYAN}${BOLD}Tasks to Run${RESET}"))

	for _, task := range summary.Tasks {
		taskName := task.TaskID

		if isSinglePackage {
			taskName = util.RootTaskTaskName(taskName)
		}

		ui.Info(util.Sprintf("${BOLD}%s${RESET}", taskName))
		w := tabwriter.NewWriter(os.Stdout, 0, 0, 1, ' ', 0)
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Task\t=\t%s\t${RESET}", task.Task))

		var dependencies []string
		var dependents []string

		if !isSinglePackage {
			fmt.Fprintln(w, util.Sprintf("  ${GREY}Package\t=\t%s\t${RESET}", task.Package))
			dependencies = task.Dependencies
			dependents = task.Dependents
		} else {
			dependencies = make([]string, len(task.Dependencies))
			for i, dependency := range task.Dependencies {
				dependencies[i] = util.StripPackageName(dependency)
			}
			dependents = make([]string, len(task.Dependents))
			for i, dependent := range task.Dependents {
				dependents[i] = util.StripPackageName(dependent)
			}
		}

		fmt.Fprintln(w, util.Sprintf("  ${GREY}Hash\t=\t%s\t${RESET}", task.Hash))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Cached (Local)\t=\t%s\t${RESET}", strconv.FormatBool(task.CacheState.Local)))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Cached (Remote)\t=\t%s\t${RESET}", strconv.FormatBool(task.CacheState.Remote)))

		if !isSinglePackage {
			fmt.Fprintln(w, util.Sprintf("  ${GREY}Directory\t=\t%s\t${RESET}", task.Dir))
		}

		fmt.Fprintln(w, util.Sprintf("  ${GREY}Command\t=\t%s\t${RESET}", task.Command))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Outputs\t=\t%s\t${RESET}", strings.Join(task.Outputs, ", ")))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Log File\t=\t%s\t${RESET}", task.LogFile))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Dependencies\t=\t%s\t${RESET}", strings.Join(dependencies, ", ")))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Dependendents\t=\t%s\t${RESET}", strings.Join(dependents, ", ")))
		fmt.Fprintln(w, util.Sprintf("  ${GREY}Inputs Files Considered\t=\t%d\t${RESET}", len(task.ExpandedInputs)))
		bytes, err := json.Marshal(task.ResolvedTaskDefinition)
		// If there's an error, we can silently ignore it, we don't need to block the entire print.
		if err == nil {
			fmt.Fprintln(w, util.Sprintf("  ${GREY}ResolvedTaskDefinition\t=\t%s\t${RESET}", string(bytes)))
		}

		fmt.Fprintln(w, util.Sprintf("  ${GREY}Framework\t=\t%s\t${RESET}", task.Framework))
		if err := w.Flush(); err != nil {
			return err
		}
	}
	return nil
}

var _isTurbo = regexp.MustCompile(fmt.Sprintf("(?:^|%v|\\s)turbo(?:$|\\s)", regexp.QuoteMeta(string(filepath.Separator))))

func commandLooksLikeTurbo(command string) bool {
	return _isTurbo.MatchString(command)
}

// TODO: put this somewhere else
// TODO(mehulkar): `Outputs` and `ExcludedOutputs` are slightly redundant
// as the information is also available in ResolvedTaskDefinition. We could remove them
// and favor a version of Outputs that is the fully expanded list of files.
type taskSummary struct {
	TaskID                 string                                `json:"taskId"`
	Task                   string                                `json:"task"`
	Package                string                                `json:"package"`
	Hash                   string                                `json:"hash"`
	CacheState             cache.ItemStatus                      `json:"cacheState"`
	Command                string                                `json:"command"`
	Outputs                []string                              `json:"outputs"`
	ExcludedOutputs        []string                              `json:"excludedOutputs"`
	LogFile                string                                `json:"logFile"`
	Dir                    string                                `json:"directory"`
	Dependencies           []string                              `json:"dependencies"`
	Dependents             []string                              `json:"dependents"`
	ResolvedTaskDefinition *fs.TaskDefinition                    `json:"resolvedTaskDefinition"`
	ExpandedInputs         map[turbopath.AnchoredUnixPath]string `json:"expandedInputs"`
	Framework              string                                `json:"framework"`
}

type singlePackageTaskSummary struct {
	Task                   string                                `json:"task"`
	Hash                   string                                `json:"hash"`
	CacheState             cache.ItemStatus                      `json:"cacheState"`
	Command                string                                `json:"command"`
	Outputs                []string                              `json:"outputs"`
	ExcludedOutputs        []string                              `json:"excludedOutputs"`
	LogFile                string                                `json:"logFile"`
	Dependencies           []string                              `json:"dependencies"`
	Dependents             []string                              `json:"dependents"`
	ResolvedTaskDefinition *fs.TaskDefinition                    `json:"resolvedTaskDefinition"`
	ExpandedInputs         map[turbopath.AnchoredUnixPath]string `json:"expandedInputs"`
	Framework              string                                `json:"framework"`
}

func (ht *taskSummary) toSinglePackageTask() singlePackageTaskSummary {
	dependencies := make([]string, len(ht.Dependencies))
	for i, depencency := range ht.Dependencies {
		dependencies[i] = util.StripPackageName(depencency)
	}
	dependents := make([]string, len(ht.Dependents))
	for i, dependent := range ht.Dependents {
		dependents[i] = util.StripPackageName(dependent)
	}

	return singlePackageTaskSummary{
		Task:                   util.RootTaskTaskName(ht.TaskID),
		Hash:                   ht.Hash,
		CacheState:             ht.CacheState,
		Command:                ht.Command,
		Outputs:                ht.Outputs,
		LogFile:                ht.LogFile,
		Dependencies:           dependencies,
		Dependents:             dependents,
		ResolvedTaskDefinition: ht.ResolvedTaskDefinition,
		Framework:              ht.Framework,
		ExpandedInputs:         ht.ExpandedInputs,
	}
}
