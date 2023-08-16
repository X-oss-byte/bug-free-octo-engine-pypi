use turbo_tasks_fs::{embed_directory, FileContentVc, FileSystem, FileSystemPathVc, FileSystemVc};
use turbopack_core::{code_builder::CodeVc, context::AssetContextVc};
use turbopack_ecmascript::StaticEcmascriptCodeVc;

#[turbo_tasks::function]
pub fn embed_fs() -> FileSystemVc {
    embed_directory!("turbopack", "$CARGO_MANIFEST_DIR/js/src")
}

#[turbo_tasks::function]
pub fn embed_file(path: &str) -> FileContentVc {
    embed_fs().root().join(path).read()
}

#[turbo_tasks::function]
pub fn embed_file_path(path: &str) -> FileSystemPathVc {
    embed_fs().root().join(path)
}

#[turbo_tasks::function]
pub fn embed_static_code(asset_context: AssetContextVc, path: &str) -> CodeVc {
    StaticEcmascriptCodeVc::new(asset_context, embed_file_path(path)).code()
}
