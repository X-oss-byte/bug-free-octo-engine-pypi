use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use log::debug;
use notify::{Config, RecommendedWatcher, Watcher};
use tokio::{join, task::JoinHandle, try_join};

/// Tracks changes for a given hash. A hash is a unique identifier for a set of
/// files. Given a hash and a set of globs to track, this will watch for file
/// changes and allow the user to query for changes.
///
/// TODO: consider replacing with dashmap
#[derive(Default, Clone)]
pub struct GlobWatcher {
    path: PathBuf,
    hash_globs: Arc<Mutex<HashMap<String, Glob>>>,
    glob_status: Arc<Mutex<HashMap<String, HashSet<String>>>>,
}

#[derive(Clone)]
pub struct Glob {
    include: HashSet<String>,
    exclude: HashSet<String>,
}

impl GlobWatcher {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }

    pub async fn watch(&self) -> () {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);

        let watch_path = self.path.clone();
        let watcher = tokio::task::spawn_blocking(move || {
            let mut watcher = RecommendedWatcher::new(
                move |res| {
                    debug!("watcher event: {:?}", res);
                    futures::executor::block_on(async {
                        tx.send(res).await.expect("works");
                    })
                },
                Config::default(),
            )
            .expect("works");

            debug!("watching {:?}", watch_path);
            watcher
                .watch(&watch_path, notify::RecursiveMode::Recursive)
                .unwrap();
        });

        let processor = async {
            while let Some(Ok(res)) = rx.recv().await {
                debug!("watcher event: {:?}", res);
            }
        };

        join!(watcher, processor);
    }

    pub fn watch_globs(&self, hash: String, include: HashSet<String>, exclude: HashSet<String>) {
        let mut map = self.glob_status.lock().expect("no panic");
        map.entry(hash.clone()).or_default().extend(include.clone());

        let mut map = self.hash_globs.lock().expect("no panic");
        map.insert(hash, Glob { include, exclude });
    }

    /// Given a hash and a set of candidates, return the subset of candidates
    /// that have changed.
    pub fn changed_globs<'a>(&'a self, hash: &str, candidates: HashSet<String>) -> HashSet<String> {
        let globs = self.hash_globs.lock().unwrap();
        match globs.get(hash) {
            Some(glob) => candidates
                .into_iter()
                .filter(|c| glob.include.contains(c))
                .collect(),
            None => candidates.into_iter().collect(),
        }
    }
}
