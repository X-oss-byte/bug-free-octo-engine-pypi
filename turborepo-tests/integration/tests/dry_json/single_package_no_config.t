Setup
  $ . ${TESTDIR}/../../../helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd) single_package
  $ rm turbo.json
  $ git commit -am "Delete turbo config" --quiet

  $ ${TURBO} run build --dry=json
  {
    "id": "[a-zA-Z0-9]+", (re)
    "version": "0",
    "turboVersion": "[a-z0-9\.-]+", (re)
    "globalCacheInputs": {
      "rootKey": "Buffalo buffalo Buffalo buffalo buffalo buffalo Buffalo buffalo",
      "files": {
        "package-lock.json": "8db0df575e6509336a6719094b63eb23d2c649c1",
        "package.json": "185771929d92c3865ce06c863c07d357500d3364"
      },
      "hashOfExternalDependencies": "",
      "rootPipeline": {
        "//#build": {
          "outputs": [],
          "cache": false,
          "dependsOn": [],
          "inputs": [],
          "outputMode": "full",
          "env": [],
          "persistent": false
        }
      }
    },
    "envMode": "infer",
    "frameworkInference": true,
    "tasks": [
      {
        "taskId": "build",
        "task": "build",
        "hash": "5b5ae44052e3d624",
        "inputs": {
          ".gitignore": "6f23ff6842b5526da43ab38f4a5bf3b0158eeb42",
          "package-lock.json": "8db0df575e6509336a6719094b63eb23d2c649c1",
          "package.json": "185771929d92c3865ce06c863c07d357500d3364",
          "somefile.txt": "45b983be36b73c0788dc9cbcb76cbb80fc7bb057"
        },
        "hashOfExternalDependencies": "",
        "cache": {
          "local": false,
          "remote": false,
          "status": "MISS",
          "timeSaved": 0
        },
        "command": "echo 'building' \u003e foo",
        "cliArguments": [],
        "outputs": null,
        "excludedOutputs": null,
        "logFile": ".turbo/turbo-build.log",
        "dependencies": [],
        "dependents": [],
        "resolvedTaskDefinition": {
          "outputs": [],
          "cache": false,
          "dependsOn": [],
          "inputs": [],
          "outputMode": "full",
          "env": [],
          "persistent": false
        },
        "expandedOutputs": [],
        "framework": "\u003cNO FRAMEWORK DETECTED\u003e",
        "envMode": "loose",
        "environmentVariables": {
          "configured": [],
          "inferred": [],
          "global": [
            "VERCEL_ANALYTICS_ID="
          ],
          "passthrough": null,
          "globalPassthrough": null
        }
      }
    ],
    "user": ".*", (re)
    "scm": {
      "type": "git",
      "sha": "[a-z0-9]+", (re)
      "branch": ".+" (re)
    }
  }
