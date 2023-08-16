// Package cmd holds the root cobra command for turbo
package cmd

import (
	"context"
	"fmt"
	"os"
	"runtime/pprof"
	"runtime/trace"

	"github.com/pkg/errors"
	"github.com/spf13/pflag"
	"github.com/vercel/turbo/cli/internal/cmd/auth"
	"github.com/vercel/turbo/cli/internal/cmdutil"
	"github.com/vercel/turbo/cli/internal/daemon"
	"github.com/vercel/turbo/cli/internal/login"
	"github.com/vercel/turbo/cli/internal/process"
	"github.com/vercel/turbo/cli/internal/prune"
	"github.com/vercel/turbo/cli/internal/run"
	"github.com/vercel/turbo/cli/internal/signals"
	"github.com/vercel/turbo/cli/internal/turbostate"
	"github.com/vercel/turbo/cli/internal/util"
)

type execOpts struct {
	heapFile       string
	cpuProfileFile string
	traceFile      string
}

func (eo *execOpts) addFlags(flags *pflag.FlagSet) {
	// Note that these are relative to the actual CWD, and do not respect the --cwd flag.
	// This is because a user likely wants to inspect them after execution, and may not immediately
	// know the repo root, depending on how turbo was invoked.
	flags.StringVar(&eo.heapFile, "heap", "", "Specify a file to save a pprof heap profile")
	flags.StringVar(&eo.cpuProfileFile, "cpuprofile", "", "Specify a file to save a cpu profile")
	flags.StringVar(&eo.traceFile, "trace", "", "Specify a file to save a pprof trace")
}

func initializeOutputFiles(helper *cmdutil.Helper, parsedArgs turbostate.ParsedArgsFromRust) error {
	if parsedArgs.Trace != "" {
		cleanup, err := createTraceFile(parsedArgs.Trace)
		if err != nil {
			return fmt.Errorf("failed to create trace file: %v", err)
		}
		helper.RegisterCleanup(cleanup)
	}
	if parsedArgs.Heap != "" {
		cleanup, err := createHeapFile(parsedArgs.Heap)
		if err != nil {
			return fmt.Errorf("failed to create heap file: %v", err)
		}
		helper.RegisterCleanup(cleanup)
	}
	if parsedArgs.CPUProfile != "" {
		cleanup, err := createCpuprofileFile(parsedArgs.CPUProfile)
		if err != nil {
			return fmt.Errorf("failed to create CPU profile file: %v", err)
		}
		helper.RegisterCleanup(cleanup)
	}

	return nil
}

// RunWithTurboState runs turbo with the CLIExecutionStateFromRust that is passed from the Rust side.
func RunWithTurboState(state turbostate.CLIExecutionStateFromRust, turboVersion string) int {
	util.InitPrintf()
	// TODO: replace this with a context
	signalWatcher := signals.NewWatcher()
	helper := cmdutil.NewHelper(turboVersion)
	ctx := context.Background()

	err := initializeOutputFiles(helper, state.ParsedArgs)
	if err != nil {
		fmt.Printf("%v", err)
		return 1
	}
	defer helper.Cleanup(&state.ParsedArgs)

	doneCh := make(chan struct{})
	var execErr error
	go func() {
		command := state.ParsedArgs.Command
		if command.Link != nil {
			execErr = login.ExecuteLink(helper, &state.ParsedArgs)
		} else if command.Login != nil {
			execErr = login.ExecuteLogin(ctx, helper, &state.ParsedArgs)
		} else if command.Logout != nil {
			execErr = auth.ExecuteLogout(helper, &state.ParsedArgs)
		} else if command.Unlink != nil {
			execErr = auth.ExecuteUnlink(helper, &state.ParsedArgs)
		} else if command.Daemon != nil {
			execErr = daemon.ExecuteDaemon(ctx, helper, signalWatcher, &state.ParsedArgs)
		} else if command.Prune != nil {
			execErr = prune.ExecutePrune(helper, &state.ParsedArgs)
		} else if command.Run != nil {
			execErr = run.ExecuteRun(ctx, helper, signalWatcher, &state)
		} else {
			execErr = fmt.Errorf("unknown command: %v", command)
		}

		close(doneCh)
	}()

	// Wait for either our command to finish, in which case we need to clean up,
	// or to receive a signal, in which case the signal handler above does the cleanup
	select {
	case <-doneCh:
		// We finished whatever task we were running
		signalWatcher.Close()
		exitErr := &process.ChildExit{}
		if errors.As(execErr, &exitErr) {
			return exitErr.ExitCode
		} else if execErr != nil {
			return 1
		}
		return 0
	case <-signalWatcher.Done():
		// We caught a signal, which already called the close handlers
		return 1
	}
}

type profileCleanup func() error

// Close implements io.Close for profileCleanup
func (pc profileCleanup) Close() error {
	return pc()
}

// To view a CPU trace, use "go tool trace [file]". Note that the trace
// viewer doesn't work under Windows Subsystem for Linux for some reason.
func createTraceFile(traceFile string) (profileCleanup, error) {
	f, err := os.Create(traceFile)
	if err != nil {
		return nil, errors.Wrapf(err, "failed to create trace file: %v", traceFile)
	}
	if err := trace.Start(f); err != nil {
		return nil, errors.Wrap(err, "failed to start tracing")
	}
	return func() error {
		trace.Stop()
		return f.Close()
	}, nil
}

// To view a heap trace, use "go tool pprof [file]" and type "top". You can
// also drop it into https://speedscope.app and use the "left heavy" or
// "sandwich" view modes.
func createHeapFile(heapFile string) (profileCleanup, error) {
	f, err := os.Create(heapFile)
	if err != nil {
		return nil, errors.Wrapf(err, "failed to create heap file: %v", heapFile)
	}
	return func() error {
		if err := pprof.WriteHeapProfile(f); err != nil {
			// we don't care if we fail to close the file we just failed to write to
			_ = f.Close()
			return errors.Wrapf(err, "failed to write heap file: %v", heapFile)
		}
		return f.Close()
	}, nil
}

// To view a CPU profile, drop the file into https://speedscope.app.
// Note: Running the CPU profiler doesn't work under Windows subsystem for
// Linux. The profiler has to be built for native Windows and run using the
// command prompt instead.
func createCpuprofileFile(cpuprofileFile string) (profileCleanup, error) {
	f, err := os.Create(cpuprofileFile)
	if err != nil {
		return nil, errors.Wrapf(err, "failed to create cpuprofile file: %v", cpuprofileFile)
	}
	if err := pprof.StartCPUProfile(f); err != nil {
		return nil, errors.Wrap(err, "failed to start CPU profiling")
	}
	return func() error {
		pprof.StopCPUProfile()
		return f.Close()
	}, nil
}
