#![feature(error_generic_member_access)]
#![feature(provide_any)]
#![feature(assert_matches)]
#![feature(box_patterns)]
#![deny(clippy::all)]

mod async_cache;
pub mod cache_archive;
pub mod fs;
pub mod http;
mod multiplexer;
pub mod signature_authentication;
#[cfg(test)]
mod test_cases;

use std::{backtrace, backtrace::Backtrace};

pub use async_cache::AsyncCache;
use camino::Utf8Path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::signature_authentication::SignatureError;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error, #[backtrace] backtrace::Backtrace),
    #[error(
        "artifact verification failed: Downloaded artifact is missing required x-artifact-tag \
         header"
    )]
    ArtifactTagMissing(#[backtrace] Backtrace),
    #[error("invalid artifact verification tag")]
    InvalidTag(#[backtrace] Backtrace),
    #[error("cannot untar file to {0}")]
    InvalidFilePath(String, #[backtrace] Backtrace),
    #[error("artifact verification failed: {0}")]
    ApiClientError(Box<turborepo_api_client::Error>, #[backtrace] Backtrace),
    #[error("signing artifact failed: {0}")]
    SignatureError(#[from] SignatureError, #[backtrace] Backtrace),
    #[error("invalid duration")]
    InvalidDuration(#[backtrace] Backtrace),
    #[error("Invalid file path: {0}")]
    PathError(#[from] turbopath::PathError, #[backtrace] Backtrace),
    #[error("links in the cache are cyclic")]
    CycleDetected(#[backtrace] Backtrace),
    #[error("Invalid file path, link target does not exist: {0}")]
    LinkTargetDoesNotExist(String, #[backtrace] Backtrace),
    #[error("Invalid tar, link target does not exist on header")]
    LinkTargetNotOnHeader(#[backtrace] Backtrace),
    #[error("attempted to restore unsupported file type: {0:?}")]
    RestoreUnsupportedFileType(tar::EntryType, #[backtrace] Backtrace),
    // We don't pass the `FileType` because there's no simple
    // way to display it nicely.
    #[error("attempted to create unsupported file type")]
    CreateUnsupportedFileType(#[backtrace] Backtrace),
    #[error("tar file is malformed")]
    MalformedTar(#[backtrace] Backtrace),
    #[error("file name is not Windows-safe: {0}")]
    WindowsUnsafeName(String, #[backtrace] Backtrace),
    #[error("tar attempts to write outside of directory: {0}")]
    LinkOutsideOfDirectory(String, #[backtrace] Backtrace),
    #[error("Invalid cache metadata file")]
    InvalidMetadata(serde_json::Error, #[backtrace] Backtrace),
    #[error("Failed to write cache metadata file")]
    MetadataWriteFailure(serde_json::Error, #[backtrace] Backtrace),
    #[error("Cache miss")]
    CacheMiss,
}

impl From<turborepo_api_client::Error> for CacheError {
    fn from(value: turborepo_api_client::Error) -> Self {
        CacheError::ApiClientError(Box::new(value), Backtrace::capture())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CacheSource {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheResponse {
    source: CacheSource,
    time_saved: u32,
}

#[derive(Debug, Default)]
pub struct CacheOpts<'a> {
    pub override_dir: Option<&'a Utf8Path>,
    pub skip_remote: bool,
    pub skip_filesystem: bool,
    pub workers: u32,
    pub remote_cache_opts: Option<RemoteCacheOpts>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCacheOpts {
    team_id: String,
    signature: bool,
}
