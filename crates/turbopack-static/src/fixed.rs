use anyhow::Result;
use turbo_tasks::Vc;
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::{Asset, AssetContent},
    ident::AssetIdent,
};

/// A static asset that is served at a fixed output path. It won't use
/// content hashing to generate a long term cacheable URL.
#[turbo_tasks::value]
pub struct FixedStaticAsset {
    output_path: Vc<FileSystemPath>,
    source: Vc<Box<dyn Asset>>,
}

#[turbo_tasks::value_impl]
impl FixedStaticAsset {
    #[turbo_tasks::function]
    pub fn new(output_path: Vc<FileSystemPath>, source: Vc<Box<dyn Asset>>) -> Vc<Self> {
        FixedStaticAsset {
            output_path,
            source,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl Asset for FixedStaticAsset {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        Ok(AssetIdent::from_path(self.output_path))
    }

    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        self.source.content()
    }
}
