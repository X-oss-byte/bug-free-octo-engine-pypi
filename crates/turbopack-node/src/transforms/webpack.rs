use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use turbo_tasks::{primitives::JsonValueVc, trace::TraceRawVcs, CompletionVc, Value};
use turbo_tasks_bytes::stream::SingleValue;
use turbo_tasks_fs::{json::parse_json_with_source_context, File, FileContent};
use turbopack_core::{
    asset::{Asset, AssetContent, AssetContentVc, AssetVc},
    context::{AssetContext, AssetContextVc},
    ident::AssetIdentVc,
    source_asset::SourceAssetVc,
    source_transform::{SourceTransform, SourceTransformVc},
    virtual_asset::VirtualAssetVc,
};
use turbopack_ecmascript::{
    EcmascriptInputTransform, EcmascriptInputTransformsVc, EcmascriptModuleAssetType,
    EcmascriptModuleAssetVc,
};

use super::util::{emitted_assets_to_virtual_assets, EmittedAsset};
use crate::{
    embed_js::embed_file_path,
    evaluate::evaluate,
    execution_context::{ExecutionContext, ExecutionContextVc},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[turbo_tasks::value(transparent, serialization = "custom")]
struct WebpackLoadersProcessingResult {
    source: String,
    map: Option<String>,
    #[turbo_tasks(trace_ignore)]
    assets: Option<Vec<EmittedAsset>>,
}

#[derive(Clone, PartialEq, Eq, Debug, TraceRawVcs, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WebpackLoaderConfigItem {
    LoaderName(String),
    LoaderNameWithOptions {
        loader: String,
        #[turbo_tasks(trace_ignore)]
        options: serde_json::Map<String, serde_json::Value>,
    },
}

#[derive(Debug, Clone)]
#[turbo_tasks::value(shared, transparent)]
pub struct WebpackLoaderConfigItems(pub Vec<WebpackLoaderConfigItem>);

#[turbo_tasks::value]
pub struct WebpackLoaders {
    evaluate_context: AssetContextVc,
    execution_context: ExecutionContextVc,
    loaders: WebpackLoaderConfigItemsVc,
}

#[turbo_tasks::value_impl]
impl WebpackLoadersVc {
    #[turbo_tasks::function]
    pub fn new(
        evaluate_context: AssetContextVc,
        execution_context: ExecutionContextVc,
        loaders: WebpackLoaderConfigItemsVc,
    ) -> Self {
        WebpackLoaders {
            evaluate_context,
            execution_context,
            loaders,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl SourceTransform for WebpackLoaders {
    #[turbo_tasks::function]
    fn transform(&self, source: AssetVc) -> AssetVc {
        WebpackLoadersProcessedAsset {
            evaluate_context: self.evaluate_context,
            execution_context: self.execution_context,
            loaders: self.loaders,
            source,
        }
        .cell()
        .into()
    }
}

#[turbo_tasks::value]
struct WebpackLoadersProcessedAsset {
    evaluate_context: AssetContextVc,
    execution_context: ExecutionContextVc,
    loaders: WebpackLoaderConfigItemsVc,
    source: AssetVc,
}

#[turbo_tasks::value_impl]
impl Asset for WebpackLoadersProcessedAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> AssetIdentVc {
        self.source.ident()
    }

    #[turbo_tasks::function]
    async fn content(self_vc: WebpackLoadersProcessedAssetVc) -> Result<AssetContentVc> {
        Ok(self_vc.process().await?.content)
    }
}

#[turbo_tasks::value]
struct ProcessWebpackLoadersResult {
    content: AssetContentVc,
    assets: Vec<VirtualAssetVc>,
}

#[turbo_tasks::function]
fn webpack_loaders_executor(context: AssetContextVc) -> AssetVc {
    EcmascriptModuleAssetVc::new(
        SourceAssetVc::new(embed_file_path("transforms/webpack-loaders.ts")).into(),
        context,
        Value::new(EcmascriptModuleAssetType::Typescript),
        EcmascriptInputTransformsVc::cell(vec![EcmascriptInputTransform::TypeScript {
            use_define_for_class_fields: false,
        }]),
        Value::new(Default::default()),
        context.compile_time_info(),
    )
    .into()
}

#[turbo_tasks::value_impl]
impl WebpackLoadersProcessedAssetVc {
    #[turbo_tasks::function]
    async fn process(self) -> Result<ProcessWebpackLoadersResultVc> {
        let this = self.await?;

        let ExecutionContext {
            project_path,
            chunking_context,
            env,
        } = *this.execution_context.await?;
        let source_content = this.source.content();
        let AssetContent::File(file) = *source_content.await? else {
            bail!("Webpack Loaders transform only support transforming files");
        };
        let FileContent::Content(content) = &*file.await? else {
            return Ok(ProcessWebpackLoadersResult {
                content: AssetContent::File(FileContent::NotFound.cell()).cell(),
                assets: Vec::new()
            }.cell());
        };
        let content = content.content().to_str()?;
        let context = this.evaluate_context;

        let webpack_loaders_executor = webpack_loaders_executor(context);
        let resource_fs_path = this.source.ident().path().await?;
        let resource_path = resource_fs_path.path.as_str();
        let loaders = this.loaders.await?;
        let config_value = evaluate(
            webpack_loaders_executor,
            project_path,
            env,
            this.source.ident(),
            context,
            chunking_context,
            None,
            vec![
                JsonValueVc::cell(content.into()),
                JsonValueVc::cell(resource_path.into()),
                JsonValueVc::cell(json!(*loaders)),
            ],
            CompletionVc::immutable(),
            /* debug */ false,
        )
        .await?;

        let SingleValue::Single(val) = config_value.try_into_single().await? else {
            // An error happened, which has already been converted into an issue.
            return Ok(ProcessWebpackLoadersResult {
                content: AssetContent::File(FileContent::NotFound.cell()).cell(),
                assets: Vec::new()
            }.cell());
        };
        let processed: WebpackLoadersProcessingResult = parse_json_with_source_context(
            val.to_str()?,
        )
        .context("Unable to deserializate response from webpack loaders transform operation")?;

        // TODO handle SourceMap
        let file = File::from(processed.source);
        let assets = emitted_assets_to_virtual_assets(processed.assets);
        let content = AssetContent::File(FileContent::Content(file).cell()).cell();
        Ok(ProcessWebpackLoadersResult { content, assets }.cell())
    }
}
