use anyhow::Result;
use turbo_tasks::Value;
use turbopack_core::{
    asset::Asset,
    chunk::{availability_info::AvailabilityInfo, ChunkItem, ChunkItemVc},
    ident::AssetIdentVc,
    reference::AssetReferencesVc,
};

use super::{asset::EcmascriptModulePartAssetVc, part_of_module, split_module};
use crate::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContentVc, EcmascriptChunkItemVc,
        EcmascriptChunkPlaceable, EcmascriptChunkingContextVc,
    },
    EcmascriptModuleContentVc,
};

/// This is an implementation of [ChunkItem] for [EcmascriptModulePartAssetVc].
///
/// This is a pointer to a part of an ES module.
#[turbo_tasks::value(shared)]
pub struct EcmascriptModulePartChunkItem {
    pub(super) module: EcmascriptModulePartAssetVc,
    pub(super) context: EcmascriptChunkingContextVc,
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for EcmascriptModulePartChunkItem {
    #[turbo_tasks::function]
    fn content(self_vc: EcmascriptModulePartChunkItemVc) -> EcmascriptChunkItemContentVc {
        self_vc.content_with_availability_info(Value::new(AvailabilityInfo::Untracked))
    }

    #[turbo_tasks::function]
    async fn content_with_availability_info(
        self_vc: EcmascriptModulePartChunkItemVc,
        availability_info: Value<AvailabilityInfo>,
    ) -> Result<EcmascriptChunkItemContentVc> {
        let this = self_vc.await?;
        let availability_info = if *this.module.analyze().needs_availability_info().await? {
            availability_info
        } else {
            Value::new(AvailabilityInfo::Untracked)
        };

        let module = this.module.await?;
        let split_data = split_module(module.full_module);
        let parsed = part_of_module(split_data, module.part);

        let content = EcmascriptModuleContentVc::new(
            parsed,
            module.full_module.ident(),
            this.context,
            this.module.analyze(),
            availability_info,
        );

        let async_module_options = module.full_module.get_async_module().module_options();

        Ok(EcmascriptChunkItemContentVc::new(
            content,
            this.context,
            async_module_options,
        ))
    }

    #[turbo_tasks::function]
    fn chunking_context(&self) -> EcmascriptChunkingContextVc {
        self.context
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for EcmascriptModulePartChunkItem {
    #[turbo_tasks::function]
    async fn references(&self) -> AssetReferencesVc {
        self.module.references()
    }

    #[turbo_tasks::function]
    async fn asset_ident(&self) -> Result<AssetIdentVc> {
        Ok(self.module.ident())
    }
}
