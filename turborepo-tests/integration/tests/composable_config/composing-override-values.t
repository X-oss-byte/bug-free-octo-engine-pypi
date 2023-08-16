Setup
  $ . ${TESTDIR}/../../../helpers/setup.sh
  $ . ${TESTDIR}/../_helpers/setup_monorepo.sh $(pwd) composable_config

# The override-values-task task in the root turbo.json has ALL the config. The workspace config
# defines the task and overrides all the keys. The tests below use `override-values-task` to assert that:
# - `outputs`, `inputs`, `env`, and `outputMode` are overriden from the root config.

# 1. First run, assert that the right `outputs` are cached.
  $ ${TURBO} run override-values-task --filter=override-values > tmp.log
  $ cat tmp.log
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache miss, executing a5d19f0c5c1718f9
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
  $ HASH=$(cat tmp.log | grep -E "override-values:override-values-task.* executing .*" | awk '{print $5}')
  $ tar -tf $TARGET_DIR/node_modules/.cache/turbo/$HASH.tar.zst;
  apps/override-values/.turbo/turbo-override-values-task.log
  apps/override-values/lib/
  apps/override-values/lib/.keep
  apps/override-values/lib/bar.min.txt

2. Run again and assert cache hit, and that full output is displayed
  $ ${TURBO} run override-values-task --filter=override-values
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache hit, replaying output a5d19f0c5c1718f9
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    1 cached, 1 total
    Time:\s*[\.0-9]+m?s >>> FULL TURBO (re)
  
3. Change input file and assert cache miss
  $ echo "more text" >> $TARGET_DIR/apps/override-values/src/bar.txt
  $ ${TURBO} run override-values-task --filter=override-values
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache miss, executing f9463ba932d00b3e
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
3a. Change a file that is declared as input in root config, and assert cache hit and FULL TURBO
  $ echo "more text" >> $TARGET_DIR/apps/override-values/src/foo.txt
  $ ${TURBO} run override-values-task --filter=override-values
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache hit, replaying output f9463ba932d00b3e
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    1 cached, 1 total
    Time:\s*[\.0-9]+m?s >>> FULL TURBO (re)
  
4. Set env var and assert cache miss, and that hash is different from above
  $ OTHER_VAR=somevalue ${TURBO} run override-values-task --filter=override-values
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache miss, executing 5507f37052d70f76
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
4a. Set env var that is declared in root config, and assert cache hit and FULL TURBO
  $ OTHER_VAR=somevalue ${TURBO} run override-values-task --filter=override-values
  \xe2\x80\xa2 Packages in scope: override-values (esc)
  \xe2\x80\xa2 Running override-values-task in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  override-values:override-values-task: cache hit, replaying output 5507f37052d70f76
  override-values:override-values-task: 
  override-values:override-values-task: > override-values-task
  override-values:override-values-task: > echo "running override-values-task" > lib/bar.min.txt
  override-values:override-values-task: 
  
   Tasks:    1 successful, 1 total
  Cached:    1 cached, 1 total
    Time:\s*[\.0-9]+m?s >>> FULL TURBO (re)
  
