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
	"sort"
	"strconv"
	"strings"
	"text/tabwriter"

	"github.com/mitchellh/cli"
	"github.com/pkg/errors"
	"github.com/vercel/turbo/cli/internal/cache"
	"github.com/vercel/turbo/cli/internal/cmdutil"
	"github.com/vercel/turbo/cli/internal/core"
	"github.com/vercel/turbo/cli/internal/graph"
	"github.com/vercel/turbo/cli/internal/nodes"
	"github.com/vercel/turbo/cli/internal/taskhash"
	"github.com/vercel/turbo/cli/internal/util"
)

// DryRunSummary contains a summary of the packages and tasks that would run
// if the --dry flag had not been passed
type dryRunSummary struct {
	Packages []string      `json:"packages"`
	Tasks    []taskSummary `json:"tasks"`
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
	tracker *taskhash.Tracker,
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
		tracker,
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

func executeDryRun(ctx gocontext.Context, engine *core.Engine, g *graph.CompleteGraph, taskHashes *taskhash.Tracker, rs *runSpec, base *cmdutil.CmdBase, turboCache cache.Cache) ([]taskSummary, error) {
	taskIDs := []taskSummary{}

	dryRunExecFunc := func(ctx gocontext.Context, packageTask *nodes.PackageTask) error {
		deps := engine.TaskGraph.DownEdges(packageTask.TaskID)
		passThroughArgs := rs.ArgsForTask(packageTask.Task)
		hash, err := taskHashes.CalculateTaskHash(packageTask, deps, base.Logger, passThroughArgs)
		if err != nil {
			return err
		}

		command, ok := packageTask.Command()
		if !ok {
			command = "<NONEXISTENT>"
		}
		isRootTask := packageTask.PackageName == util.RootPkgName
		if isRootTask && commandLooksLikeTurbo(command) {
			return fmt.Errorf("root task %v (%v) looks like it invokes turbo and might cause a loop", packageTask.Task, command)
		}

		ancestors, err := engine.TaskGraph.Ancestors(packageTask.TaskID)
		if err != nil {
			return err
		}

		stringAncestors := []string{}
		for _, dep := range ancestors {
			// Don't leak out internal ROOT_NODE_NAME nodes, which are just placeholders
			if !strings.Contains(dep.(string), core.ROOT_NODE_NAME) {
				stringAncestors = append(stringAncestors, dep.(string))
			}
		}
		descendents, err := engine.TaskGraph.Descendents(packageTask.TaskID)
		if err != nil {
			return err
		}
		stringDescendents := []string{}
		for _, dep := range descendents {
			// Don't leak out internal ROOT_NODE_NAME nodes, which are just placeholders
			if !strings.Contains(dep.(string), core.ROOT_NODE_NAME) {
				stringDescendents = append(stringDescendents, dep.(string))
			}
		}
		sort.Strings(stringDescendents)

		itemStatus, err := turboCache.Exists(hash)
		if err != nil {
			return err
		}

		taskIDs = append(taskIDs, taskSummary{
			TaskID:          packageTask.TaskID,
			Task:            packageTask.Task,
			Package:         packageTask.PackageName,
			Hash:            hash,
			CacheState:      itemStatus,
			Command:         command,
			Dir:             packageTask.Pkg.Dir.ToString(),
			Outputs:         packageTask.TaskDefinition.Outputs.Inclusions,
			ExcludedOutputs: packageTask.TaskDefinition.Outputs.Exclusions,
			LogFile:         packageTask.RepoRelativeLogFile(),
			Dependencies:    stringAncestors,
			Dependents:      stringDescendents,
		})
		return nil
	}

	// This setup mirrors a real run. We call engine.execute() with
	// a visitor function and some hardcoded execOpts.
	// Note: we do not currently attempt to parallelize the graph walking
	// (as we do in real execution)
	visitorFn := g.GetPackageTaskVisitor(ctx, dryRunExecFunc)
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

func displayDryTextRun(ui cli.Ui, summary *dryRunSummary, workspaceInfos graph.WorkspaceInfos, isSinglePackage bool) error {
	if !isSinglePackage {
		ui.Output("")
		ui.Info(util.Sprintf("${CYAN}${BOLD}Packages in Scope${RESET}"))
		p := tabwriter.NewWriter(os.Stdout, 0, 0, 1, ' ', 0)
		fmt.Fprintln(p, "Name\tPath\t")
		for _, pkg := range summary.Packages {
			fmt.Fprintf(p, "%s\t%s\t\n", pkg, workspaceInfos[pkg].Dir)
		}
		if err := p.Flush(); err != nil {
			return err
		}
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
type taskSummary struct {
	TaskID          string           `json:"taskId"`
	Task            string           `json:"task"`
	Package         string           `json:"package"`
	Hash            string           `json:"hash"`
	CacheState      cache.ItemStatus `json:"cacheState"`
	Command         string           `json:"command"`
	Outputs         []string         `json:"outputs"`
	ExcludedOutputs []string         `json:"excludedOutputs"`
	LogFile         string           `json:"logFile"`
	Dir             string           `json:"directory"`
	Dependencies    []string         `json:"dependencies"`
	Dependents      []string         `json:"dependents"`
}

type singlePackageTaskSummary struct {
	Task            string           `json:"task"`
	Hash            string           `json:"hash"`
	CacheState      cache.ItemStatus `json:"cacheState"`
	Command         string           `json:"command"`
	Outputs         []string         `json:"outputs"`
	ExcludedOutputs []string         `json:"excludedOutputs"`
	LogFile         string           `json:"logFile"`
	Dependencies    []string         `json:"dependencies"`
	Dependents      []string         `json:"dependents"`
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
		Task:         util.RootTaskTaskName(ht.TaskID),
		Hash:         ht.Hash,
		CacheState:   ht.CacheState,
		Command:      ht.Command,
		Outputs:      ht.Outputs,
		LogFile:      ht.LogFile,
		Dependencies: dependencies,
		Dependents:   dependents,
	}
}
