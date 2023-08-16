//! JSON asset support for turbopack.
//!
//! JSON assets are parsed to ensure they contain valid JSON.
//!
//! When imported from ES modules, they produce a module that exports the
//! JSON value as an object.

#![feature(min_specialization)]
#![feature(arbitrary_self_types)]
#![feature(async_fn_in_trait)]

use std::fmt::Write;

use anyhow::{bail, Error, Result};
use turbo_tasks::{Value, ValueToString, Vc};
use turbo_tasks_fs::{FileContent, FileJsonContent};
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{
        availability_info::AvailabilityInfo, Chunk, ChunkItem, ChunkableAsset, ChunkingContext,
    },
    ident::AssetIdent,
    reference::AssetReferences,
};
use turbopack_ecmascript::chunk::{
    EcmascriptChunk, EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkPlaceable,
    EcmascriptChunkingContext, EcmascriptExports,
};

#[turbo_tasks::function]
fn modifier() -> Vc<String> {
    Vc::cell("json".to_string())
}

#[turbo_tasks::value]
pub struct JsonModuleAsset {
    source: Vc<Box<dyn Asset>>,
}

#[turbo_tasks::value_impl]
impl JsonModuleAsset {
    #[turbo_tasks::function]
    pub fn new(source: Vc<Box<dyn Asset>>) -> Vc<Self> {
        Self::cell(JsonModuleAsset { source })
    }
}

#[turbo_tasks::value_impl]
impl Asset for JsonModuleAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        self.source.ident().with_modifier(modifier())
    }

    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        self.source.content()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for JsonModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk(
        self: Vc<Self>,
        context: Vc<Box<dyn ChunkingContext>>,
        availability_info: Value<AvailabilityInfo>,
    ) -> Vc<Box<dyn Chunk>> {
        Vc::upcast(EcmascriptChunk::new(
            context,
            Vc::upcast(self),
            availability_info,
        ))
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for JsonModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self: Vc<Self>,
        context: Vc<Box<dyn EcmascriptChunkingContext>>,
    ) -> Vc<Box<dyn EcmascriptChunkItem>> {
        Vc::upcast(JsonChunkItem::cell(JsonChunkItem {
            module: self,
            context,
        }))
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> Vc<EcmascriptExports> {
        EcmascriptExports::Value.cell()
    }
}

#[turbo_tasks::value]
struct JsonChunkItem {
    module: Vc<JsonModuleAsset>,
    context: Vc<Box<dyn EcmascriptChunkingContext>>,
}

#[turbo_tasks::value_impl]
impl ChunkItem for JsonChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> Vc<AssetIdent> {
        self.module.ident()
    }

    #[turbo_tasks::function]
    fn references(&self) -> Vc<AssetReferences> {
        self.module.references()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for JsonChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> Vc<Box<dyn EcmascriptChunkingContext>> {
        self.context
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<EcmascriptChunkItemContent>> {
        // We parse to JSON and then stringify again to ensure that the
        // JSON is valid.
        let content = self.module.content().file_content();
        let data = content.parse_json().await?;
        match &*data {
            FileJsonContent::Content(data) => {
                let js_str_content = serde_json::to_string(&data.to_string())?;
                let inner_code =
                    format!("__turbopack_export_value__(JSON.parse({js_str_content}));");

                Ok(EcmascriptChunkItemContent {
                    inner_code: inner_code.into(),
                    ..Default::default()
                }
                .into())
            }
            FileJsonContent::Unparseable(e) => {
                let mut message = "Unable to make a module from invalid JSON: ".to_string();
                if let FileContent::Content(content) = &*content.await? {
                    let text = content.content().to_str()?;
                    e.write_with_content(&mut message, text.as_ref())?;
                } else {
                    write!(message, "{}", e)?;
                }

                Err(Error::msg(message))
            }
            FileJsonContent::NotFound => {
                bail!(
                    "JSON file not found: {}",
                    self.module.ident().to_string().await?
                );
            }
        }
    }
}

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    turbopack_ecmascript::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
