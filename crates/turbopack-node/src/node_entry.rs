use anyhow::Result;
use turbo_tasks::Value;
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::chunk::{ChunkingContextVc, EvaluatableAssetsVc};
use turbopack_dev_server::source::ContentSourceData;
use turbopack_ecmascript::EcmascriptModuleAssetVc;

#[turbo_tasks::value(shared)]
pub struct NodeRenderingEntry {
    pub runtime_entries: EvaluatableAssetsVc,
    pub module: EcmascriptModuleAssetVc,
    pub chunking_context: ChunkingContextVc,
    pub node_path: FileSystemPathVc,
    pub node_root: FileSystemPathVc,
    pub project_dir: FileSystemPathVc,
}

#[turbo_tasks::value(transparent)]
pub struct NodeRenderingEntries(Vec<NodeRenderingEntryVc>);

/// Trait that allows to get the entry module for rendering something in Node.js
#[turbo_tasks::value_trait]
pub trait NodeEntry {
    fn entry(&self, data: Value<ContentSourceData>) -> NodeRenderingEntryVc;
    fn entries(&self) -> NodeRenderingEntriesVc {
        NodeRenderingEntriesVc::cell(vec![self.entry(Value::new(Default::default()))])
    }
}
