Setup
  $ . ${TESTDIR}/../setup.sh
  $ . ${TESTDIR}/setup.sh $(pwd)

Verbosity level 1
  $ ${TURBO} build -v --filter=util --force
  [-0-9:.TWZ+]+ \[INFO]  turbo: skipping turbod since we appear to be in a non-interactive context (re)
  \xe2\x80\xa2 Packages in scope: util (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  util:build: cache bypass, force executing 1a3651e1149bfaf7
  util:build: 
  util:build: > build
  util:build: > echo 'building'
  util:build: 
  util:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
  $ ${TURBO} build --verbosity=1 --filter=util --force
  [-0-9:.TWZ+]+ \[INFO]  turbo: skipping turbod since we appear to be in a non-interactive context (re)
  \xe2\x80\xa2 Packages in scope: util (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  util:build: cache bypass, force executing 1a3651e1149bfaf7
  util:build: 
  util:build: > build
  util:build: > echo 'building'
  util:build: 
  util:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  

Verbosity level 2
  $ ${TURBO} build -vv --filter=util --force
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Global turbo version: .* (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Repository Root: .* (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Running command as global turbo (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::cli: pkg_inference_root set to "" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: Found go binary at "[\-\w\/]+" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: build tag: (go|rust) (re)
  [-0-9:.TWZ+]+ \[INFO]  turbo: skipping turbod since we appear to be in a non-interactive context (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: global hash env vars: vars=\["SOME_ENV_VAR", "VERCEL_ANALYTICS_ID"] (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: global hash: value=23db54addfd92f5b (re)
  [-0-9:.TWZ+]+ |[DEBUG] turbo: local cache folder: path="" (re)
  \xe2\x80\xa2 Packages in scope: util (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: task hash env vars for util:build: vars=\["NODE_ENV="] (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: start (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: task hash: value=1a3651e1149bfaf7 (re)
  util:build: cache bypass, force executing 1a3651e1149bfaf7
  util:build: 
  util:build: > build
  util:build: > echo 'building'
  util:build: 
  util:build: building
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: caching output: outputs="{\[packages/util/.turbo/turbo-build.log] \[]}" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: done: status=complete duration=[\.0-9]+m?s (re)
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
  $ ${TURBO} build --verbosity=2 --filter=util --force
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Global turbo version: .* (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Repository Root: .* (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::shim: Running command as global turbo (re)
  [-0-9:.TWZ+]+ \[DEBUG] turborepo_lib::cli: pkg_inference_root set to "" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: Found go binary at "[\-\w\/]+" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: build tag: (go|rust) (re)
  [-0-9:.TWZ+]+ \[INFO]  turbo: skipping turbod since we appear to be in a non-interactive context (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: global hash env vars: vars=\["SOME_ENV_VAR", "VERCEL_ANALYTICS_ID"] (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: global hash: value=23db54addfd92f5b (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: local cache folder: path="" (re)
  \xe2\x80\xa2 Packages in scope: util (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: task hash env vars for util:build: vars=\["NODE_ENV="] (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: start (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo: task hash: value=1a3651e1149bfaf7 (re)
  util:build: cache bypass, force executing 1a3651e1149bfaf7
  util:build: 
  util:build: > build
  util:build: > echo 'building'
  util:build: 
  util:build: building
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: caching output: outputs="{\[packages/util/.turbo/turbo-build.log] \[]}" (re)
  [-0-9:.TWZ+]+ \[DEBUG] turbo.: done: status=complete duration=[\.0-9]+m?s (re)
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
 


Make sure users can only use one verbosity flag
  $ ${TURBO} build -v --verbosity=1
  ERROR the argument '-v...' cannot be used with '--verbosity <COUNT>'
  
  Usage: turbo [OPTIONS] [COMMAND]
  
  For more information, try '--help'.
  
  [1]
