Setup
  $ . ${TESTDIR}/setup.sh

Bad flag should print misuse text
  $ ${TURBO} --bad-flag
  ERROR unexpected argument '--bad-flag' found
  
    note: to pass '--bad-flag' as a value, use '-- --bad-flag'
  
  Usage: turbo [OPTIONS] [COMMAND]
  
  For more information, try '--help'.
  
  [1]

Bad flag with an implied run command should display run flags
  $ ${TURBO} build --bad-flag
  ERROR unexpected argument '--bad-flag' found
  
    note: to pass '--bad-flag' as a value, use '-- --bad-flag'
  
  Usage: turbo <--cache-dir <CACHE_DIR>|--cache-workers <CACHE_WORKERS>|--concurrency <CONCURRENCY>|--continue|--dry-run [<DRY_RUN>]|--single-package|--filter <FILTER>|--force|--global-deps <GLOBAL_DEPS>|--graph [<GRAPH>]|--ignore <IGNORE>|--include-dependencies|--no-cache|--no-daemon|--no-deps|--output-logs <OUTPUT_LOGS>|--only|--parallel|--pkg-inference-root <PKG_INFERENCE_ROOT>|--profile <PROFILE>|--remote-only|--scope <SCOPE>|--since <SINCE>|--summarize [<SUMMARIZE>]|--log-prefix <LOG_PREFIX>|TASKS|PASS_THROUGH_ARGS|--experimental-space-id <EXPERIMENTAL_SPACE_ID>>
  
  For more information, try '--help'.
  
  [1]
