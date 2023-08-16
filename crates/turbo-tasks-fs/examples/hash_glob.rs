#![feature(trivial_bounds)]
#![feature(once_cell)]
#![feature(min_specialization)]

use std::{
    collections::BTreeMap,
    env::current_dir,
    io::Read,
    time::{Duration, Instant},
};

use anyhow::Result;
use sha2::{Digest, Sha256};
use turbo_tasks::{primitives::StringVc, util::FormatDuration, NothingVc, TurboTasks, UpdateInfo};
use turbo_tasks_fs::{
    glob::GlobVc, register, DirectoryEntry, DiskFileSystemVc, FileContent, FileSystem,
    FileSystemPathVc, FileSystemVc, ReadGlobResultVc,
};
use turbo_tasks_memory::MemoryBackend;

#[tokio::main]
async fn main() -> Result<()> {
    register();
    include!(concat!(env!("OUT_DIR"), "/register_example_hash_glob.rs"));

    let tt = TurboTasks::new(MemoryBackend::default());
    let start = Instant::now();

    let task = tt.spawn_root_task(|| {
        Box::pin(async {
            let root = current_dir().unwrap().to_str().unwrap().to_string();
            let disk_fs = DiskFileSystemVc::new("project".to_string(), root);
            disk_fs.await?.start_watching()?;

            // Smart Pointer cast
            let fs: FileSystemVc = disk_fs.into();
            let input = fs.root().join("crates");
            let glob = GlobVc::new("**/*.rs");
            let glob_result = input.read_glob(glob, true);
            let dir_hash = hash_glob_result(glob_result);
            print_hash(dir_hash);
            Ok(NothingVc::new().into())
        })
    });
    tt.wait_task_completion(task, true).await.unwrap();
    println!("done in {}", FormatDuration(start.elapsed()));

    loop {
        let UpdateInfo {
            duration, tasks, ..
        } = tt
            .get_or_wait_aggregated_update_info(Duration::from_millis(100))
            .await;
        println!("updated {} tasks in {}", tasks, FormatDuration(duration));
    }
}

#[turbo_tasks::function]
pub fn empty_string() -> StringVc {
    StringVc::cell("".to_string())
}

#[turbo_tasks::function]
async fn print_hash(dir_hash: StringVc) -> Result<()> {
    println!("DIR HASH: {}", dir_hash.await?.as_str());
    Ok(())
}

#[turbo_tasks::function]
async fn hash_glob_result(result: ReadGlobResultVc) -> Result<StringVc> {
    let result = result.await?;
    let mut hashes = BTreeMap::new();
    for (name, entry) in result.results.iter() {
        if let DirectoryEntry::File(path) = entry {
            hashes.insert(name, hash_file(*path).await?.clone_value());
        }
    }
    for (name, result) in result.inner.iter() {
        let hash = hash_glob_result(*result).await?;
        if !hash.is_empty() {
            hashes.insert(name, hash.clone_value());
        }
    }
    if hashes.is_empty() {
        return Ok(empty_string());
    }
    let hash = hash_content(
        &mut hashes
            .into_values()
            .collect::<Vec<String>>()
            .join(",")
            .as_bytes(),
    );
    Ok(hash)
}

#[turbo_tasks::function]
async fn hash_file(file_path: FileSystemPathVc) -> Result<StringVc> {
    let content = file_path.read().await?;
    Ok(match &*content {
        FileContent::Content(file) => hash_content(&mut file.read()),
        FileContent::NotFound => {
            // report error
            StringVc::cell("".to_string())
        }
    })
}

fn hash_content<R: Read>(content: &mut R) -> StringVc {
    let mut hasher = Sha256::new();
    let mut buf = [0; 1024];
    while let Ok(size) = content.read(&mut buf) {
        hasher.update(&buf[0..size]);
    }
    let result = format!("{:x}", hasher.finalize());

    StringVc::cell(result)
}
