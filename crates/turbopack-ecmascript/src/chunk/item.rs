use anyhow::Result;
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use turbo_tasks::{trace::TraceRawVcs, TryJoinIterExt, Value};
use turbo_tasks_fs::rope::Rope;
use turbopack_core::{
    asset::AssetVc,
    chunk::{
        availability_info::AvailabilityInfo, available_assets::AvailableAssetsVc, ChunkItem,
        ChunkItemVc, ChunkableAssetVc, ChunkingContextVc, FromChunkableAsset, ModuleIdVc,
    },
};

use super::{
    context::EcmascriptChunkContextVc,
    manifest::{chunk_asset::ManifestChunkAssetVc, loader_item::ManifestLoaderItemVc},
    placeable::EcmascriptChunkPlaceableVc,
    snapshot::{
        EcmascriptChunkContentEntries, EcmascriptChunkContentEntriesSnapshot,
        EcmascriptChunkContentEntriesSnapshotVc, EcmascriptChunkContentEntryVc,
    },
    EcmascriptChunkPlaceable,
};
use crate::ParseResultSourceMapVc;

#[turbo_tasks::value(shared)]
#[derive(Default)]
pub struct EcmascriptChunkItemContent {
    pub inner_code: Rope,
    pub source_map: Option<ParseResultSourceMapVc>,
    pub options: EcmascriptChunkItemOptions,
    pub placeholder_for_future_extensions: (),
}

#[derive(PartialEq, Eq, Default, Debug, Clone, Serialize, Deserialize, TraceRawVcs)]
pub struct EcmascriptChunkItemOptions {
    pub module: bool,
    pub exports: bool,
    pub this: bool,
    pub placeholder_for_future_extensions: (),
}

#[turbo_tasks::value_trait]
pub trait EcmascriptChunkItem: ChunkItem {
    fn content(&self) -> EcmascriptChunkItemContentVc;
    fn chunking_context(&self) -> ChunkingContextVc;
    fn id(&self) -> ModuleIdVc {
        EcmascriptChunkContextVc::of(self.chunking_context()).chunk_item_id(*self)
    }
}

#[async_trait::async_trait]
impl FromChunkableAsset for EcmascriptChunkItemVc {
    async fn from_asset(context: ChunkingContextVc, asset: AssetVc) -> Result<Option<Self>> {
        if let Some(placeable) = EcmascriptChunkPlaceableVc::resolve_from(asset).await? {
            return Ok(Some(placeable.as_chunk_item(context)));
        }
        Ok(None)
    }

    async fn from_async_asset(
        context: ChunkingContextVc,
        asset: ChunkableAssetVc,
        availability_info: Value<AvailabilityInfo>,
    ) -> Result<Option<Self>> {
        let next_availability_info = match availability_info.into_value() {
            AvailabilityInfo::Untracked => AvailabilityInfo::Untracked,
            AvailabilityInfo::Root {
                current_availability_root,
            } => AvailabilityInfo::Inner {
                available_assets: AvailableAssetsVc::new(vec![current_availability_root]),
                current_availability_root: asset.as_asset(),
            },
            AvailabilityInfo::Inner {
                available_assets,
                current_availability_root,
            } => AvailabilityInfo::Inner {
                available_assets: available_assets.with_roots(vec![current_availability_root]),
                current_availability_root: asset.as_asset(),
            },
        };
        let manifest_asset =
            ManifestChunkAssetVc::new(asset, context, Value::new(next_availability_info));
        let manifest_loader = ManifestLoaderItemVc::new(manifest_asset);
        Ok(Some(manifest_loader.into()))
    }
}

#[turbo_tasks::value(transparent)]
pub struct EcmascriptChunkItemsChunk(Vec<EcmascriptChunkItemVc>);

#[turbo_tasks::value(transparent)]
pub struct EcmascriptChunkItems(pub(super) Vec<EcmascriptChunkItemsChunkVc>);

impl EcmascriptChunkItems {
    pub fn make_chunks(list: &[EcmascriptChunkItemVc]) -> Vec<EcmascriptChunkItemsChunkVc> {
        let size = list.len().div_ceil(100);
        let chunk_items = list
            .chunks(size)
            .map(|chunk| EcmascriptChunkItemsChunkVc::cell(chunk.to_vec()))
            .collect();
        chunk_items
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItemsChunkVc {
    #[turbo_tasks::function]
    async fn to_entry_snapshot(self) -> Result<EcmascriptChunkContentEntriesSnapshotVc> {
        let list = self.await?;
        Ok(EcmascriptChunkContentEntries(
            list.iter()
                .map(|chunk_item| EcmascriptChunkContentEntryVc::new(*chunk_item))
                .collect(),
        )
        .cell()
        .snapshot())
    }
}

#[turbo_tasks::value(transparent)]
pub(super) struct EcmascriptChunkItemsSet(IndexSet<EcmascriptChunkItemVc>);

#[turbo_tasks::value_impl]
impl EcmascriptChunkItemsVc {
    #[turbo_tasks::function]
    pub(super) async fn to_entry_snapshot(self) -> Result<EcmascriptChunkContentEntriesSnapshotVc> {
        let list = self.await?;
        Ok(EcmascriptChunkContentEntriesSnapshot::Nested(
            list.iter()
                .map(|chunk| chunk.to_entry_snapshot())
                .try_join()
                .await?,
        )
        .cell())
    }

    #[turbo_tasks::function]
    pub(super) async fn to_set(self) -> Result<EcmascriptChunkItemsSetVc> {
        let mut set = IndexSet::new();
        for chunk in self.await?.iter().copied().try_join().await? {
            set.extend(chunk.iter().copied())
        }
        Ok(EcmascriptChunkItemsSetVc::cell(set))
    }
}
