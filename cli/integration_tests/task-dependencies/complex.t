
Setup
  $ . ${TESTDIR}/../_helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd) task_dependencies/complex

# Workspace Graph:
# app-a -> lib-a
#              \
#                -> lib-b -> lib-d
#              /
#     app-b ->
#              \ ->lib-c
# app-a depends on lib-a
# app-b depends on lib-b, lib-c
# lib-a depends on lib-b
# lib-b depends on lib-d

We can scope the run to specific packages
  $ ${TURBO} run build1 --filter=app-b --graph
  
  digraph {
  \tcompound = "true" (esc)
  \tnewrank = "true" (esc)
  \tsubgraph "root" { (esc)
  \t\t"[root] ___ROOT___#build1" -> "[root] ___ROOT___" (esc)
  \t\t"[root] app-b#build1" -> "[root] lib-b#build1" (esc)
  \t\t"[root] app-b#build1" -> "[root] lib-c#build1" (esc)
  \t\t"[root] lib-b#build1" -> "[root] lib-d#build1" (esc)
  \t\t"[root] lib-c#build1" -> "[root] ___ROOT___#build1" (esc)
  \t\t"[root] lib-d#build1" -> "[root] ___ROOT___#build1" (esc)
  \t} (esc)
  }
  


Can't depend on unknown tasks
  $ ${TURBO} run build2
   ERROR  run failed: error preparing engine: Could not find "app-a#custom" in root turbo.json or "app-a" workspace
  Turbo error: error preparing engine: Could not find "app-a#custom" in root turbo.json or "app-a" workspace
  [1]

Can't depend on tasks from unknown packages
  $ ${TURBO} run build3
   ERROR  run failed: error preparing engine: Could not find workspace "unknown" from task "unknown#custom" in project
  Turbo error: error preparing engine: Could not find workspace "unknown" from task "unknown#custom" in project
  [1]


Complex dependency chain
  $ ${TURBO} run test --graph
  
  digraph {
  \tcompound = "true" (esc)
  \tnewrank = "true" (esc)
  \tsubgraph "root" { (esc)
  \t\t"[root] ___ROOT___#build0" -> "[root] ___ROOT___#prepare" (esc)
  \t\t"[root] ___ROOT___#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] app-a#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] app-a#test" -> "[root] app-a#prepare" (esc)
  \t\t"[root] app-a#test" -> "[root] lib-a#build0" (esc)
  \t\t"[root] app-b#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] app-b#test" -> "[root] app-b#prepare" (esc)
  \t\t"[root] app-b#test" -> "[root] lib-b#build0" (esc)
  \t\t"[root] app-b#test" -> "[root] lib-c#build0" (esc)
  \t\t"[root] lib-a#build0" -> "[root] lib-a#prepare" (esc)
  \t\t"[root] lib-a#build0" -> "[root] lib-b#build0" (esc)
  \t\t"[root] lib-a#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-a#test" -> "[root] lib-a#prepare" (esc)
  \t\t"[root] lib-a#test" -> "[root] lib-b#build0" (esc)
  \t\t"[root] lib-b#build0" -> "[root] lib-b#prepare" (esc)
  \t\t"[root] lib-b#build0" -> "[root] lib-d#build0" (esc)
  \t\t"[root] lib-b#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-b#test" -> "[root] lib-b#prepare" (esc)
  \t\t"[root] lib-b#test" -> "[root] lib-d#build0" (esc)
  \t\t"[root] lib-c#build0" -> "[root] ___ROOT___#build0" (esc)
  \t\t"[root] lib-c#build0" -> "[root] lib-c#prepare" (esc)
  \t\t"[root] lib-c#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-c#test" -> "[root] ___ROOT___#build0" (esc)
  \t\t"[root] lib-c#test" -> "[root] lib-c#prepare" (esc)
  \t\t"[root] lib-d#build0" -> "[root] ___ROOT___#build0" (esc)
  \t\t"[root] lib-d#build0" -> "[root] lib-d#prepare" (esc)
  \t\t"[root] lib-d#prepare" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-d#test" -> "[root] ___ROOT___#build0" (esc)
  \t\t"[root] lib-d#test" -> "[root] lib-d#prepare" (esc)
  \t} (esc)
  }
  



Check that --only only runs leaf tasks
  $ ${TURBO} run test --only --graph
  
  digraph {
  \tcompound = "true" (esc)
  \tnewrank = "true" (esc)
  \tsubgraph "root" { (esc)
  \t\t"[root] app-a#test" -> "[root] ___ROOT___" (esc)
  \t\t"[root] app-b#test" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-a#test" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-b#test" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-c#test" -> "[root] ___ROOT___" (esc)
  \t\t"[root] lib-d#test" -> "[root] ___ROOT___" (esc)
  \t} (esc)
  }
  


Can't depend on itself
  $ ${TURBO} run build4
   ERROR  run failed: error preparing engine: Invalid task dependency graph:
  .*#build4 depends on itself (re)
  Turbo error: error preparing engine: Invalid task dependency graph:
  .*#build4 depends on itself (re)
  [1]
