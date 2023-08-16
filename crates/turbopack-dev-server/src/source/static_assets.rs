use anyhow::Result;
use turbo_tasks::{Value, Vc};
use turbo_tasks_fs::{DirectoryContent, DirectoryEntry, FileSystemEntryType, FileSystemPath};
use turbopack_core::{
    asset::Asset,
    introspect::{asset::IntrospectableAsset, Introspectable, IntrospectableChildren},
    source_asset::SourceAsset,
    version::VersionedContentExt,
};

use super::{ContentSource, ContentSourceContent, ContentSourceData, ContentSourceResult};

#[turbo_tasks::value(shared)]
pub struct StaticAssetsContentSource {
    pub prefix: Vc<String>,
    pub dir: Vc<FileSystemPath>,
}

#[turbo_tasks::value_impl]
impl StaticAssetsContentSource {
    // TODO(WEB-1151): Remove this method and migrate users to `with_prefix`.
    #[turbo_tasks::function]
    pub fn new(prefix: String, dir: Vc<FileSystemPath>) -> Vc<StaticAssetsContentSource> {
        StaticAssetsContentSource::with_prefix(Vc::cell(prefix), dir)
    }

    #[turbo_tasks::function]
    pub async fn with_prefix(
        prefix: Vc<String>,
        dir: Vc<FileSystemPath>,
    ) -> Result<Vc<StaticAssetsContentSource>> {
        if cfg!(debug_assertions) {
            let prefix_string = prefix.await?;
            debug_assert!(prefix_string.is_empty() || prefix_string.ends_with('/'));
            debug_assert!(!prefix_string.starts_with('/'));
        }
        Ok(StaticAssetsContentSource { prefix, dir }.cell())
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for StaticAssetsContentSource {
    #[turbo_tasks::function]
    async fn get(
        &self,
        path: String,
        _data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        if !path.is_empty() {
            let prefix = self.prefix.await?;
            if let Some(path) = path.strip_prefix(&*prefix) {
                let path = self.dir.join(path.to_string());
                let ty = path.get_type().await?;
                if matches!(
                    &*ty,
                    FileSystemEntryType::File | FileSystemEntryType::Symlink
                ) {
                    let content = Vc::upcast::<Box<dyn Asset>>(SourceAsset::new(path)).content();
                    return Ok(ContentSourceResult::exact(Vc::upcast(
                        ContentSourceContent::static_content(content.versioned()),
                    )));
                }
            }
        }
        Ok(ContentSourceResult::not_found())
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for StaticAssetsContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("static assets directory content source".to_string())
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        let dir = self.dir.read_dir().await?;
        let DirectoryContent::Entries(entries) = &*dir else {
            return Ok(Vc::cell(Default::default()));
        };

        let prefix = self.prefix.await?;
        let children = entries
            .iter()
            .map(|(name, entry)| {
                let child = match entry {
                    DirectoryEntry::File(path) | DirectoryEntry::Symlink(path) => {
                        IntrospectableAsset::new(Vc::upcast(SourceAsset::new(*path)))
                    }
                    DirectoryEntry::Directory(path) => {
                        Vc::upcast(StaticAssetsContentSource::with_prefix(
                            Vc::cell(format!("{}{name}/", &*prefix)),
                            *path,
                        ))
                    }
                    DirectoryEntry::Other(_) => todo!("what's DirectoryContent::Other?"),
                    DirectoryEntry::Error => todo!(),
                };
                (Vc::cell(name.clone()), child)
            })
            .collect();
        Ok(Vc::cell(children))
    }
}
