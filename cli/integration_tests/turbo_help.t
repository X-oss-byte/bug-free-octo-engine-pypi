Setup
  $ . ${TESTDIR}/setup.sh

Test help flag
  $ ${TURBO} -h
  The build system that makes ship happen
  
  Usage:
    turbo [command]
  
  Available Commands:
    bin         Get the path to the Turbo binary
    completion  Generate the autocompletion script for the specified shell
    daemon      Runs the Turborepo background daemon
    help        Help about any command
    prune       Prepare a subset of your monorepo.
    run         Run tasks across projects in your monorepo
  
  Flags:
        --api string          Override the endpoint for API calls
        --color               Force color usage in the terminal
        --cpuprofile string   Specify a file to save a cpu profile
        --cwd string          The directory in which to run turbo
        --heap string         Specify a file to save a pprof heap profile
    -h, --help                help for turbo
        --login string        Override the login endpoint
        --no-color            Suppress color usage in the terminal
        --preflight           When enabled, turbo will precede HTTP requests with an OPTIONS request for authorization
        --team string         Set the team slug for API calls
        --token string        Set the auth token for API calls
        --trace string        Specify a file to save a pprof trace
    -v, --verbosity count     verbosity
        --version             version for turbo
  
  Use "turbo [command] --help" for more information about a command.

  $ ${TURBO} --help
  The build system that makes ship happen
  
  Usage:
    turbo [command]
  
  Available Commands:
    bin         Get the path to the Turbo binary
    completion  Generate the autocompletion script for the specified shell
    daemon      Runs the Turborepo background daemon
    help        Help about any command
    prune       Prepare a subset of your monorepo.
    run         Run tasks across projects in your monorepo
  
  Flags:
        --api string          Override the endpoint for API calls
        --color               Force color usage in the terminal
        --cpuprofile string   Specify a file to save a cpu profile
        --cwd string          The directory in which to run turbo
        --heap string         Specify a file to save a pprof heap profile
    -h, --help                help for turbo
        --login string        Override the login endpoint
        --no-color            Suppress color usage in the terminal
        --preflight           When enabled, turbo will precede HTTP requests with an OPTIONS request for authorization
        --team string         Set the team slug for API calls
        --token string        Set the auth token for API calls
        --trace string        Specify a file to save a pprof trace
    -v, --verbosity count     verbosity
        --version             version for turbo
  
  Use "turbo [command] --help" for more information about a command.

Test help flag for shim
  $ ${SHIM} -h
  turbo 
  The build system that makes ship happen
  
  USAGE:
      turbo [OPTIONS] [TASKS]... [SUBCOMMAND]
  
  ARGS:
      <TASKS>...    
  
  OPTIONS:
          --api <API>                    Override the endpoint for API calls
          --color                        Force color usage in the terminal
          --cpu-profile <CPU_PROFILE>    Specify a file to save a cpu profile
          --cwd <CWD>                    The directory in which to run turbo
      -h, --help                         
          --heap <HEAP>                  Specify a file to save a pprof heap profile
          --login <LOGIN>                Override the login endpoint
          --no-color                     Suppress color usage in the terminal
          --preflight                    When enabled, turbo will precede HTTP requests with an
                                         OPTIONS request for authorization
          --team <TEAM>                  Set the team slug for API calls
          --token <TOKEN>                Set the auth token for API calls
          --trace <TRACE>                Specify a file to save a pprof trace
      -v, --verbosity <VERBOSITY>        verbosity
          --version                      
  
  SUBCOMMANDS:
      bin           Get the path to the Turbo binary
      completion    Generate the autocompletion script for the specified shell
      daemon        Runs the Turborepo background daemon
      help          Help about any command
      link          Link your local directory to a Vercel organization and enable remote caching
      login         Login to your Vercel account
      logout        Logout to your Vercel account
      prune         Prepare a subset of your monorepo
      run           Run tasks across projects in your monorepo
      unlink        Unlink the current directory from your Vercel organization and disable Remote
                        Caching






  $ ${SHIM} --help
  turbo 
  The build system that makes ship happen
  
  USAGE:
      turbo [OPTIONS] [TASKS]... [SUBCOMMAND]
  
  ARGS:
      <TASKS>...    
  
  OPTIONS:
          --api <API>                    Override the endpoint for API calls
          --color                        Force color usage in the terminal
          --cpu-profile <CPU_PROFILE>    Specify a file to save a cpu profile
          --cwd <CWD>                    The directory in which to run turbo
      -h, --help                         
          --heap <HEAP>                  Specify a file to save a pprof heap profile
          --login <LOGIN>                Override the login endpoint
          --no-color                     Suppress color usage in the terminal
          --preflight                    When enabled, turbo will precede HTTP requests with an
                                         OPTIONS request for authorization
          --team <TEAM>                  Set the team slug for API calls
          --token <TOKEN>                Set the auth token for API calls
          --trace <TRACE>                Specify a file to save a pprof trace
      -v, --verbosity <VERBOSITY>        verbosity
          --version                      
  
  SUBCOMMANDS:
      bin           Get the path to the Turbo binary
      completion    Generate the autocompletion script for the specified shell
      daemon        Runs the Turborepo background daemon
      help          Help about any command
      link          Link your local directory to a Vercel organization and enable remote caching
      login         Login to your Vercel account
      logout        Logout to your Vercel account
      prune         Prepare a subset of your monorepo
      run           Run tasks across projects in your monorepo
      unlink        Unlink the current directory from your Vercel organization and disable Remote
                        Caching

Test help flag for shim's link command
  $ ${SHIM} link -h
  link 
  Link your local directory to a Vercel organization and enable remote caching
  
  USAGE:
      link [OPTIONS]
  
  OPTIONS:
      -h, --help
              help for link
  
          --no-gitignore
              Do not create or modify .gitignore (default false)

Test help flag for shim's unlink command
  $ ${SHIM} unlink -h
  unlink 
  Unlink the current directory from your Vercel organization and disable Remote Caching
  
  USAGE:
      unlink
  
  OPTIONS:
      -h, --help
              Help flag

Test help flag for shim's login command
  $ ${SHIM} login -h
  login 
  Login to your Vercel account
  
  USAGE:
      login [OPTIONS]
  
  OPTIONS:
      -h, --help
              Help flag
  
          --sso-team <SSO_TEAM>
              

Test help flag for shim's logout command
  $ ${SHIM} logout -h
  logout 
  Logout to your Vercel account
  
  USAGE:
      logout
  
  OPTIONS:
      -h, --help
              Help flag
