use anyhow::Result;
use serde::{Deserialize, Serialize};
use turbo_tasks::{trace::TraceRawVcs, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    reference_type::ReferenceType, source::Source, source_transform::SourceTransforms,
};
use turbopack_css::{CssInputTransforms, CssModuleAssetType};
use turbopack_ecmascript::{EcmascriptInputTransforms, EcmascriptOptions};
use turbopack_mdx::MdxTransformOptions;
use turbopack_wasm::source::WebAssemblySourceType;

use super::{CustomModuleType, ModuleRuleCondition};

#[derive(Debug, Clone, Serialize, Deserialize, TraceRawVcs, PartialEq, Eq)]
pub struct ModuleRule {
    condition: ModuleRuleCondition,
    effects: Vec<ModuleRuleEffect>,
    match_mode: MatchMode,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TraceRawVcs)]
enum MatchMode {
    // Match all but internal references.
    NonInternal,
    // Only match internal references.
    Internal,
    // Match both internal and non-internal references.
    All,
}

impl MatchMode {
    fn matches(&self, reference_type: &ReferenceType) -> bool {
        matches!(
            (self, reference_type.is_internal()),
            (MatchMode::All, _) | (MatchMode::NonInternal, false) | (MatchMode::Internal, true)
        )
    }
}

impl ModuleRule {
    /// Creates a new module rule. Will not match internal references.
    pub fn new(condition: ModuleRuleCondition, effects: Vec<ModuleRuleEffect>) -> Self {
        ModuleRule {
            condition,
            effects,
            match_mode: MatchMode::NonInternal,
        }
    }

    /// Creates a new module rule. Will only matches internal references.
    pub fn new_internal(condition: ModuleRuleCondition, effects: Vec<ModuleRuleEffect>) -> Self {
        ModuleRule {
            condition,
            effects,
            match_mode: MatchMode::Internal,
        }
    }

    /// Creates a new module rule. Will only matches internal references.
    pub fn new_all(condition: ModuleRuleCondition, effects: Vec<ModuleRuleEffect>) -> Self {
        ModuleRule {
            condition,
            effects,
            match_mode: MatchMode::All,
        }
    }

    pub fn effects(&self) -> impl Iterator<Item = &ModuleRuleEffect> {
        self.effects.iter()
    }

    pub async fn matches(
        &self,
        source: Vc<Box<dyn Source>>,
        path: &FileSystemPath,
        reference_type: &ReferenceType,
    ) -> Result<bool> {
        Ok(self.match_mode.matches(reference_type)
            && self.condition.matches(source, path, reference_type).await?)
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
    CssGlobal,
    CssModule,
    Css {
        ty: CssModuleAssetType,
        transforms: Vc<CssInputTransforms>,
    },
    Static,
    WebAssembly {
        source_ty: WebAssemblySourceType,
    },
    Custom(Vc<Box<dyn CustomModuleType>>),
}
