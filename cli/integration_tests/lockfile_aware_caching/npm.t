Setup
  $ . ${TESTDIR}/../setup.sh
  $ . ${TESTDIR}/setup.sh $(pwd) npm

Populate cache
  $ ${TURBO} build --filter=a
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: a (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  a:build: cache miss, executing [0-9a-f]+ (re)
  a:build: 
  a:build: > build
  a:build: > echo 'building'
  a:build: 
  a:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
  $ ${TURBO} build --filter=b
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: b (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  b:build: cache miss, executing [0-9a-f]+ (re)
  b:build: 
  b:build: > build
  b:build: > echo 'building'
  b:build: 
  b:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  

Bump dependency for b and rebuild
Only b should have a cache miss
  $ patch package-lock.json package-lock.patch
  patching file package-lock.json
  $ ${TURBO} build  --filter=a
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: a (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  a:build: cache hit, replaying output [0-9a-f]+ (re)
  a:build: 
  a:build: > build
  a:build: > echo 'building'
  a:build: 
  a:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    1 cached, 1 total
    Time:\s*[\.0-9]+m?s >>> FULL TURBO (re)
  

  $ ${TURBO} build  --filter=b
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: b (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  b:build: cache miss, executing [0-9a-f]+ (re)
  b:build: 
  b:build: > build
  b:build: > echo 'building'
  b:build: 
  b:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
 
Bump of root workspace invalidates all packages
  $ patch package-lock.json turbo-bump.patch
  patching file package-lock.json
  $ ${TURBO} build  --filter=a
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: a (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  a:build: cache miss, executing [0-9a-f]+ (re)
  a:build: 
  a:build: > build
  a:build: > echo 'building'
  a:build: 
  a:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
  $ ${TURBO} build  --filter=b
  No local turbo binary found at: .+node_modules/\.bin/turbo (re)
  Running command as global turbo
  \xe2\x80\xa2 Packages in scope: b (esc)
  \xe2\x80\xa2 Running build in 1 packages (esc)
  \xe2\x80\xa2 Remote caching disabled (esc)
  b:build: cache miss, executing [0-9a-f]+ (re)
  b:build: 
  b:build: > build
  b:build: > echo 'building'
  b:build: 
  b:build: building
  
   Tasks:    1 successful, 1 total
  Cached:    0 cached, 1 total
    Time:\s*[\.0-9]+m?s  (re)
  
