/**
 * # Usage:
 *
 * cd cli/ && node new-integration-test my-feature my-test-name
 *
 * This generates:
 *
 * cli/integration_tests/my-feature/
 * |___my-test-name.t
 * |___my-test-name/
 *      |___package.json
 *      |___turbo.json
 *      |___workspace-a/package.json
 *      |___workspace-b/package.json
 *      |___workspace-c/package.json
 *
 * You should then be able to run from the cli/ directory:
 *
 * .cram_env/bin/prysk integration_tests/my-feature
 *
 * and see a test failure. Modify at your own pleasure from there.
 */

import { execSync } from "child_process";
import fs from "fs";

const SETUP_FILE_CONTENTS = `#!/bin/bash

SCRIPT_DIR=$(dirname \${BASH_SOURCE[0]})
TARGET_DIR=$1
MONOREPO_DIR=$2
cp -a \${SCRIPT_DIR}/\${MONOREPO_DIR}/. \${TARGET_DIR}/
\${SCRIPT_DIR}/../setup_git.sh \${TARGET_DIR}
`;

// Feature name is something high level
const featureName = process.argv[2];

// Test name is something more specific. Generates a monorepo with that name and .t file
const testName = process.argv[3] ?? featureName;

console.log("feature name", featureName);
console.log("test name", testName);

// Defaults
const workspaces = ["workspace-a", "workspace-b", "workspace-c"];

// Validations
validateHyphenated(featureName);
validateHyphenated(testName);

createTestDir(featureName); // create integration test dir

console.log("Creating test:");
createTestForFeature(featureName, testName);

function createTestForFeature(fName, tName) {
  // Create monorepo directory
  createDir(`${fName}/${tName}`);
  // create test file
  const testFile = `integration_tests/${fName}/${tName}.t`;
  fs.writeFileSync(testFile, testFileContents(tName));

  const rootPkgJSON = `integration_tests/${fName}/${tName}/package.json`;
  const rootTurboJSON = `integration_tests/${fName}/${tName}/turbo.json`;

  // Create `package.json` and turbo.json files
  fs.writeFileSync(
    rootPkgJSON,
    JSON.stringify(
      {
        name: testName,
        workspaces,
      },
      null,
      2
    )
  );
  fs.writeFileSync(
    rootTurboJSON,
    JSON.stringify(
      {
        pipeline: {
          build: {},
          test: {},
        },
      },
      null,
      2
    )
  );

  // Create workspace directories
  for (const w of workspaces) {
    const wPath = createDir(`${fName}/${tName}/${w}`);
    const contents = { name: w, scripts: { build: `echo 'build ${w}'` } };
    fs.writeFileSync(
      `${wPath}/package.json`,
      JSON.stringify(contents, null, 2)
    );
  }
}

function testFileContents(tName) {
  return `
Setup
  $ . \${TESTDIR}/../setup.sh
  $ . \${TESTDIR\}/setup.sh $(pwd) ${tName}

Test
  $ \${TURBO} run build
  this test fails
`;
}

// ------------------------------------

function createTestDir(name) {
  if (!name) {
    throw new Error("No name passed");
  }

  const path = `integration_tests/${name}`;

  if (fs.existsSync(path)) {
    console.log(`Found test dir for ${name}`);
  } else {
    console.log("Creating", path);
    execSync(`mkdir -p ${path}`);
  }

  fs.writeFileSync(`${path}/setup.sh`, SETUP_FILE_CONTENTS);
}

// Helpers
function createDir(path) {
  const dirPath = `integration_tests/${path}`;
  console.log("Creating", dirPath);
  execSync(`mkdir -p ${dirPath}`);
  return dirPath;
}

function validateHyphenated(str) {
  if (typeof str === "undefined") {
    throw new Error("missing name");
  }

  if (!str) {
    throw new Error("Please provide a test name");
  }

  if (/\s/.test(str)) {
    throw new Error("Please no spaces in the test name");
  }

  if (!/[-_a-z]/.test(str)) {
    throw new Error(
      "Please only lower case characters, hyphens and underscores"
    );
  }
}
