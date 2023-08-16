use anyhow::Result;
use indexmap::indexmap;
use turbo_tasks::{primitives::OptionStringVc, Value};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack::{
    ecmascript::chunk_group_files_asset::ChunkGroupFilesAsset,
    module_options::ModuleOptionsContextVc,
    resolve_options_context::ResolveOptionsContextVc,
    transition::{Transition, TransitionVc},
    ModuleAssetContextVc,
};
use turbopack_core::{
    asset::AssetVc, chunk::ChunkingContextVc, compile_time_info::CompileTimeInfoVc,
    context::AssetContext,
};
use turbopack_ecmascript::{
    EcmascriptInputTransform, EcmascriptInputTransformsVc, EcmascriptModuleAssetType,
    EcmascriptModuleAssetVc, InnerAssetsVc,
};

use super::runtime_entry::RuntimeEntriesVc;
use crate::embed_js::next_asset;

/// Makes a transition into a next.js client context.
///
/// It wraps the target asset with client bootstrapping hydration. It changes
/// the environment to be inside of the browser. It offers a module to the
/// importer that exports an array of chunk urls.
#[turbo_tasks::value(shared)]
pub struct NextClientTransition {
    pub is_app: bool,
    pub client_compile_time_info: CompileTimeInfoVc,
    pub client_module_options_context: ModuleOptionsContextVc,
    pub client_resolve_options_context: ResolveOptionsContextVc,
    pub client_chunking_context: ChunkingContextVc,
    pub server_root: FileSystemPathVc,
    pub runtime_entries: RuntimeEntriesVc,
}

#[turbo_tasks::value_impl]
impl Transition for NextClientTransition {
    #[turbo_tasks::function]
    fn process_compile_time_info(
        &self,
        _compile_time_info: CompileTimeInfoVc,
    ) -> CompileTimeInfoVc {
        self.client_compile_time_info
    }

    #[turbo_tasks::function]
    fn process_module_options_context(
        &self,
        _context: ModuleOptionsContextVc,
    ) -> ModuleOptionsContextVc {
        self.client_module_options_context
    }

    #[turbo_tasks::function]
    fn process_resolve_options_context(
        &self,
        _context: ResolveOptionsContextVc,
    ) -> ResolveOptionsContextVc {
        self.client_resolve_options_context
    }

    #[turbo_tasks::function]
    async fn process_module(
        &self,
        asset: AssetVc,
        context: ModuleAssetContextVc,
    ) -> Result<AssetVc> {
        let internal_asset = if self.is_app {
            next_asset("entry/app/hydrate.tsx")
        } else {
            next_asset("entry/next-hydrate.tsx")
        };

        let asset = EcmascriptModuleAssetVc::new_with_inner_assets(
            internal_asset,
            context.into(),
            Value::new(EcmascriptModuleAssetType::Typescript),
            EcmascriptInputTransformsVc::cell(vec![
                EcmascriptInputTransform::TypeScript {
                    use_define_for_class_fields: false,
                },
                EcmascriptInputTransform::React {
                    refresh: false,
                    import_source: OptionStringVc::cell(None),
                    runtime: OptionStringVc::cell(None),
                },
            ]),
            context.compile_time_info(),
            InnerAssetsVc::cell(indexmap! {
                "PAGE".to_string() => asset
            }),
        );

        let runtime_entries = self.runtime_entries.resolve_entries(context.into());

        let asset = ChunkGroupFilesAsset {
            asset: asset.into(),
            chunking_context: self.client_chunking_context,
            base_path: self.server_root.join("_next"),
            server_root: self.server_root,
            runtime_entries: Some(runtime_entries),
        };

        Ok(asset.cell().into())
    }
}
