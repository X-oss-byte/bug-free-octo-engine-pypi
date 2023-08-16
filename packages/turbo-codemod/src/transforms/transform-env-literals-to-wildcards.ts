import path from "path";
import fs from "fs-extra";
import { getTurboConfigs } from "@turbo/utils";
import type { EnvWildcard, Schema as TurboJsonSchema } from "@turbo/types";

import type { TransformerArgs } from "../types";
import getTransformerHelpers from "../utils/getTransformerHelpers";
import { TransformerResults } from "../runner";
import { RootSchema } from "@turbo/types/src/types/config";

// transformer details
const TRANSFORMER = "transform-env-literals-to-wildcards";
const DESCRIPTION = "Rewrite env fields to distinguish wildcards from literals";
const INTRODUCED_IN = "1.10.0";

// Rewriting of environment variable names.
const asteriskLiteral = new RegExp("\\*", "g");
function transformEnvVarName(envVarName: string): EnvWildcard {
  let output = envVarName;

  // Transform leading !
  if (envVarName[0] === "!") {
    output = `\\${output}`;
  }

  // Transform literal asterisks
  output = output.replace(asteriskLiteral, "\\*");

  return output;
}

function migrateRootConfig(config: RootSchema) {
  let { globalEnv, globalPassThroughEnv } = config;

  if (Array.isArray(globalEnv)) {
    config.globalEnv = globalEnv.map(transformEnvVarName);
  }
  if (Array.isArray(globalPassThroughEnv)) {
    config.globalPassThroughEnv = globalPassThroughEnv.map(transformEnvVarName);
  }

  return migrateTaskConfigs(config);
}

function migrateTaskConfigs(config: TurboJsonSchema) {
  for (const [_, taskDef] of Object.entries(config.pipeline)) {
    let { env, passThroughEnv } = taskDef;

    if (Array.isArray(env)) {
      taskDef.env = env.map(transformEnvVarName);
    }
    if (Array.isArray(passThroughEnv)) {
      taskDef.passThroughEnv = passThroughEnv.map(transformEnvVarName);
    }
  }

  return config;
}

export function transformer({
  root,
  options,
}: TransformerArgs): TransformerResults {
  const { log, runner } = getTransformerHelpers({
    transformer: TRANSFORMER,
    rootPath: root,
    options,
  });

  // If `turbo` key is detected in package.json, require user to run the other codemod first.
  const packageJsonPath = path.join(root, "package.json");
  // package.json should always exist, but if it doesn't, it would be a silly place to blow up this codemod
  let packageJSON = {};

  try {
    packageJSON = fs.readJSONSync(packageJsonPath);
  } catch (e) {
    // readJSONSync probably failed because the file doesn't exist
  }

  if ("turbo" in packageJSON) {
    return runner.abortTransform({
      reason:
        '"turbo" key detected in package.json. Run `npx @turbo/codemod transform create-turbo-config` first',
    });
  }

  log.info("Rewriting env vars to support wildcards");
  const turboConfigPath = path.join(root, "turbo.json");
  if (!fs.existsSync(turboConfigPath)) {
    return runner.abortTransform({
      reason: `No turbo.json found at ${root}. Is the path correct?`,
    });
  }

  const turboJson: RootSchema = fs.readJsonSync(turboConfigPath);
  runner.modifyFile({
    filePath: turboConfigPath,
    after: migrateRootConfig(turboJson),
  });

  // find and migrate any workspace configs
  const allTurboJsons = getTurboConfigs(root);
  allTurboJsons.forEach((workspaceConfig) => {
    const { config, turboConfigPath, isRootConfig } = workspaceConfig;
    if (!isRootConfig) {
      runner.modifyFile({
        filePath: turboConfigPath,
        after: migrateTaskConfigs(config),
      });
    }
  });

  return runner.finish();
}

const transformerMeta = {
  name: `${TRANSFORMER}: ${DESCRIPTION}`,
  value: TRANSFORMER,
  introducedIn: INTRODUCED_IN,
  transformer,
};

export default transformerMeta;
