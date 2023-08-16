package run

import (
	"bytes"
	gocontext "context"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"strings"
	"sync"
	"time"

	"github.com/fatih/color"
	"github.com/hashicorp/go-hclog"
	"github.com/mitchellh/cli"
	"github.com/pkg/errors"
	"github.com/vercel/turbo/cli/internal/cache"
	"github.com/vercel/turbo/cli/internal/cmdutil"
	"github.com/vercel/turbo/cli/internal/colorcache"
	"github.com/vercel/turbo/cli/internal/core"
	"github.com/vercel/turbo/cli/internal/env"
	"github.com/vercel/turbo/cli/internal/fs"
	"github.com/vercel/turbo/cli/internal/graph"
	"github.com/vercel/turbo/cli/internal/logstreamer"
	"github.com/vercel/turbo/cli/internal/nodes"
	"github.com/vercel/turbo/cli/internal/packagemanager"
	"github.com/vercel/turbo/cli/internal/process"
	"github.com/vercel/turbo/cli/internal/runcache"
	"github.com/vercel/turbo/cli/internal/runsummary"
	"github.com/vercel/turbo/cli/internal/spinner"
	"github.com/vercel/turbo/cli/internal/taskhash"
	"github.com/vercel/turbo/cli/internal/turbopath"
	"github.com/vercel/turbo/cli/internal/ui"
	"github.com/vercel/turbo/cli/internal/util"
)

// threadsafeOutputBuffer implements io.Writer for multiple goroutines
// to write to the same underlying buffer. Child processes use separate
// goroutines to handle reading from stdout and stderr, but for now we
// send both to the same buffer.
type threadsafeOutputBuffer struct {
	buf bytes.Buffer
	mu  sync.Mutex
}

func (tsob *threadsafeOutputBuffer) Write(p []byte) (n int, err error) {
	tsob.mu.Lock()
	defer tsob.mu.Unlock()
	return tsob.buf.Write(p)
}

func (tsob *threadsafeOutputBuffer) Bytes() []byte {
	return tsob.buf.Bytes()
}

// RealRun executes a set of tasks
func RealRun(
	ctx gocontext.Context,
	g *graph.CompleteGraph,
	rs *runSpec,
	engine *core.Engine,
	taskHashTracker *taskhash.Tracker,
	turboCache cache.Cache,
	turboJSON *fs.TurboJSON,
	globalEnvMode util.EnvMode,
	globalEnv env.EnvironmentVariableMap,
	globalPassThroughEnv env.EnvironmentVariableMap,
	packagesInScope []string,
	base *cmdutil.CmdBase,
	runSummary runsummary.Meta,
	packageManager *packagemanager.PackageManager,
	processes *process.Manager,
) error {
	singlePackage := rs.Opts.runOpts.SinglePackage

	if singlePackage {
		base.UI.Output(fmt.Sprintf("%s %s", ui.Dim("• Running"), ui.Dim(ui.Bold(strings.Join(rs.Targets, ", ")))))
	} else {
		base.UI.Output(fmt.Sprintf(ui.Dim("• Packages in scope: %v"), strings.Join(packagesInScope, ", ")))
		base.UI.Output(fmt.Sprintf("%s %s %s", ui.Dim("• Running"), ui.Dim(ui.Bold(strings.Join(rs.Targets, ", "))), ui.Dim(fmt.Sprintf("in %v packages", rs.FilteredPkgs.Len()))))
	}

	// Log whether remote cache is enabled
	useHTTPCache := !rs.Opts.cacheOpts.SkipRemote
	if useHTTPCache {
		base.UI.Info(ui.Dim("• Remote caching enabled"))
	} else {
		base.UI.Info(ui.Dim("• Remote caching disabled"))
	}

	defer func() {
		_ = spinner.WaitFor(ctx, turboCache.Shutdown, base.UI, "...writing to cache...", 1500*time.Millisecond)
	}()
	colorCache := colorcache.New()

	runCache := runcache.New(turboCache, base.RepoRoot, rs.Opts.runcacheOpts, colorCache)

	concurrentUIFactory := ui.ConcurrentUIFactory{
		Base: base.UIFactory,
	}

	ec := &execContext{
		colorCache:      colorCache,
		runSummary:      runSummary,
		rs:              rs,
		ui:              concurrentUIFactory.Build(os.Stdin, os.Stdout, os.Stderr),
		runCache:        runCache,
		env:             globalEnv,
		passThroughEnv:  globalPassThroughEnv,
		logger:          base.Logger,
		packageManager:  packageManager,
		processes:       processes,
		taskHashTracker: taskHashTracker,
		repoRoot:        base.RepoRoot,
		isSinglePackage: singlePackage,
	}

	// run the thing
	execOpts := core.EngineExecutionOptions{
		Parallel:    rs.Opts.runOpts.Parallel,
		Concurrency: rs.Opts.runOpts.Concurrency,
	}

	taskCount := len(engine.TaskGraph.Vertices())
	logChan := make(chan taskLogContext, taskCount)
	logWaitGroup := sync.WaitGroup{}
	isGrouped := rs.Opts.runOpts.LogOrder == "grouped"

	if isGrouped {
		logWaitGroup.Add(1)
		go func() {
			for logContext := range logChan {

				outBytes := logContext.outBuf.Bytes()
				errBytes := logContext.errBuf.Bytes()

				_, errOut := os.Stdout.Write(outBytes)
				_, errErr := os.Stderr.Write(errBytes)

				if errOut != nil || errErr != nil {
					ec.ui.Error("Failed to output some of the logs.")
				}

			}
			logWaitGroup.Done()
		}()
	}

	taskSummaryMutex := sync.Mutex{}
	taskSummaries := []*runsummary.TaskSummary{}
	execFunc := func(ctx gocontext.Context, packageTask *nodes.PackageTask, taskSummary *runsummary.TaskSummary) error {
		outBuf := &bytes.Buffer{}
		errBuf := &bytes.Buffer{}

		var outWriter io.Writer = os.Stdout
		var errWriter io.Writer = os.Stderr

		if isGrouped {
			outWriter = outBuf
			errWriter = errBuf
		}

		var spacesLogBuffer *threadsafeOutputBuffer
		if runSummary.SpacesIsEnabled() {
			spacesLogBuffer = &threadsafeOutputBuffer{}
			outWriter = io.MultiWriter(spacesLogBuffer, outWriter)
			errWriter = io.MultiWriter(spacesLogBuffer, errWriter)
		}

		ui := concurrentUIFactory.Build(os.Stdin, outWriter, errWriter)

		taskExecutionSummary, err := ec.exec(ctx, packageTask, ui, outWriter)

		// taskExecutionSummary will be nil if the task never executed
		// (i.e. if the workspace didn't implement the script corresponding to the task)
		// We don't need to collect any of the outputs or execution if the task didn't execute.
		if taskExecutionSummary != nil {
			taskSummary.ExpandedOutputs = taskHashTracker.GetExpandedOutputs(taskSummary.TaskID)
			taskSummary.Execution = taskExecutionSummary
			taskSummary.CacheSummary = taskHashTracker.GetCacheStatus(taskSummary.TaskID)

			// lock since multiple things to be appending to this array at the same time
			taskSummaryMutex.Lock()
			taskSummaries = append(taskSummaries, taskSummary)
			// not using defer, just release the lock
			taskSummaryMutex.Unlock()

			var logBytes []byte
			if spacesLogBuffer != nil {
				logBytes = spacesLogBuffer.Bytes()
			}
			runSummary.CloseTask(taskSummary, logBytes)
		}
		if isGrouped {
			logChan <- taskLogContext{
				outBuf: outBuf,
				errBuf: errBuf,
			}
		}

		// Return the error when there is one
		if err != nil {
			return err
		}

		return nil
	}

	getArgs := func(taskID string) []string {
		return rs.ArgsForTask(taskID)
	}

	visitorFn := g.GetPackageTaskVisitor(ctx, engine.TaskGraph, rs.Opts.runOpts.FrameworkInference, globalEnvMode, getArgs, base.Logger, execFunc)
	errs := engine.Execute(visitorFn, execOpts)

	// Track if we saw any child with a non-zero exit code
	exitCode := 0
	exitCodeErr := &process.ChildExit{}

	// Assign tasks after execution
	runSummary.RunSummary.Tasks = taskSummaries

	for _, err := range errs {
		if errors.As(err, &exitCodeErr) {
			// If a process gets killed via a signal, Go reports it's exit code as -1.
			// We take the absolute value of the exit code so we don't select '0' as
			// the greatest exit code.
			childExit := exitCodeErr.ExitCode
			if childExit < 0 {
				childExit = -childExit
			}
			if childExit > exitCode {
				exitCode = childExit
			}
		} else if exitCode == 0 {
			// We hit some error, it shouldn't be exit code 0
			exitCode = 1
		}
		base.UI.Error(err.Error())
	}

	// When continue on error is enabled don't register failed tasks as errors
	// and instead must inspect the task summaries.
	if ec.rs.Opts.runOpts.ContinueOnError {
		for _, summary := range runSummary.RunSummary.Tasks {
			if childExit := summary.Execution.ExitCode(); childExit != nil {
				childExit := *childExit
				if childExit < 0 {
					childExit = -childExit
				}
				if childExit > exitCode {
					exitCode = childExit
				}
			}
		}
	}

	if isGrouped {
		close(logChan)
		logWaitGroup.Wait()
	}

	if err := runSummary.Close(ctx, exitCode, g.WorkspaceInfos, base.UI); err != nil {
		// We don't need to throw an error, but we can warn on this.
		// Note: this method doesn't actually return an error for Real Runs at the time of writing.
		base.UI.Info(fmt.Sprintf("Failed to close Run Summary %v", err))
	}

	if exitCode != 0 {
		return &process.ChildExit{
			ExitCode: exitCode,
		}
	}
	return nil
}

type taskLogContext struct {
	outBuf *bytes.Buffer
	errBuf *bytes.Buffer
}

type execContext struct {
	colorCache      *colorcache.ColorCache
	runSummary      runsummary.Meta
	rs              *runSpec
	ui              cli.Ui
	runCache        *runcache.RunCache
	env             env.EnvironmentVariableMap
	passThroughEnv  env.EnvironmentVariableMap
	logger          hclog.Logger
	packageManager  *packagemanager.PackageManager
	processes       *process.Manager
	taskHashTracker *taskhash.Tracker
	repoRoot        turbopath.AbsoluteSystemPath
	isSinglePackage bool
}

func (ec *execContext) logError(prefix string, err error) {
	ec.logger.Error(prefix, "error", err)

	if prefix != "" {
		prefix += ": "
	}

	ec.ui.Error(fmt.Sprintf("%s%s%s", ui.ERROR_PREFIX, prefix, color.RedString(" %v", err)))
}

func (ec *execContext) exec(ctx gocontext.Context, packageTask *nodes.PackageTask, ui cli.Ui, outWriter io.Writer) (*runsummary.TaskExecutionSummary, error) {
	// Setup tracer. Every time tracer() is called the taskExecutionSummary's duration is updated
	// So make sure to call it before returning.
	successExitCode := 0 // We won't use this till later

	tracer, taskExecutionSummary := ec.runSummary.RunSummary.TrackTask(packageTask.TaskID)
	progressLogger := ec.logger.Named("")
	progressLogger.Debug("start")

	passThroughArgs := ec.rs.ArgsForTask(packageTask.Task)
	hash := packageTask.Hash
	ec.logger.Debug("task hash", "value", hash)
	// TODO(gsoltis): if/when we fix https://github.com/vercel/turbo/issues/937
	// the following block should never get hit. In the meantime, keep it after hashing
	// so that downstream tasks can count on the hash existing
	//
	// bail if the script doesn't exist
	if packageTask.Command == "" {
		progressLogger.Debug("no task in package, skipping")
		progressLogger.Debug("done", "status", "skipped", "duration", taskExecutionSummary.Duration)
		// Return nil here because there was no execution, so there is no task execution summary
		return nil, nil
	}

	// Set building status now that we know it's going to run.
	tracer(runsummary.TargetBuilding, nil, &successExitCode)

	var prefix string
	var prettyPrefix string
	if ec.rs.Opts.runOpts.LogPrefix == "none" {
		prefix = ""
	} else {
		prefix = packageTask.OutputPrefix(ec.isSinglePackage)
	}

	prettyPrefix = ec.colorCache.PrefixWithColor(packageTask.PackageName, prefix)

	// Cache ---------------------------------------------
	taskCache := ec.runCache.TaskCache(packageTask, hash)
	// Create a logger for replaying
	prefixedUI := &cli.PrefixedUi{
		Ui:           ui,
		OutputPrefix: prettyPrefix,
		InfoPrefix:   prettyPrefix,
		ErrorPrefix:  prettyPrefix,
		WarnPrefix:   prettyPrefix,
	}

	if ec.rs.Opts.runOpts.IsGithubActions {
		ui.Output(fmt.Sprintf("::group::%s", packageTask.OutputPrefix(ec.isSinglePackage)))
	}

	cacheStatus, err := taskCache.RestoreOutputs(ctx, prefixedUI, progressLogger)

	// It's safe to set the CacheStatus even if there's an error, because if there's
	// an error, the 0 values are actually what we want. We save cacheStatus and timeSaved
	// for the task, so that even if there's an error, we have those values for the taskSummary.
	ec.taskHashTracker.SetCacheStatus(
		packageTask.TaskID,
		runsummary.NewTaskCacheSummary(cacheStatus),
	)

	if err != nil {
		prefixedUI.Error(fmt.Sprintf("error fetching from cache: %s", err))
	} else if cacheStatus.Hit { // If there was a cache hit
		ec.taskHashTracker.SetExpandedOutputs(packageTask.TaskID, taskCache.ExpandedOutputs)
		// We only cache successful executions, so we can assume this is a successExitCode exit.
		tracer(runsummary.TargetCached, nil, &successExitCode)
		return taskExecutionSummary, nil
	}

	// Setup command execution
	argsactual := append([]string{"run"}, packageTask.Task)
	if len(passThroughArgs) > 0 {
		// This will be either '--' or a typed nil
		argsactual = append(argsactual, ec.packageManager.ArgSeparator...)
		argsactual = append(argsactual, passThroughArgs...)
	}

	cmd := exec.Command(ec.packageManager.Command, argsactual...)
	cmd.Dir = packageTask.Pkg.Dir.ToSystemPath().RestoreAnchor(ec.repoRoot).ToString()

	passThroughEnv := env.EnvironmentVariableMap{}

	if packageTask.EnvMode == util.Strict {
		defaultPassThroughEnvVarMap, err := ec.taskHashTracker.EnvAtExecutionStart.FromWildcards([]string{
			"PATH",
			"SHELL",
			"SYSTEMROOT", // Go will always include this on Windows, but we're being explicit here
		})
		if err != nil {
			return nil, err
		}

		envVarPassThroughMap, err := ec.taskHashTracker.EnvAtExecutionStart.FromWildcards(packageTask.TaskDefinition.PassThroughEnv)
		if err != nil {
			return nil, err
		}

		passThroughEnv.Union(defaultPassThroughEnvVarMap)
		passThroughEnv.Union(ec.env)
		passThroughEnv.Union(ec.passThroughEnv)
		passThroughEnv.Union(ec.taskHashTracker.GetEnvVars(packageTask.TaskID).All)
		passThroughEnv.Union(envVarPassThroughMap)
	} else {
		passThroughEnv.Union(ec.taskHashTracker.EnvAtExecutionStart)
	}

	// Always last to make sure it clobbers.
	passThroughEnv.Add("TURBO_HASH", hash)

	cmd.Env = passThroughEnv.ToHashable()

	// Setup stdout/stderr
	// If we are not caching anything, then we don't need to write logs to disk
	// be careful about this conditional given the default of cache = true
	writer, err := taskCache.OutputWriter(prettyPrefix, outWriter)
	if err != nil {
		tracer(runsummary.TargetBuildFailed, err, nil)

		ec.logError(prettyPrefix, err)
		if !ec.rs.Opts.runOpts.ContinueOnError {
			return nil, core.StopExecution(errors.Wrapf(err, "failed to capture outputs for \"%v\"", packageTask.TaskID))
		}
	}

	// Create a logger
	logger := log.New(writer, "", 0)
	// Setup a streamer that we'll pipe cmd.Stdout to
	logStreamerOut := logstreamer.NewLogstreamer(logger, prettyPrefix, false)
	// Setup a streamer that we'll pipe cmd.Stderr to.
	logStreamerErr := logstreamer.NewLogstreamer(logger, prettyPrefix, false)
	cmd.Stderr = logStreamerErr
	cmd.Stdout = logStreamerOut
	// Flush/Reset any error we recorded
	logStreamerErr.FlushRecord()
	logStreamerOut.FlushRecord()

	closeOutputs := func() error {
		var closeErrors []error
		if ec.rs.Opts.runOpts.IsGithubActions {
			// We don't use the prefixedUI here because the prefix in this case would include
			// the ::group::<taskID>, and we explicitly want to close the github group
			ui.Output("::endgroup::")
		}

		if err := logStreamerOut.Close(); err != nil {
			closeErrors = append(closeErrors, errors.Wrap(err, "log stdout"))
		}
		if err := logStreamerErr.Close(); err != nil {
			closeErrors = append(closeErrors, errors.Wrap(err, "log stderr"))
		}

		if err := writer.Close(); err != nil {
			closeErrors = append(closeErrors, errors.Wrap(err, "log file"))
		}
		if len(closeErrors) > 0 {
			msgs := make([]string, len(closeErrors))
			for i, err := range closeErrors {
				msgs[i] = err.Error()
			}
			return fmt.Errorf("could not flush log output: %v", strings.Join(msgs, ", "))
		}
		return nil
	}

	// Run the command
	if err := ec.processes.Exec(cmd); err != nil {
		// close off our outputs. We errored, so we mostly don't care if we fail to close
		_ = closeOutputs()
		// if we already know we're in the process of exiting,
		// we don't need to record an error to that effect.
		if errors.Is(err, process.ErrClosing) {
			return taskExecutionSummary, nil
		}

		// If the error we got is a ChildExit, it will have an ExitCode field
		// Pass that along into the tracer.
		var e *process.ChildExit
		if errors.As(err, &e) {
			tracer(runsummary.TargetBuildFailed, err, &e.ExitCode)
		} else {
			// If it wasn't a ChildExit, and something else went wrong, we don't have an exitCode
			tracer(runsummary.TargetBuildFailed, err, nil)
		}

		// If there was an error, flush the buffered output
		taskCache.OnError(prefixedUI, progressLogger)
		progressLogger.Error(fmt.Sprintf("Error: command finished with error: %v", err))
		if !ec.rs.Opts.runOpts.ContinueOnError {
			prefixedUI.Error(fmt.Sprintf("ERROR: command finished with error: %s", err))
			ec.processes.Close()
			// We're not continuing, stop graph traversal
			err = core.StopExecution(err)
		} else {
			prefixedUI.Warn("command finished with error, but continuing...")
		}

		return taskExecutionSummary, err
	}

	// Add another timestamp into the tracer, so we have an accurate timestamp for how long the task took.
	tracer(runsummary.TargetExecuted, nil, nil)

	// Close off our outputs and cache them
	if err := closeOutputs(); err != nil {
		ec.logError("", err)
	} else {
		if err = taskCache.SaveOutputs(ctx, progressLogger, prefixedUI, int(taskExecutionSummary.Duration.Milliseconds())); err != nil {
			ec.logError("", fmt.Errorf("error caching output: %w", err))
		} else {
			ec.taskHashTracker.SetExpandedOutputs(packageTask.TaskID, taskCache.ExpandedOutputs)
		}
	}

	// Clean up tracing

	tracer(runsummary.TargetBuilt, nil, &successExitCode)
	progressLogger.Debug("done", "status", "complete", "duration", taskExecutionSummary.Duration)
	return taskExecutionSummary, nil
}
