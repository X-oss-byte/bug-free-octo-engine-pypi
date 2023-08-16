use anyhow::Result;
use turbo_tasks_fs::FileSystemPathVc;

use crate::{
    asset::AssetVc,
    compile_time_info::CompileTimeInfoVc,
    reference_type::ReferenceType,
    resolve::{options::ResolveOptionsVc, parse::RequestVc, ResolveResultVc},
};

/// A context for building an asset graph. It's passed through the assets while
/// creating them. It's needed to resolve assets and upgrade assets to a higher
/// type (e. g. from SourceAsset to ModuleAsset).
#[turbo_tasks::value_trait]
pub trait AssetContext {
    fn compile_time_info(&self) -> CompileTimeInfoVc;
    fn resolve_options(
        &self,
        origin_path: FileSystemPathVc,
        reference_type: ReferenceType,
    ) -> ResolveOptionsVc;
    fn resolve_asset(
        &self,
        origin_path: FileSystemPathVc,
        request: RequestVc,
        resolve_options: ResolveOptionsVc,
        reference_type: ReferenceType,
    ) -> ResolveResultVc;
    fn process(&self, asset: AssetVc, reference_type: ReferenceType) -> AssetVc;
    fn process_resolve_result(
        &self,
        result: ResolveResultVc,
        reference_type: ReferenceType,
    ) -> ResolveResultVc;
    fn with_transition(&self, transition: &str) -> AssetContextVc;
}
