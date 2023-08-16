use anyhow::Result;
use turbo_tasks::Vc;
use turbo_tasks_fs::{FileContent, FileSystemEntryType, FileSystemPath, LinkContent};

use crate::{
    asset::{Asset, AssetContent},
    ident::AssetIdent,
    reference::AssetReferences,
};

/// The raw [Asset]. It represents raw content from a path without any
/// references to other [Asset]s.
#[turbo_tasks::value]
pub struct SourceAsset {
    pub path: Vc<FileSystemPath>,
}

#[turbo_tasks::value_impl]
impl SourceAsset {
    #[turbo_tasks::function]
    pub fn new(path: Vc<FileSystemPath>) -> Vc<Self> {
        Self::cell(SourceAsset { path })
    }
}

#[turbo_tasks::value_impl]
impl Asset for SourceAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        AssetIdent::from_path(self.path)
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<AssetContent>> {
        let file_type = &*self.path.get_type().await?;
        match file_type {
            FileSystemEntryType::Symlink => match &*self.path.read_link().await? {
                LinkContent::Link { target, link_type } => Ok(AssetContent::Redirect {
                    target: target.clone(),
                    link_type: *link_type,
                }
                .cell()),
                _ => Err(anyhow::anyhow!("Invalid symlink")),
            },
            FileSystemEntryType::File => Ok(AssetContent::File(self.path.read()).cell()),
            FileSystemEntryType::NotFound => {
                Ok(AssetContent::File(FileContent::NotFound.cell()).cell())
            }
            _ => Err(anyhow::anyhow!("Invalid file type {:?}", file_type)),
        }
    }

    #[turbo_tasks::function]
    fn references(&self) -> Vc<AssetReferences> {
        // TODO: build input sourcemaps via language specific sourceMappingURL comment
        // or parse.
        AssetReferences::empty()
    }
}
