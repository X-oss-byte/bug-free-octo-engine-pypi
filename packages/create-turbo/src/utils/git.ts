import { execSync } from "child_process";
import path from "path";
import rimraf from "rimraf";

export const DEFAULT_IGNORE = `
# See https://help.github.com/articles/ignoring-files/ for more about ignoring files.

# dependencies
node_modules
.pnp
.pnp.js

# testing
coverage

# misc
.DS_Store
*.pem

# debug
npm-debug.log*
yarn-debug.log*
yarn-error.log*

# turbo
.turbo

# vercel
.vercel
`;

export const GIT_REPO_COMMAND = "git rev-parse --is-inside-work-tree";
export const HG_REPO_COMMAND = "hg --cwd . root";

export function isInGitRepository(): boolean {
  try {
    execSync(GIT_REPO_COMMAND, { stdio: "ignore" });
    return true;
  } catch (_) {}
  return false;
}

export function isInMercurialRepository(): boolean {
  try {
    execSync(HG_REPO_COMMAND, { stdio: "ignore" });
    return true;
  } catch (_) {}
  return false;
}

export function tryGitInit(root: string, message: string): boolean {
  let didInit = false;
  try {
    if (isInGitRepository() || isInMercurialRepository()) {
      return false;
    }

    execSync("git init", { stdio: "ignore" });
    didInit = true;

    execSync("git checkout -b main", { stdio: "ignore" });

    gitCommit(message);
    return true;
  } catch (err) {
    if (didInit) {
      try {
        rimraf.sync(path.join(root, ".git"));
      } catch (_) {}
    }
    return false;
  }
}

export function tryGitCommit(message: string): boolean {
  try {
    gitCommit(message);
    return true;
  } catch (err) {
    return false;
  }
}

function gitCommit(message: string) {
  execSync(
    `git commit --author="Turbobot <turbobot@vercel.com>" -am "${message}"`,
    {
      stdio: "ignore",
    }
  );
}
