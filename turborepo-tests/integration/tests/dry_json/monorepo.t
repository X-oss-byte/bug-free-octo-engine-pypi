Setup
  $ . ${TESTDIR}/../../../helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd)

# Save JSON to tmp file so we don't need to keep re-running the build
  $ ${TURBO} run build --dry=json > tmpjson.log

# test with a regex that captures what release we usually have (1.x.y or 1.a.b-canary.c)
  $ cat tmpjson.log | jq .turboVersion
  "[a-z0-9\.-]+" (re)

  $ cat tmpjson.log | jq .globalCacheInputs
  {
    "rootKey": "You don't understand! I coulda had class. I coulda been a contender. I could've been somebody, instead of a bum, which is what I am.",
    "files": {
      "foo.txt": "eebae5f3ca7b5831e429e947b7d61edd0de69236"
    },
    "hashOfExternalDependencies": "ccab0b28617f1f56",
    "globalDotEnv": null,
    "environmentVariables": {
      "specified": {
        "env": [
          "SOME_ENV_VAR"
        ],
        "passThroughEnv": null
      },
      "configured": [],
      "inferred": [],
      "passthrough": null
    }
  }

  $ cat tmpjson.log | jq 'keys'
  [
    "envMode",
    "frameworkInference",
    "globalCacheInputs",
    "id",
    "monorepo",
    "packages",
    "scm",
    "tasks",
    "turboVersion",
    "user",
    "version"
  ]

# Validate output of my-app#build task
  $ cat tmpjson.log | jq '.tasks | map(select(.taskId == "my-app#build")) | .[0]'
  {
    "taskId": "my-app#build",
    "task": "build",
    "package": "my-app",
    "hash": "0d1e6ee2c143211c",
    "inputs": {
      ".env.local": "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
      "package.json": "6bcf57fd6ff30d1a6f40ad8d8d08e8b940fc7e3b"
    },
    "hashOfExternalDependencies": "ccab0b28617f1f56",
    "cache": {
      "local": false,
      "remote": false,
      "status": "MISS",
      "timeSaved": 0
    },
    "command": "echo 'building'",
    "cliArguments": [],
    "outputs": [
      "apple.json",
      "banana.txt"
    ],
    "excludedOutputs": null,
    "logFile": "apps/my-app/.turbo/turbo-build.log",
    "directory": "apps/my-app",
    "dependencies": [],
    "dependents": [],
    "resolvedTaskDefinition": {
      "outputs": [
        "apple.json",
        "banana.txt"
      ],
      "cache": true,
      "dependsOn": [],
      "inputs": [],
      "outputMode": "full",
      "persistent": false,
      "env": [],
      "passThroughEnv": null,
      "dotEnv": [
        ".env.local"
      ]
    },
    "expandedOutputs": [],
    "framework": "<NO FRAMEWORK DETECTED>",
    "envMode": "loose",
    "environmentVariables": {
      "specified": {
        "env": [],
        "passThroughEnv": null
      },
      "configured": [],
      "inferred": [],
      "passthrough": null
    },
    "dotEnv": [
      ".env.local"
    ]
  }

# Validate output of util#build task
  $ cat tmpjson.log | jq '.tasks | map(select(.taskId == "util#build")) | .[0]'
  {
    "taskId": "util#build",
    "task": "build",
    "package": "util",
    "hash": "76ab904c7ecb2d51",
    "inputs": {
      "package.json": "4d57bb28c9967640d812981198a743b3188f713e"
    },
    "hashOfExternalDependencies": "ccab0b28617f1f56",
    "cache": {
      "local": false,
      "remote": false,
      "status": "MISS",
      "timeSaved": 0
    },
    "command": "echo 'building'",
    "cliArguments": [],
    "outputs": null,
    "excludedOutputs": null,
    "logFile": "packages/util/.turbo/turbo-build.log",
    "directory": "packages/util",
    "dependencies": [],
    "dependents": [],
    "resolvedTaskDefinition": {
      "outputs": [],
      "cache": true,
      "dependsOn": [],
      "inputs": [],
      "outputMode": "full",
      "persistent": false,
      "env": [
        "NODE_ENV"
      ],
      "passThroughEnv": null,
      "dotEnv": null
    },
    "expandedOutputs": [],
    "framework": "<NO FRAMEWORK DETECTED>",
    "envMode": "loose",
    "environmentVariables": {
      "specified": {
        "env": [
          "NODE_ENV"
        ],
        "passThroughEnv": null
      },
      "configured": [],
      "inferred": [],
      "passthrough": null
    },
    "dotEnv": null
  }

Run again with NODE_ENV set and see the value in the summary. --filter=util workspace so the output is smaller
  $ NODE_ENV=banana ${TURBO} run build --dry=json --filter=util | jq '.tasks | map(select(.taskId == "util#build")) | .[0].environmentVariables'
  {
    "specified": {
      "env": [
        "NODE_ENV"
      ],
      "passThroughEnv": null
    },
    "configured": [
      "NODE_ENV=b493d48364afe44d11c0165cf470a4164d1e2609911ef998be868d46ade3de4e"
    ],
    "inferred": [],
    "passthrough": null
  }

Tasks that don't exist throw an error
  $ ${TURBO} run doesnotexist --dry=json
   ERROR  run failed: error preparing engine: Could not find the following tasks in project: doesnotexist
  Turbo error: error preparing engine: Could not find the following tasks in project: doesnotexist
  [1]
