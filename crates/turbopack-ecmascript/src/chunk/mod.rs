pub(crate) mod content;
pub(crate) mod context;
pub(crate) mod data;
pub(crate) mod item;

use std::fmt::Write;

use anyhow::{anyhow, bail, Result};
use indexmap::IndexSet;
use turbo_tasks::{
    primitives::{StringReadRef, StringVc, UsizeVc},
    TryJoinIterExt, Value, ValueToString, ValueToStringVc,
};
use turbo_tasks_fs::FileSystemPathOptionVc;
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    chunk::{
        availability_info::AvailabilityInfo, Chunk, ChunkItem, ChunkVc, ChunkingContext,
        ChunkingContextVc, ChunksVc, ModuleIdsVc,
    },
    ident::{AssetIdent, AssetIdentVc},
    introspect::{
        asset::{children_from_asset_references, content_to_details, IntrospectableAssetVc},
        Introspectable, IntrospectableChildrenVc, IntrospectableVc,
    },
    reference::AssetReferencesVc,
};

use self::content::ecmascript_chunk_content;
pub use self::{
    content::{EcmascriptChunkContent, EcmascriptChunkContentVc},
    context::{EcmascriptChunkingContext, EcmascriptChunkingContextVc},
    data::EcmascriptChunkData,
    item::{
        EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemContentVc,
        EcmascriptChunkItemOptions, EcmascriptChunkItemVc,
    },
    placeable::{
        EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc, EcmascriptChunkPlaceables,
        EcmascriptChunkPlaceablesVc, EcmascriptExports, EcmascriptExportsVc,
    },
};
use crate::utils::FormatIter;

#[turbo_tasks::value]
struct EcmascriptChunkType;

#[turbo_tasks::value_impl]
impl EcmascriptChunkTypeVc {
    #[turbo_tasks::function]
    pub fn new() -> Self {
        EcmascriptChunkType.cell()
    }
}

impl ChunkType for EcmascriptChunkType {
    fn name(&self) -> StringVc {
        StringVc::cell("ecmascript".to_string())
    }
}
