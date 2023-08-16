# Setup
  $ . ${TESTDIR}/../_helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd) persistent_dependencies/9-cross-workspace-nested

// Workspace Graph
// - No workspace dependencies
// 
// Task Graph:
//
// workspace-a#build
// └── workspace-b#build
// 		 └── workspace-c#build
// 		 		 └── workspace-z#dev // this one is persistent
//
  $ ${TURBO} run build
   ERROR  run failed: error preparing engine: Invalid persistent task configuration:
  "app-z#dev" is a persistent task, "app-c#build" cannot depend on it
  Turbo error: error preparing engine: Invalid persistent task configuration:
  "app-z#dev" is a persistent task, "app-c#build" cannot depend on it
  [1]
