use anyhow::Result;
use indexmap::IndexSet;
use turbo_tasks::{
    primitives::{BoolVc, U64Vc},
    TryJoinIterExt, ValueToString,
};
use turbo_tasks_hash::Xxh3Hash64Hasher;

use crate::asset::{Asset, AssetVc};

/// Allows to gather information about which assets are already available.
/// Adding more roots will form a linked list like structure to allow caching
/// `include` queries.
#[turbo_tasks::value(shared)]
pub struct AvailableAssets {
    pub parent: Option<AvailableAssetsVc>,
    pub assets: IndexSet<AssetVc>,
}

#[turbo_tasks::value_impl]
impl AvailableAssetsVc {
    #[turbo_tasks::function]
    pub async fn hash(self) -> Result<U64Vc> {
        let this = self.await?;
        let mut hasher = Xxh3Hash64Hasher::new();
        if let Some(parent) = this.parent {
            hasher.write_value(parent.hash().await?);
        } else {
            hasher.write_value(0u64);
        }
        let idents = this
            .assets
            .iter()
            .map(|asset| asset.ident().to_string())
            .try_join()
            .await?;
        for ident in idents {
            hasher.write_value(ident.as_str());
        }
        Ok(U64Vc::cell(hasher.finish()))
    }

    #[turbo_tasks::function]
    pub async fn includes(self, asset: AssetVc) -> Result<BoolVc> {
        let this = self.await?;
        if let Some(parent) = this.parent {
            if *parent.includes(asset).await? {
                return Ok(BoolVc::cell(true));
            }
        }
        Ok(BoolVc::cell(this.assets.contains(&asset)))
    }
}
