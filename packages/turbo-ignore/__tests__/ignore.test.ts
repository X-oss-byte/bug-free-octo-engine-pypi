import child_process, { ChildProcess, ExecException } from "child_process";
import turboIgnore from "../src/ignore";
import { spyExit, spyConsole, mockEnv, validateLogs } from "./test-utils";
import type { SpyExit } from "./test-utils";

function expectBuild(mockExit: SpyExit) {
  expect(mockExit.exit).toHaveBeenCalledWith(1);
}

function expectIgnore(mockExit: SpyExit) {
  expect(mockExit.exit).toHaveBeenCalledWith(0);
}

describe("turboIgnore()", () => {
  mockEnv();
  const mockExit = spyExit();
  const mockConsole = spyConsole();

  it("throws error and allows build when exec fails", async () => {
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            "error" as unknown as ExecException,
            "stdout",
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });

    turboIgnore({
      args: { workspace: "test-workspace" },
    });

    expect(mockExec).toHaveBeenCalledWith(
      "npx turbo run build --filter=test-workspace...[HEAD^] --dry=json",
      expect.anything(),
      expect.anything()
    );

    validateLogs(["UNKNOWN_ERROR: error"], mockConsole.error);

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("throws pretty error and allows build when exec fails", async () => {
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            {
              message:
                "run failed: We did not detect an in-use package manager for your project",
            } as unknown as ExecException,
            "stdout",
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });

    turboIgnore({
      args: { workspace: "test-workspace" },
    });

    expect(mockExec).toHaveBeenCalledWith(
      "npx turbo run build --filter=test-workspace...[HEAD^] --dry=json",
      expect.anything(),
      expect.anything()
    );

    validateLogs(
      [
        `turbo-ignore could not complete - no package manager detected, please commit a lockfile, or set "packageManager" in your root "package.json"`,
      ],
      mockConsole.warn
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("throws pretty error and allows build when fallback fails", async () => {
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            {
              message:
                "ERROR run failed: failed to resolve packages to run: commit HEAD^ does not exist",
            } as unknown as ExecException,
            "stdout",
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });

    turboIgnore({
      args: { workspace: "test-workspace" },
    });

    expect(mockExec).toHaveBeenCalledWith(
      "npx turbo run build --filter=test-workspace...[HEAD^] --dry=json",
      expect.anything(),
      expect.anything()
    );

    validateLogs(
      [
        `turbo-ignore could not complete - not enough information available to compare`,
      ],
      mockConsole.warn
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("skips checks and allows build when no workspace can be found", async () => {
    turboIgnore({
      args: {
        directory: "__fixtures__/no-app",
      },
    });
    validateLogs(
      [
        () =>
          expect.stringContaining(
            " could not be found. turbo-ignore inferencing failed"
          ),
      ],
      mockConsole.error
    );
    expectBuild(mockExit);
  });

  it("skips checks and allows build when a workspace with no name is found", async () => {
    turboIgnore({
      args: {
        directory: "__fixtures__/invalid-app",
      },
    });
    validateLogs(
      [
        () =>
          expect.stringContaining(' is missing the "name" field (required).'),
      ],
      mockConsole.error
    );
    expectBuild(mockExit);
  });

  it("skips checks and allows build when no monorepo root can be found", async () => {
    turboIgnore({
      args: { directory: "/" },
    });
    expectBuild(mockExit);
    expect(mockConsole.error).toHaveBeenLastCalledWith(
      "≫  ",
      "monorepo root not found. turbo-ignore inferencing failed"
    );
  });

  it("skips checks and allows build when TURBO_FORCE is set", async () => {
    process.env.TURBO_FORCE = "true";
    turboIgnore({
      args: { workspace: "test-workspace" },
    });
    expect(mockConsole.log).toHaveBeenNthCalledWith(
      2,
      "≫  ",
      "`TURBO_FORCE` detected"
    );
    expectBuild(mockExit);
  });

  it("allows build when no comparison is returned", async () => {
    process.env.VERCEL = "1";
    process.env.VERCEL_GIT_PREVIOUS_SHA = "";
    process.env.VERCEL_GIT_COMMIT_REF = "my-branch";
    turboIgnore({
      args: {
        workspace: "test-app",
        fallback: "false",
        directory: "__fixtures__/app",
      },
    });
    expect(mockConsole.log).toHaveBeenNthCalledWith(
      3,
      "≫  ",
      'no previous deployments found for "test-app" on "my-branch".'
    );
    expectBuild(mockExit);
  });

  it("skips build for `previousDeploy` comparison with no changes", async () => {
    process.env.VERCEL = "1";
    process.env.VERCEL_GIT_PREVIOUS_SHA = "last-deployed-sha";
    process.env.VERCEL_GIT_COMMIT_REF = "my-branch";
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            null,
            '{"packages":[],"tasks":[]}',
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });
    turboIgnore({
      args: {
        fallback: "false",
        directory: "__fixtures__/app",
      },
    });
    validateLogs(
      [
        "Using Turborepo to determine if this project is affected by the commit...\n",
        'inferred "test-app" as workspace from "package.json"',
        `found previous deployment ("last-deployed-sha") for \"test-app\" on \"my-branch\"`,
        "analyzing results of `npx turbo run build --filter=test-app...[last-deployed-sha] --dry=json`",
        "this project and its dependencies are not affected",
        "ignoring the change",
      ],
      mockConsole.log
    );

    expectIgnore(mockExit);
    mockExec.mockRestore();
  });

  it("allows build for `previousDeploy` comparison with changes", async () => {
    process.env.VERCEL = "1";
    process.env.VERCEL_GIT_PREVIOUS_SHA = "last-deployed-sha";
    process.env.VERCEL_GIT_COMMIT_REF = "my-branch";
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            null,
            '{"packages":["test-app"],"tasks":[]}',
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });
    turboIgnore({
      args: {
        fallback: "false",
        directory: "__fixtures__/app",
      },
    });
    validateLogs(
      [
        "Using Turborepo to determine if this project is affected by the commit...\n",
        'inferred "test-app" as workspace from "package.json"',
        'found previous deployment ("last-deployed-sha") for "test-app" on "my-branch"',
        "analyzing results of `npx turbo run build --filter=test-app...[last-deployed-sha] --dry=json`",
        'this commit affects "test-app"',
        "proceeding with deployment",
      ],
      mockConsole.log
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("allows build for `previousDeploy` comparison with single dependency change", async () => {
    process.env.VERCEL = "1";
    process.env.VERCEL_GIT_PREVIOUS_SHA = "last-deployed-sha";
    process.env.VERCEL_GIT_COMMIT_REF = "my-branch";
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            null,
            '{"packages":["test-app", "ui"],"tasks":[]}',
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });
    turboIgnore({
      args: {
        fallback: "false",
        directory: "__fixtures__/app",
      },
    });
    validateLogs(
      [
        "Using Turborepo to determine if this project is affected by the commit...\n",
        'inferred "test-app" as workspace from "package.json"',
        'found previous deployment ("last-deployed-sha") for "test-app" on "my-branch"',
        "analyzing results of `npx turbo run build --filter=test-app...[last-deployed-sha] --dry=json`",
        'this commit affects "test-app" and 1 dependency (ui)',
        "proceeding with deployment",
      ],
      mockConsole.log
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("allows build for `previousDeploy` comparison with multiple dependency changes", async () => {
    process.env.VERCEL = "1";
    process.env.VERCEL_GIT_PREVIOUS_SHA = "last-deployed-sha";
    process.env.VERCEL_GIT_COMMIT_REF = "my-branch";
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            null,
            '{"packages":["test-app", "ui", "tsconfig"],"tasks":[]}',
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });
    turboIgnore({
      args: {
        fallback: "false",
        directory: "__fixtures__/app",
      },
    });
    validateLogs(
      [
        "Using Turborepo to determine if this project is affected by the commit...\n",
        'inferred "test-app" as workspace from "package.json"',
        'found previous deployment ("last-deployed-sha") for "test-app" on "my-branch"',
        "analyzing results of `npx turbo run build --filter=test-app...[last-deployed-sha] --dry=json`",
        'this commit affects "test-app" and 2 dependencies (ui, tsconfig)',
        "proceeding with deployment",
      ],
      mockConsole.log
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("throws error and allows build when json cannot be parsed", async () => {
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(null, "stdout", "stderr") as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });

    turboIgnore({
      args: {
        directory: "__fixtures__/app",
      },
    });

    expect(mockExec).toHaveBeenCalledWith(
      "npx turbo run build --filter=test-app...[HEAD^] --dry=json",
      expect.anything(),
      expect.anything()
    );
    validateLogs(
      [
        "failed to parse JSON output from `npx turbo run build --filter=test-app...[HEAD^] --dry=json`.",
      ],
      mockConsole.error
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });

  it("throws error and allows build when stdout is null", async () => {
    const mockExec = jest
      .spyOn(child_process, "exec")
      .mockImplementation((command, options, callback) => {
        if (callback) {
          return callback(
            null,
            null as unknown as string,
            "stderr"
          ) as unknown as ChildProcess;
        }
        return {} as unknown as ChildProcess;
      });

    turboIgnore({
      args: {
        directory: "__fixtures__/app",
      },
    });

    expect(mockExec).toHaveBeenCalledWith(
      "npx turbo run build --filter=test-app...[HEAD^] --dry=json",
      expect.anything(),
      expect.anything()
    );
    validateLogs(
      [
        "failed to parse JSON output from `npx turbo run build --filter=test-app...[HEAD^] --dry=json`.",
      ],
      mockConsole.error
    );

    expectBuild(mockExit);
    mockExec.mockRestore();
  });
});
