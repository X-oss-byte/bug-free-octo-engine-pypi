pub(crate) mod single_item_chunk;
pub mod source_map;
pub(crate) mod writer;

use std::fmt::Write;

use anyhow::{anyhow, Result};
use indexmap::IndexSet;
use turbo_tasks::{primitives::StringVc, TryJoinIterExt, Value, ValueToString};
use turbo_tasks_fs::{rope::Rope, File, FileSystemPathOptionVc};
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc, AssetsVc},
    chunk::{
        availability_info::AvailabilityInfo, chunk_content, chunk_content_split, Chunk,
        ChunkContentResult, ChunkItem, ChunkItemVc, ChunkVc, ChunkableAssetVc, ChunkingContext,
        ChunkingContextVc, ChunksVc, FromChunkableAsset, ModuleId, ModuleIdVc, ModuleIdsVc,
        OutputChunk, OutputChunkRuntimeInfo, OutputChunkRuntimeInfoVc, OutputChunkVc,
    },
    code_builder::{CodeBuilder, CodeVc},
    ident::{AssetIdent, AssetIdentVc},
    introspect::{
        asset::{children_from_asset_references, content_to_details, IntrospectableAssetVc},
        Introspectable, IntrospectableChildrenVc, IntrospectableVc,
    },
    reference::{AssetReference, AssetReferenceVc, AssetReferencesVc},
    resolve::PrimaryResolveResult,
    source_map::{GenerateSourceMap, GenerateSourceMapVc, OptionSourceMapVc},
};
use writer::expand_imports;

use self::{
    single_item_chunk::{chunk::SingleItemCssChunkVc, reference::SingleItemCssChunkReferenceVc},
    source_map::CssChunkSourceMapAssetReferenceVc,
};
use crate::{
    embed::{CssEmbed, CssEmbeddable, CssEmbeddableVc},
    parse::ParseCssResultSourceMapVc,
    util::stringify_js,
    ImportAssetReferenceVc,
};

#[turbo_tasks::value]
struct CssChunkType;

#[turbo_tasks::value_impl]
impl CssChunkTypeVc {
    #[turbo_tasks::function]
    pub fn new() -> Self {
        CssChunkType.cell()
    }
}

impl ChunkType for CssChunkType {
    fn name(&self) -> StringVc {
        StringVc::cell("css".to_string())
    }
}

#[turbo_tasks::value]
pub struct CssChunk {
    pub content: ChunkVc,
}

#[turbo_tasks::value(transparent)]
pub struct CssChunks(Vec<CssChunkVc>);

#[turbo_tasks::value_impl]
impl CssChunkVc {
    #[turbo_tasks::function]
    pub fn new(content: ChunkVc) -> Self {
        CssChunk { content }.cell()
    }

    #[turbo_tasks::function]
    async fn code(self) -> Result<CodeVc> {
        use std::io::Write;

        let this = self.await?;
        let chunk_name = this.chunk.path().to_string();

        let mut body = CodeBuilder::default();
        let mut external_imports = IndexSet::new();
        for entry in this.main_entries.await?.iter() {
            let entry_placeable = CssChunkPlaceableVc::cast_from(entry);
            let entry_item = entry_placeable.as_chunk_item(this.context);

            // TODO(WEB-1261)
            for external_import in expand_imports(&mut body, entry_item).await? {
                external_imports.insert(external_import.await?.to_owned());
            }
        }

        let mut code = CodeBuilder::default();
        writeln!(code, "/* chunk {} */", chunk_name.await?)?;
        for external_import in external_imports {
            writeln!(code, "@import {};", stringify_js(&external_import))?;
        }

        code.push_code(&body.build());

        if *this
            .context
            .reference_chunk_source_maps(this.chunk.into())
            .await?
            && code.has_source_map()
        {
            let chunk_path = this.chunk.path().await?;
            write!(
                code,
                "\n/*# sourceMappingURL={}.map*/",
                chunk_path.file_name()
            )?;
        }

        let c = code.build().cell();
        Ok(c)
    }

    #[turbo_tasks::function]
    async fn content(self) -> Result<AssetContentVc> {
        let code = self.code().await?;
        Ok(File::from(code.source_code().clone()).into())
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for CssChunk {
    #[turbo_tasks::function]
    fn generate_source_map(self_vc: CssChunkVc) -> OptionSourceMapVc {
        self_vc.code().generate_source_map()
    }
}

#[turbo_tasks::value_impl]
impl OutputChunk for CssChunk {
    #[turbo_tasks::function]
    async fn runtime_info(&self) -> Result<OutputChunkRuntimeInfoVc> {
        let content = css_chunk_content(
            self.context,
            self.main_entries,
            Value::new(self.availability_info),
        )
        .await?;
        let entries_chunk_items: Vec<_> = self
            .main_entries
            .await?
            .iter()
            .map(|&entry| entry.as_chunk_item(self.context))
            .collect();
        let included_ids = entries_chunk_items
            .iter()
            .map(|chunk_item| chunk_item.id())
            .collect();
        let imports_chunk_items: Vec<_> = entries_chunk_items
            .iter()
            .map(|&chunk_item| async move {
                Ok(chunk_item
                    .content()
                    .await?
                    .imports
                    .iter()
                    .filter_map(|import| {
                        if let CssImport::Internal(_, item) = import {
                            Some(*item)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>())
            })
            .try_join()
            .await?
            .into_iter()
            .flatten()
            .collect();
        let module_chunks: Vec<_> = content
            .chunk_items
            .iter()
            .chain(imports_chunk_items.iter())
            .map(|item| SingleItemCssChunkVc::new(self.context, *item).into())
            .collect();
        Ok(OutputChunkRuntimeInfo {
            included_ids: Some(ModuleIdsVc::cell(included_ids)),
            module_chunks: Some(AssetsVc::cell(module_chunks)),
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl Asset for CssChunk {
    #[turbo_tasks::function]
    async fn ident(self_vc: CssChunkVc) -> Result<AssetIdentVc> {
        let this = self_vc.await?;

        let main_entries = this.main_entries.await?;
        let main_entry_key = StringVc::cell(String::new());
        let assets = main_entries
            .iter()
            .map(|entry| (main_entry_key, entry.ident()))
            .collect::<Vec<_>>();

        let ident = if let [(_, ident)] = assets[..] {
            ident
        } else {
            let (_, ident) = assets[0];
            AssetIdentVc::new(Value::new(AssetIdent {
                path: ident.path(),
                query: None,
                fragment: None,
                assets,
                modifiers: Vec::new(),
                part: None,
            }))
        };

        Ok(AssetIdentVc::from_path(
            this.context.chunk_path(ident, ".css"),
        ))
    }

    #[turbo_tasks::function]
    fn content(self_vc: CssChunkVc) -> AssetContentVc {
        self_vc.chunk_content().content()
    }

    #[turbo_tasks::function]
    async fn references(self_vc: CssChunkVc) -> Result<AssetReferencesVc> {
        let this = self_vc.await?;
        let content = css_chunk_content(
            this.context,
            this.main_entries,
            Value::new(this.availability_info),
        )
        .await?;
        let mut references = Vec::new();
        for r in content.external_asset_references.iter() {
            references.push(*r);
            for result in r.resolve_reference().await?.primary.iter() {
                if let PrimaryResolveResult::Asset(asset) = result {
                    if let Some(embeddable) = CssEmbeddableVc::resolve_from(asset).await? {
                        let embed = embeddable.as_css_embed(this.context);
                        references.extend(embed.references().await?.iter());
                    }
                }
            }
        }
        for item in content.chunk_items.iter() {
            references.push(SingleItemCssChunkReferenceVc::new(this.context, *item).into());
        }
        if *this
            .context
            .reference_chunk_source_maps(self_vc.into())
            .await?
        {
            references.push(CssChunkSourceMapAssetReferenceVc::new(self_vc).into());
        }
        Ok(AssetReferencesVc::cell(references))
    }
}

#[turbo_tasks::value]
pub struct CssChunkContext {
    context: ChunkingContextVc,
}

#[turbo_tasks::value_impl]
impl CssChunkContextVc {
    #[turbo_tasks::function]
    pub fn of(context: ChunkingContextVc) -> CssChunkContextVc {
        CssChunkContext { context }.cell()
    }

    #[turbo_tasks::function]
    pub async fn chunk_item_id(self, chunk_item: CssChunkItemVc) -> Result<ModuleIdVc> {
        let layer = self.await?.context.layer();
        let mut ident = chunk_item.asset_ident();
        if !layer.await?.is_empty() {
            ident = ident.with_modifier(layer)
        }
        Ok(ModuleId::String(ident.to_string().await?.clone_value()).cell())
    }
}

#[derive(Clone)]
#[turbo_tasks::value(shared)]
pub enum CssImport {
    External(StringVc),
    Internal(ImportAssetReferenceVc, CssChunkItemVc),
    Composes(CssChunkItemVc),
}

#[turbo_tasks::value(shared)]
pub struct CssChunkItemContent {
    pub inner_code: Rope,
    pub imports: Vec<CssImport>,
    pub source_map: Option<ParseCssResultSourceMapVc>,
}

#[turbo_tasks::value_trait]
pub trait CssChunkItem: ChunkItem {
    fn content(&self) -> CssChunkItemContentVc;
    fn chunking_context(&self) -> ChunkingContextVc;
    fn id(&self) -> ModuleIdVc {
        CssChunkContextVc::of(self.chunking_context()).chunk_item_id(*self)
    }
}
