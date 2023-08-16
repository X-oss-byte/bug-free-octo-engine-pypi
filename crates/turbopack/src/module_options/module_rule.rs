use anyhow::Result;
use serde::{Deserialize, Serialize};
use turbo_tasks::{trace::TraceRawVcs, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::Asset, plugin::CustomModuleType, reference_type::ReferenceType,
    source_transform::SourceTransforms,
};
use turbopack_css::CssInputTransforms;
use turbopack_ecmascript::{EcmascriptInputTransforms, EcmascriptOptions};
use turbopack_mdx::MdxTransformOptions;

use super::ModuleRuleCondition;

#[derive(Debug, Clone, Serialize, Deserialize, TraceRawVcs, PartialEq, Eq)]
pub struct ModuleRule {
    condition: ModuleRuleCondition,
    effects: Vec<ModuleRuleEffect>,
}

impl ModuleRule {
    pub fn new(condition: ModuleRuleCondition, effects: Vec<ModuleRuleEffect>) -> Self {
        ModuleRule { condition, effects }
    }

    pub fn effects(&self) -> impl Iterator<Item = &ModuleRuleEffect> {
        self.effects.iter()
    }

    pub async fn matches(
        &self,
        source: Vc<Box<dyn Asset>>,
        path: &FileSystemPath,
        reference_type: &ReferenceType,
    ) -> Result<bool> {
        self.condition.matches(source, path, reference_type).await
    }
}

#[turbo_tasks::value(shared)]
#[derive(Debug, Clone)]
pub enum ModuleRuleEffect {
    ModuleType(ModuleType),
    AddEcmascriptTransforms(Vc<EcmascriptInputTransforms>),
    SourceTransforms(Vc<SourceTransforms>),
}

#[turbo_tasks::value(serialization = "auto_for_input", shared)]
#[derive(PartialOrd, Ord, Hash, Debug, Copy, Clone)]
pub enum ModuleType {
    Ecmascript {
        transforms: Vc<EcmascriptInputTransforms>,
        #[turbo_tasks(trace_ignore)]
        options: EcmascriptOptions,
    },
    Typescript {
        transforms: Vc<EcmascriptInputTransforms>,
        #[turbo_tasks(trace_ignore)]
        options: EcmascriptOptions,
    },
    TypescriptWithTypes {
        transforms: Vc<EcmascriptInputTransforms>,
        #[turbo_tasks(trace_ignore)]
        options: EcmascriptOptions,
    },
    TypescriptDeclaration {
        transforms: Vc<EcmascriptInputTransforms>,
        #[turbo_tasks(trace_ignore)]
        options: EcmascriptOptions,
    },
    Json,
    Raw,
    Mdx {
        transforms: Vc<EcmascriptInputTransforms>,
        options: Vc<MdxTransformOptions>,
    },
    Css(Vc<CssInputTransforms>),
    CssModule(Vc<CssInputTransforms>),
    Static,
    Custom(Vc<Box<dyn CustomModuleType>>),
}
