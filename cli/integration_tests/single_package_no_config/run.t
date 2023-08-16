Setup
  $ . ${TESTDIR}/../setup.sh
  $ . ${TESTDIR}/setup.sh $(pwd)

Check
  $ ${TURBO} run build --single-package
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Running build (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  build: cache bypass, force executing c7223f212c321d3b
  build: 
  build: > build
  build: > echo 'building'
  build: 
  build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
Run a second time, verify no caching because there is no config
  $ ${TURBO} run build --single-package
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Running build (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  build: cache bypass, force executing c7223f212c321d3b
  build: 
  build: > build
  build: > echo 'building'
  build: 
  build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  