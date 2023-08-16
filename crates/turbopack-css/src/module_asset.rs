use std::{fmt::Write, sync::Arc};

use anyhow::Result;
use indexmap::IndexMap;
use indoc::formatdoc;
use swc_core::{
    common::{BytePos, FileName, LineCol, SourceMap},
    css::modules::CssClassName,
};
use turbo_tasks::{Value, ValueToString, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::{Asset, AssetContent},
    chunk::{
        availability_info::AvailabilityInfo, Chunk, ChunkItem, ChunkableAsset,
        ChunkableAssetReference, ChunkingContext, ChunkingType, ChunkingTypeOption,
    },
    context::AssetContext,
    ident::AssetIdent,
    issue::{Issue, IssueExt, IssueSeverity},
    reference::{AssetReference, AssetReferences},
    resolve::{origin::ResolveOrigin, parse::Request, ResolveResult},
};
use turbopack_ecmascript::{
    chunk::{
        EcmascriptChunk, EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemExt,
        EcmascriptChunkPlaceable, EcmascriptChunkingContext, EcmascriptExports,
    },
    utils::StringifyJs,
    ParseResultSourceMap,
};

use crate::{
    chunk::{CssChunk, CssChunkItem, CssChunkItemContent, CssChunkPlaceable},
    parse::ParseResult,
    references::compose::CssModuleComposeReference,
    transform::CssInputTransforms,
    CssModuleAsset,
};

#[turbo_tasks::function]
fn modifier() -> Vc<String> {
    Vc::cell("css module".to_string())
}

#[turbo_tasks::value]
#[derive(Clone)]
pub struct ModuleCssModuleAsset {
    pub inner: Vc<CssModuleAsset>,
}

#[turbo_tasks::value_impl]
impl ModuleCssModuleAsset {
    #[turbo_tasks::function]
    pub fn new(
        source: Vc<Box<dyn Asset>>,
        context: Vc<Box<dyn AssetContext>>,
        transforms: Vc<CssInputTransforms>,
    ) -> Vc<Self> {
        Self::cell(ModuleCssModuleAsset {
            inner: CssModuleAsset::new_module(source, context, transforms),
        })
    }
}

#[turbo_tasks::value_impl]
impl Asset for ModuleCssModuleAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        self.inner.source_ident().with_modifier(modifier())
    }

    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        self.inner.content()
    }

    #[turbo_tasks::function]
    async fn references(self: Vc<Self>) -> Result<Vc<AssetReferences>> {
        let references = self.await?.inner.references().await?;
        let module_references = self.module_references().await?;

        let references: Vec<_> = references
            .iter()
            .copied()
            .chain(module_references.iter().copied())
            .collect();

        Ok(Vc::cell(references))
    }
}

/// A CSS class that is exported from a CSS module.
///
/// See [`ModuleCssClasses`] for more information.
#[turbo_tasks::value(transparent)]
#[derive(Debug, Clone)]
enum ModuleCssClass {
    Local {
        name: String,
    },
    Global {
        name: String,
    },
    Import {
        original: String,
        from: Vc<CssModuleComposeReference>,
    },
}

/// A map of CSS classes exported from a CSS module.
///
/// ## Example
///
/// ```css
/// :global(.class1) {
///    color: red;
/// }
///
/// .class2 {
///   color: blue;
/// }
///
/// .class3 {
///   composes: class4 from "./other.module.css";
/// }
/// ```
///
/// The above CSS module would have the following exports:
/// 1. class1: [Global("exported_class1")]
/// 2. class2: [Local("exported_class2")]
/// 3. class3: [Local("exported_class3), Import("class4", "./other.module.css")]
#[turbo_tasks::value(transparent)]
#[derive(Debug, Clone)]
struct ModuleCssClasses(IndexMap<String, Vec<ModuleCssClass>>);

#[turbo_tasks::value_impl]
impl ModuleCssModuleAsset {
    #[turbo_tasks::function]
    async fn classes(self: Vc<Self>) -> Result<Vc<ModuleCssClasses>> {
        let inner = self.await?.inner;
        let parse_result = inner.parse().await?;
        let mut classes = IndexMap::default();

        // TODO(alexkirsz) Should we report an error on parse error here?
        if let ParseResult::Ok { exports, .. } = &*parse_result {
            for (class_name, export_class_names) in exports {
                let mut export = Vec::default();

                for export_class_name in export_class_names {
                    export.push(match export_class_name {
                        CssClassName::Import { from, name } => ModuleCssClass::Import {
                            original: name.value.to_string(),
                            from: CssModuleComposeReference::new(
                                Vc::upcast(self),
                                Request::parse(Value::new(from.to_string().into())),
                            ),
                        },
                        CssClassName::Local { name } => ModuleCssClass::Local {
                            name: name.value.to_string(),
                        },
                        CssClassName::Global { name } => ModuleCssClass::Global {
                            name: name.value.to_string(),
                        },
                    })
                }

                classes.insert(class_name.to_string(), export);
            }
        }

        Ok(Vc::cell(classes))
    }

    #[turbo_tasks::function]
    async fn module_references(self: Vc<Self>) -> Result<Vc<AssetReferences>> {
        let mut references = vec![];

        for (_, class_names) in &*self.classes().await? {
            for class_name in class_names {
                match class_name {
                    ModuleCssClass::Import { from, .. } => {
                        references.push(Vc::upcast(*from));
                    }
                    ModuleCssClass::Local { .. } | ModuleCssClass::Global { .. } => {}
                }
            }
        }

        Ok(Vc::cell(references))
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for ModuleCssModuleAsset {
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
impl EcmascriptChunkPlaceable for ModuleCssModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self: Vc<Self>,
        context: Vc<Box<dyn EcmascriptChunkingContext>>,
    ) -> Vc<Box<dyn EcmascriptChunkItem>> {
        Vc::upcast(
            ModuleChunkItem {
                context,
                module: self,
            }
            .cell(),
        )
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> Vc<EcmascriptExports> {
        EcmascriptExports::Value.cell()
    }
}

#[turbo_tasks::value_impl]
impl ResolveOrigin for ModuleCssModuleAsset {
    #[turbo_tasks::function]
    fn origin_path(&self) -> Vc<FileSystemPath> {
        self.inner.ident().path()
    }

    #[turbo_tasks::function]
    fn context(&self) -> Vc<Box<dyn AssetContext>> {
        self.inner.context()
    }
}

#[turbo_tasks::value]
struct ModuleChunkItem {
    module: Vc<ModuleCssModuleAsset>,
    context: Vc<Box<dyn EcmascriptChunkingContext>>,
}

#[turbo_tasks::value_impl]
impl ChunkItem for ModuleChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> Vc<AssetIdent> {
        self.module.ident()
    }

    #[turbo_tasks::function]
    async fn references(&self) -> Result<Vc<AssetReferences>> {
        // The proxy reference must come first so it is processed before other potential
        // references inside of the CSS, like `@import` and `composes:`.
        // This affects the order in which the resulting CSS chunks will be loaded:
        // later references are processed first in the post-order traversal of the
        // reference tree, and as such they will be loaded first in the resulting HTML.
        let mut references = vec![Vc::upcast(
            CssProxyToCssAssetReference {
                module: self.module,
            }
            .cell(),
        )];

        references.extend(self.module.references().await?.iter().copied());

        Ok(Vc::cell(references))
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for ModuleChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> Vc<Box<dyn EcmascriptChunkingContext>> {
        self.context
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<EcmascriptChunkItemContent>> {
        let classes = self.module.classes().await?;

        let mut code = "__turbopack_export_value__({\n".to_string();
        for (export_name, class_names) in &*classes {
            let mut exported_class_names = Vec::with_capacity(class_names.len());

            for class_name in class_names {
                match class_name {
                    ModuleCssClass::Import {
                        original: original_name,
                        from,
                    } => {
                        let resolved_module = from.resolve_reference().first_asset().await?;

                        let Some(resolved_module) = &*resolved_module else {
                            CssModuleComposesIssue {
                                severity: IssueSeverity::Error.cell(),
                                source: self.module.ident(),
                                message: Vc::cell(formatdoc! {
                                    r#"
                                        Module {from} referenced in `composes: ... from {from};` can't be resolved.
                                    "#,
                                    from = &*from.await?.request.to_string().await?
                                }),
                            }.cell().emit();
                            continue;
                        };

                        let Some(css_module) = Vc::try_resolve_downcast_type::<ModuleCssModuleAsset>(*resolved_module).await? else {
                            CssModuleComposesIssue {
                                severity: IssueSeverity::Error.cell(),
                                source: self.module.ident(),
                                message: Vc::cell(formatdoc! {
                                    r#"
                                        Module {from} referenced in `composes: ... from {from};` is not a CSS module.
                                    "#,
                                    from = &*from.await?.request.to_string().await?
                                }),
                            }.cell().emit();
                            continue;
                        };

                        // TODO(alexkirsz) We should also warn if `original_name` can't be found in
                        // the target module.

                        let placeable: Vc<Box<dyn EcmascriptChunkPlaceable>> =
                            Vc::upcast(css_module);

                        let module_id = placeable.as_chunk_item(self.context).id().await?;
                        let module_id = StringifyJs(&*module_id);
                        let original_name = StringifyJs(&original_name);
                        exported_class_names.push(format! {
                            "__turbopack_import__({module_id})[{original_name}]"
                        });
                    }
                    ModuleCssClass::Local { name: class_name }
                    | ModuleCssClass::Global { name: class_name } => {
                        exported_class_names.push(StringifyJs(&class_name).to_string());
                    }
                }
            }

            writeln!(
                code,
                "  {}: {},",
                StringifyJs(export_name),
                exported_class_names.join(" + \" \" + ")
            )?;
        }
        code += "});\n";
        Ok(EcmascriptChunkItemContent {
            inner_code: code.clone().into(),
            // We generate a minimal map for runtime code so that the filename is
            // displayed in dev tools.
            source_map: Some(generate_minimal_source_map(
                self.module.ident().to_string().await?.to_string(),
                code,
            )),
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value]
struct CssProxyToCssAssetReference {
    module: Vc<ModuleCssModuleAsset>,
}

#[turbo_tasks::value_impl]
impl ValueToString for CssProxyToCssAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(format!(
            "proxy(css) {}",
            self.module.ident().to_string().await?,
        )))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for CssProxyToCssAssetReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> Vc<ResolveResult> {
        ResolveResult::asset(Vc::upcast(
            CssProxyModuleAsset {
                module: self.module,
            }
            .cell(),
        ))
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAssetReference for CssProxyToCssAssetReference {
    #[turbo_tasks::function]
    fn chunking_type(&self) -> Vc<ChunkingTypeOption> {
        Vc::cell(Some(ChunkingType::Parallel))
    }
}

/// This structure exists solely in order to extend the `references` returned by
/// a standard [`CssModuleAsset`] with CSS modules' `composes:` references.
#[turbo_tasks::value]
#[derive(Clone)]
struct CssProxyModuleAsset {
    module: Vc<ModuleCssModuleAsset>,
}

#[turbo_tasks::value_impl]
impl Asset for CssProxyModuleAsset {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<Vc<AssetIdent>> {
        Ok(self.module.await?.inner.ident().with_modifier(modifier()))
    }

    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        self.module.content()
    }

    #[turbo_tasks::function]
    async fn references(&self) -> Result<Vc<AssetReferences>> {
        // The original references must come first so they're processed before other
        // potential references inside of the CSS, like `@import` and `composes:`. This
        // affects the order in which the resulting CSS chunks will be loaded:
        // later references are processed first in the post-order traversal of
        // the reference tree, and as such they will be loaded first in the
        // resulting HTML.
        let mut references = self.module.await?.inner.references().await?.clone_value();

        references.extend(self.module.module_references().await?.iter().copied());

        Ok(Vc::cell(references))
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for CssProxyModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk(
        self: Vc<Self>,
        context: Vc<Box<dyn ChunkingContext>>,
        availability_info: Value<AvailabilityInfo>,
    ) -> Vc<Box<dyn Chunk>> {
        Vc::upcast(CssChunk::new(context, Vc::upcast(self), availability_info))
    }
}

#[turbo_tasks::value_impl]
impl CssChunkPlaceable for CssProxyModuleAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self: Vc<Self>,
        context: Vc<Box<dyn ChunkingContext>>,
    ) -> Vc<Box<dyn CssChunkItem>> {
        Vc::upcast(CssProxyModuleChunkItem::cell(CssProxyModuleChunkItem {
            inner: self,
            context,
        }))
    }
}

#[turbo_tasks::value_impl]
impl ResolveOrigin for CssProxyModuleAsset {
    #[turbo_tasks::function]
    fn origin_path(&self) -> Vc<FileSystemPath> {
        self.module.ident().path()
    }

    #[turbo_tasks::function]
    fn context(&self) -> Vc<Box<dyn AssetContext>> {
        self.module.context()
    }
}

#[turbo_tasks::value]
struct CssProxyModuleChunkItem {
    inner: Vc<CssProxyModuleAsset>,
    context: Vc<Box<dyn ChunkingContext>>,
}

#[turbo_tasks::value_impl]
impl ChunkItem for CssProxyModuleChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> Vc<AssetIdent> {
        self.inner.ident()
    }

    #[turbo_tasks::function]
    fn references(&self) -> Vc<AssetReferences> {
        self.inner.references()
    }
}

#[turbo_tasks::value_impl]
impl CssChunkItem for CssProxyModuleChunkItem {
    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<CssChunkItemContent>> {
        Ok(self
            .inner
            .await?
            .module
            .await?
            .inner
            .as_chunk_item(self.context)
            .content())
    }

    #[turbo_tasks::function]
    fn chunking_context(&self) -> Vc<Box<dyn ChunkingContext>> {
        self.context
    }
}

fn generate_minimal_source_map(filename: String, source: String) -> Vc<ParseResultSourceMap> {
    let mut mappings = vec![];
    // Start from 1 because 0 is reserved for dummy spans in SWC.
    let mut pos = 1;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        mappings.push((
            BytePos(pos),
            LineCol {
                line: index as u32,
                col: 0,
            },
        ));
        pos += line.len() as u32;
    }
    let sm: Arc<SourceMap> = Default::default();
    sm.new_source_file(FileName::Custom(filename), source);
    let map = ParseResultSourceMap::new(sm, mappings);
    map.cell()
}

#[turbo_tasks::value(shared)]
struct CssModuleComposesIssue {
    severity: Vc<IssueSeverity>,
    source: Vc<AssetIdent>,
    message: Vc<String>,
}

#[turbo_tasks::value_impl]
impl Issue for CssModuleComposesIssue {
    #[turbo_tasks::function]
    fn severity(&self) -> Vc<IssueSeverity> {
        self.severity
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(
            "An issue occurred while resolving a CSS module `composes:` rule".to_string(),
        ))
    }

    #[turbo_tasks::function]
    fn category(&self) -> Vc<String> {
        Vc::cell("css".to_string())
    }

    #[turbo_tasks::function]
    fn context(&self) -> Vc<FileSystemPath> {
        self.source.path()
    }

    #[turbo_tasks::function]
    fn description(&self) -> Vc<String> {
        self.message
    }
}
