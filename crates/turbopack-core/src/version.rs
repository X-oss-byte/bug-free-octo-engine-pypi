use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use turbo_tasks::{
    debug::ValueDebugFormat, primitives::StringVc, trace::TraceRawVcs, IntoTraitRef, TraitRef,
};
use turbo_tasks_fs::{FileContent, FileContentReadRef, LinkType};
use turbo_tasks_hash::{encode_hex, hash_xxh3_hash64};

use crate::asset::{AssetContent, AssetContentReadRef, AssetContentVc};

/// The content of an [Asset] alongside its version.
#[turbo_tasks::value_trait]
pub trait VersionedContent {
    /// The content of the [Asset].
    fn content(&self) -> AssetContentVc;

    /// Get a [`Version`] implementor that contains enough information to
    /// identify and diff a future [`VersionedContent`] against it.
    fn version(&self) -> VersionVc;

    /// Describes how to update the content from an earlier version to the
    /// latest available one.
    async fn update(self_vc: VersionedContentVc, from: VersionVc) -> Result<UpdateVc> {
        // By default, since we can't make any assumptions about the versioning
        // scheme of the content, we ask for a full invalidation, except in the
        // case where versions are the same.
        let to = self_vc.version();
        let from_ref = from.into_trait_ref().await?;
        let to_ref = to.into_trait_ref().await?;

        // Fast path: versions are the same.
        if from_ref == to_ref {
            return Ok(Update::None.into());
        }

        // The fast path might not always work since `self_vc` might have been converted
        // from a `ReadRef` or a `ReadRef`, in which case `self_vc.version()` would
        // return a new `VersionVc`. In this case, we need to compare version ids.
        let from_id = from.id();
        let to_id = to.id();
        let from_id = from_id.await?;
        let to_id = to_id.await?;
        Ok(if *from_id == *to_id {
            Update::None.into()
        } else {
            Update::Total(TotalUpdate { to: to_ref }).into()
        })
    }
}

/// A versioned file content.
#[turbo_tasks::value]
pub struct VersionedAssetContent {
    // We can't store a `FileContentVc` directly because we don't want
    // `VersionedAssetContentVc` to invalidate when the content changes.
    // Otherwise, reading `content` and `version` at two different instants in
    // time might return inconsistent values.
    asset_content: AssetContentReadRef,
}

#[turbo_tasks::value]
#[derive(Clone)]
enum AssetContentSnapshot {
    File(FileContentReadRef),
    Redirect { target: String, link_type: LinkType },
}

#[turbo_tasks::value_impl]
impl VersionedContent for VersionedAssetContent {
    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        (*self.asset_content).clone().cell()
    }

    #[turbo_tasks::function]
    async fn version(&self) -> Result<VersionVc> {
        Ok(FileHashVersionVc::compute(&self.asset_content)
            .await?
            .into())
    }
}

#[turbo_tasks::value_impl]
impl VersionedAssetContentVc {
    #[turbo_tasks::function]
    /// Creates a new [VersionedAssetContentVc] from a [FileContentVc].
    pub async fn new(asset_content: AssetContentVc) -> Result<Self> {
        let asset_content = asset_content.strongly_consistent().await?;
        Ok(Self::cell(VersionedAssetContent { asset_content }))
    }
}

impl From<AssetContent> for VersionedAssetContentVc {
    fn from(asset_content: AssetContent) -> Self {
        VersionedAssetContentVc::new(asset_content.cell())
    }
}

impl From<AssetContentVc> for VersionedAssetContentVc {
    fn from(asset_content: AssetContentVc) -> Self {
        VersionedAssetContentVc::new(asset_content)
    }
}

impl From<AssetContent> for VersionedContentVc {
    fn from(asset_content: AssetContent) -> Self {
        VersionedAssetContentVc::new(asset_content.cell()).into()
    }
}

impl From<AssetContentVc> for VersionedContentVc {
    fn from(asset_content: AssetContentVc) -> Self {
        VersionedAssetContentVc::new(asset_content).into()
    }
}

/// Describes the current version of an object, and how to update them from an
/// earlier version.
#[turbo_tasks::value_trait]
pub trait Version {
    /// Get a unique identifier of the version as a string. There is no way
    /// to convert an id back to its original `Version`, so the original object
    /// needs to be stored somewhere.
    fn id(&self) -> StringVc;
}

/// This trait allows multiple `VersionedContent` to declare which
/// [`VersionedContentMerger`] implementation should be used for merging.
///
/// [`MergeableVersionedContent`] which return the same merger will be merged
/// together.
#[turbo_tasks::value_trait]
pub trait MergeableVersionedContent: VersionedContent {
    fn get_merger(&self) -> VersionedContentMergerVc;
}

/// A [`VersionedContentMerger`] merges multiple [`VersionedContent`] into a
/// single one.
#[turbo_tasks::value_trait]
pub trait VersionedContentMerger {
    fn merge(&self, contents: VersionedContentsVc) -> VersionedContentVc;
}

#[turbo_tasks::value(transparent)]
pub struct VersionedContents(Vec<VersionedContentVc>);

#[turbo_tasks::value]
pub struct NotFoundVersion;

#[turbo_tasks::value_impl]
impl NotFoundVersionVc {
    #[turbo_tasks::function]
    pub fn new() -> Self {
        NotFoundVersion.cell()
    }
}

#[turbo_tasks::value_impl]
impl Version for NotFoundVersion {
    #[turbo_tasks::function]
    fn id(&self) -> StringVc {
        StringVc::empty()
    }
}

/// Describes an update to a versioned object.
#[turbo_tasks::value(shared)]
#[derive(Debug)]
pub enum Update {
    /// The asset can't be meaningfully updated while the app is running, so the
    /// whole thing needs to be replaced.
    Total(TotalUpdate),

    /// The asset can (potentially) be updated to a new version by applying a
    /// specific set of instructions.
    Partial(PartialUpdate),

    /// No update required.
    None,
}

/// A total update to a versioned object.
#[derive(PartialEq, Eq, Debug, Clone, TraceRawVcs, ValueDebugFormat, Serialize, Deserialize)]
pub struct TotalUpdate {
    /// The version this update will bring the object to.
    #[turbo_tasks(trace_ignore)]
    pub to: TraitRef<VersionVc>,
}

/// A partial update to a versioned object.
#[derive(PartialEq, Eq, Debug, Clone, TraceRawVcs, ValueDebugFormat, Serialize, Deserialize)]
pub struct PartialUpdate {
    /// The version this update will bring the object to.
    #[turbo_tasks(trace_ignore)]
    pub to: TraitRef<VersionVc>,
    /// The instructions to be passed to a remote system in order to update the
    /// versioned object.
    #[turbo_tasks(trace_ignore)]
    pub instruction: Arc<serde_json::Value>,
}

/// [`Version`] implementation that hashes a file at a given path and returns
/// the hex encoded hash as a version identifier.
#[turbo_tasks::value]
#[derive(Clone)]
pub struct FileHashVersion {
    hash: String,
}

impl FileHashVersionVc {
    /// Computes a new [`FileHashVersionVc`] from a path.
    pub async fn compute(asset_content: &AssetContent) -> Result<Self> {
        match asset_content {
            AssetContent::File(file_vc) => match &*file_vc.await? {
                FileContent::Content(file) => {
                    let hash = hash_xxh3_hash64(file.content());
                    let hex_hash = encode_hex(hash);
                    Ok(Self::cell(FileHashVersion { hash: hex_hash }))
                }
                FileContent::NotFound => Err(anyhow!("file not found")),
            },
            AssetContent::Redirect { .. } => Err(anyhow!("not a file")),
        }
    }
}

#[turbo_tasks::value_impl]
impl Version for FileHashVersion {
    #[turbo_tasks::function]
    async fn id(&self) -> Result<StringVc> {
        Ok(StringVc::cell(self.hash.clone()))
    }
}
