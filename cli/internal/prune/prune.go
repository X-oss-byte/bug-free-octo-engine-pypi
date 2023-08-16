package prune

import (
	"bufio"
	"bytes"
	"fmt"
	"os"
	"strings"

	"github.com/spf13/cobra"
	"github.com/spf13/pflag"
	"github.com/vercel/turborepo/cli/internal/cache"
	"github.com/vercel/turborepo/cli/internal/config"
	"github.com/vercel/turborepo/cli/internal/context"
	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/ui"
	"github.com/vercel/turborepo/cli/internal/util"

	"github.com/fatih/color"
	"github.com/hashicorp/go-hclog"
	"github.com/mitchellh/cli"
	"github.com/pkg/errors"
	"gopkg.in/yaml.v3"
)

// PruneCommand is a Command implementation that tells Turbo to run a task
type PruneCommand struct {
	Config *config.Config
	Ui     *cli.ColoredUi
}

// Synopsis of run command
func (c *PruneCommand) Synopsis() string {
	return getCmd(c.Config, c.Ui).Short
}

// Help returns information about the `run` command
func (c *PruneCommand) Help() string {
	cmd := getCmd(c.Config, c.Ui)
	return util.HelpForCobraCmd(cmd)
}

// Run implements cli.Command.Run
func (c *PruneCommand) Run(args []string) int {
	cmd := getCmd(c.Config, c.Ui)
	cmd.SetArgs(args)
	if err := cmd.Execute(); err != nil {
		return 1
	}
	return 0
}

type opts struct {
	scope     string
	docker    bool
	outputDir string
}

func addPruneFlags(opts *opts, flags *pflag.FlagSet) {
	flags.StringVar(&opts.scope, "scope", "", "Specify package to act as entry point for pruned monorepo (required).")
	flags.BoolVar(&opts.docker, "docker", false, "Output pruned workspace into 'full' and 'json' directories optimized for Docker layer caching.")
	flags.StringVar(&opts.outputDir, "out-dir", "out", "Set the root directory for files output by this command")
	// No-op the cwd flag while the root level command is not yet cobra
	_ = flags.String("cwd", "", "")
	if err := flags.MarkHidden("cwd"); err != nil {
		// Fail fast if we have misconfigured our flags
		panic(err)
	}
}

func getCmd(config *config.Config, ui cli.Ui) *cobra.Command {
	opts := &opts{}
	cmd := &cobra.Command{
		Use:                   "turbo prune --scope=<package name> [<flags>]",
		Short:                 "Prepare a subset of your monorepo.",
		SilenceUsage:          true,
		SilenceErrors:         true,
		DisableFlagsInUseLine: true,
		RunE: func(cmd *cobra.Command, args []string) error {
			logger := config.Logger.Named("prune")
			if len(args) > 0 {
				err := errors.Errorf("unexpected arguments: %v", args)
				logError(logger, ui, err)
				return err
			}
			if opts.scope == "" {
				err := errors.New("at least one target must be specified")
				logError(logger, ui, err)
				return err
			}
			p := &prune{
				logger: logger,
				ui:     ui,
				config: config,
			}
			if err := p.prune(opts); err != nil {
				logError(p.logger, p.ui, err)
				return err
			}
			return nil
		},
	}
	addPruneFlags(opts, cmd.Flags())
	return cmd
}

func logError(logger hclog.Logger, ui cli.Ui, err error) {
	logger.Error("error", err)
	pref := color.New(color.Bold, color.FgRed, color.ReverseVideo).Sprint(" ERROR ")
	ui.Error(fmt.Sprintf("%s%s", pref, color.RedString(" %v", err)))
}

type prune struct {
	logger hclog.Logger
	ui     cli.Ui
	config *config.Config
}

// Prune creates a smaller monorepo with only the required workspaces
func (p *prune) prune(opts *opts) error {
	cacheDir := cache.DefaultLocation(p.config.Cwd)
	ctx, err := context.New(context.WithGraph(p.config, cacheDir))
	if err != nil {
		return errors.Wrap(err, "could not construct graph")
	}
	p.logger.Trace("scope", "value", opts.scope)
	target, scopeIsValid := ctx.PackageInfos[opts.scope]
	if !scopeIsValid {
		return errors.Errorf("invalid scope: package %v not found", opts.scope)
	}
	outDir := p.config.Cwd.Join(opts.outputDir)
	p.logger.Trace("target", "value", target.Name)
	p.logger.Trace("directory", "value", target.Dir)
	p.logger.Trace("external deps", "value", target.UnresolvedExternalDeps)
	p.logger.Trace("internal deps", "value", target.InternalDeps)
	p.logger.Trace("docker", "value", opts.docker)
	p.logger.Trace("out dir", "value", outDir.ToString())

	if !util.IsYarn(ctx.PackageManager.Name) {
		return errors.Errorf("this command is not yet implemented for %s", ctx.PackageManager.Name)
	} else if ctx.PackageManager.Name == "nodejs-berry" {
		if isNMLinker, err := util.IsNMLinker(p.config.Cwd.ToStringDuringMigration()); err != nil {
			return errors.Wrap(err, "could not determine if yarn is using `nodeLinker: node-modules`")
		} else if !isNMLinker {
			return errors.New("only yarn v2/v3 with `nodeLinker: node-modules` is supported at this time")
		}
	}

	p.ui.Output(fmt.Sprintf("Generating pruned monorepo for %v in %v", ui.Bold(opts.scope), ui.Bold(outDir.ToString())))

	packageJSONPath := outDir.Join("package.json")
	if err := packageJSONPath.EnsureDir(); err != nil {
		return errors.Wrap(err, "could not create output directory")
	}
	workspaces := []string{}
	lockfile := p.config.RootPackageJSON.SubLockfile
	targets := []interface{}{opts.scope}
	internalDeps, err := ctx.TopologicalGraph.Ancestors(opts.scope)
	if err != nil {
		return errors.Wrap(err, "could find traverse the dependency graph to find topological dependencies")
	}
	targets = append(targets, internalDeps.List()...)

	for _, internalDep := range targets {
		if internalDep == ctx.RootNode {
			continue
		}
		workspaces = append(workspaces, ctx.PackageInfos[internalDep].Dir)
		if opts.docker {
			targetDir := outDir.Join("full", ctx.PackageInfos[internalDep].Dir)
			jsonDir := outDir.Join("json", ctx.PackageInfos[internalDep].PackageJSONPath)
			if err := targetDir.EnsureDir(); err != nil {
				return errors.Wrapf(err, "failed to create folder %v for %v", targetDir, internalDep)
			}
			if err := fs.RecursiveCopy(ctx.PackageInfos[internalDep].Dir, targetDir.ToStringDuringMigration()); err != nil {
				return errors.Wrapf(err, "failed to copy %v into %v", internalDep, targetDir)
			}
			if err := jsonDir.EnsureDir(); err != nil {
				return errors.Wrapf(err, "failed to create folder %v for %v", jsonDir, internalDep)
			}
			if err := fs.RecursiveCopy(ctx.PackageInfos[internalDep].PackageJSONPath, jsonDir.ToStringDuringMigration()); err != nil {
				return errors.Wrapf(err, "failed to copy %v into %v", internalDep, jsonDir)
			}
		} else {
			targetDir := outDir.Join(ctx.PackageInfos[internalDep].Dir)
			if err := targetDir.EnsureDir(); err != nil {
				return errors.Wrapf(err, "failed to create folder %v for %v", targetDir, internalDep)
			}
			if err := fs.RecursiveCopy(ctx.PackageInfos[internalDep].Dir, targetDir.ToStringDuringMigration()); err != nil {
				return errors.Wrapf(err, "failed to copy %v into %v", internalDep, targetDir)
			}
		}

		for k, v := range ctx.PackageInfos[internalDep].SubLockfile {
			lockfile[k] = v
		}

		p.ui.Output(fmt.Sprintf(" - Added %v", ctx.PackageInfos[internalDep].Name))
	}
	p.logger.Trace("new workspaces", "value", workspaces)
	if opts.docker {
		if fs.FileExists(".gitignore") {
			if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join(".gitignore")}, outDir.Join("full", ".gitignore").ToStringDuringMigration()); err != nil {
				return errors.Wrap(err, "failed to copy root .gitignore")
			}
		}
		// We only need to actually copy turbo.json into "full" folder since it isn't needed for installation in docker
		if fs.FileExists("turbo.json") {
			if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join("turbo.json")}, outDir.Join("full", "turbo.json").ToStringDuringMigration()); err != nil {
				return errors.Wrap(err, "failed to copy root turbo.json")
			}
		}

		if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join("package.json")}, outDir.Join("full", "package.json").ToStringDuringMigration()); err != nil {
			return errors.Wrap(err, "failed to copy root package.json")
		}

		if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join("package.json")}, outDir.Join("json", "package.json").ToStringDuringMigration()); err != nil {
			return errors.Wrap(err, "failed to copy root package.json")
		}
	} else {
		if fs.FileExists(".gitignore") {
			if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join(".gitignore")}, outDir.Join(".gitignore").ToStringDuringMigration()); err != nil {
				return errors.Wrap(err, "failed to copy root .gitignore")
			}
		}

		if fs.FileExists("turbo.json") {
			if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join("turbo.json")}, outDir.Join("turbo.json").ToStringDuringMigration()); err != nil {
				return errors.Wrap(err, "failed to copy root turbo.json")
			}
		}

		if err := fs.CopyFile(&fs.LstatCachedFile{Path: p.config.Cwd.Join("package.json")}, outDir.Join("package.json").ToStringDuringMigration()); err != nil {
			return errors.Wrap(err, "failed to copy root package.json")
		}
	}

	var b bytes.Buffer
	yamlEncoder := yaml.NewEncoder(&b)
	yamlEncoder.SetIndent(2)
	if err := yamlEncoder.Encode(lockfile); err != nil {
		return errors.Wrap(err, "failed to materialize sub-lockfile. This can happen if your lockfile contains merge conflicts or is somehow corrupted. Please report this if it occurs")
	}
	if err := outDir.Join("yarn.lock").WriteFile(b.Bytes(), fs.DirPermissions); err != nil {
		return errors.Wrap(err, "failed to write sub-lockfile")
	}

	yarnTmpFilePath := outDir.Join("yarn-tmp.lock")
	tmpGeneratedLockfile, err := yarnTmpFilePath.Create()
	if err != nil {
		return errors.Wrap(err, "failed create temporary lockfile")
	}
	tmpGeneratedLockfileWriter := bufio.NewWriter(tmpGeneratedLockfile)

	if ctx.PackageManager.Name == "nodejs-yarn" {
		tmpGeneratedLockfileWriter.WriteString("# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n# yarn lockfile v1\n\n")
	} else {
		tmpGeneratedLockfileWriter.WriteString("# This file is generated by running \"yarn install\" inside your project.\n# Manual changes might be lost - proceed with caution!\n\n__metadata:\nversion: 5\ncacheKey: 8\n\n")
	}

	// because of yarn being yarn, we need to inject lines in between each block of YAML to make it "valid" SYML
	lockFilePath := outDir.Join("yarn.lock")
	generatedLockfile, err := lockFilePath.Open()
	if err != nil {
		return errors.Wrap(err, "failed to massage lockfile")
	}

	scan := bufio.NewScanner(generatedLockfile)
	buf := make([]byte, 0, 1024*1024)
	scan.Buffer(buf, 10*1024*1024)
	for scan.Scan() {
		line := scan.Text() //Writing to Stdout
		if !strings.HasPrefix(line, " ") {
			tmpGeneratedLockfileWriter.WriteString(fmt.Sprintf("\n%v\n", strings.ReplaceAll(line, "'", "\"")))
		} else {
			tmpGeneratedLockfileWriter.WriteString(fmt.Sprintf("%v\n", strings.ReplaceAll(line, "'", "\"")))
		}
	}
	// Make sure to flush the log write before we start saving it.
	if err := tmpGeneratedLockfileWriter.Flush(); err != nil {
		return errors.Wrap(err, "failed to flush to temporary lock file")
	}

	// Close the files before we rename them
	if err := tmpGeneratedLockfile.Close(); err != nil {
		return errors.Wrap(err, "failed to close temporary lock file")
	}
	if err := generatedLockfile.Close(); err != nil {
		return errors.Wrap(err, "failed to close existing lock file")
	}

	// Rename the file
	if err := os.Rename(yarnTmpFilePath.ToStringDuringMigration(), lockFilePath.ToStringDuringMigration()); err != nil {
		return errors.Wrap(err, "failed finalize lockfile")
	}
	return nil
}
