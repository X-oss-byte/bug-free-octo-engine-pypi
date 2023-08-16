use std::{collections::BTreeMap, future::Future, pin::Pin};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use turbo_tasks::{
    debug::ValueDebugFormat, primitives::StringVc, trace::TraceRawVcs, TryJoinIterExt, Value,
    ValueToString, ValueToStringVc,
};
use turbo_tasks_fs::{glob::GlobVc, FileSystemPathVc};

use super::{
    alias_map::{AliasMap, AliasTemplate},
    AliasPattern, PrimaryResolveResult, ResolveResult, ResolveResultVc,
};
use crate::resolve::{parse::RequestVc, plugin::ResolvePluginVc};

#[turbo_tasks::value(shared)]
#[derive(Hash, Debug)]
pub struct LockedVersions {}

/// A location where to resolve modules.
#[derive(
    TraceRawVcs, Hash, PartialEq, Eq, Clone, Debug, Serialize, Deserialize, ValueDebugFormat,
)]
pub enum ResolveModules {
    /// when inside of path, use the list of directories to
    /// resolve inside these
    Nested(FileSystemPathVc, Vec<String>),
    /// look into that directory
    Path(FileSystemPathVc),
    /// lookup versions based on lockfile in the registry filesystem
    /// registry filesystem is assumed to have structure like
    /// @scope/module/version/<path-in-package>
    Registry(FileSystemPathVc, LockedVersionsVc),
}

#[derive(TraceRawVcs, Hash, PartialEq, Eq, Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ConditionValue {
    Set,
    Unset,
    Unknown,
}

impl From<bool> for ConditionValue {
    fn from(v: bool) -> Self {
        if v {
            ConditionValue::Set
        } else {
            ConditionValue::Unset
        }
    }
}

pub type ResolutionConditions = BTreeMap<String, ConditionValue>;

/// The different ways to resolve a package, as described in package.json.
#[derive(TraceRawVcs, Hash, PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum ResolveIntoPackage {
    /// Using the [exports] field.
    ///
    /// [exports]: https://nodejs.org/api/packages.html#exports
    ExportsField {
        conditions: ResolutionConditions,
        unspecified_conditions: ConditionValue,
    },
    /// Using a [main]-like field (e.g. [main], [module], [browser], etc.).
    ///
    /// [main]: https://nodejs.org/api/packages.html#main
    /// [module]: https://esbuild.github.io/api/#main-fields
    /// [browser]: https://esbuild.github.io/api/#main-fields
    MainField(String),
    /// Default behavior of using the index.js file at the root of the package.
    Default(String),
}

// The different ways to resolve a request withing a package
#[derive(TraceRawVcs, Hash, PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub enum ResolveInPackage {
    /// Using a alias field which allows to map requests
    AliasField(String),
    /// Using the [imports] field.
    ///
    /// [imports]: https://nodejs.org/api/packages.html#imports
    ImportsField {
        conditions: ResolutionConditions,
        unspecified_conditions: ConditionValue,
    },
}

#[turbo_tasks::value(shared)]
#[derive(Clone)]
pub enum ImportMapping {
    External(Option<String>),
    /// An already resolved result that will be returned directly.
    Direct(ResolveResultVc),
    /// A request alias that will be resolved first, and fall back to resolving
    /// the original request if it fails. Useful for the tsconfig.json
    /// `compilerOptions.paths` option and Next aliases.
    PrimaryAlternative(String, Option<FileSystemPathVc>),
    Ignore,
    Empty,
    Alternatives(Vec<ImportMappingVc>),
    Dynamic(ImportMappingReplacementVc),
}

impl ImportMapping {
    pub fn primary_alternatives(
        list: Vec<String>,
        context: Option<FileSystemPathVc>,
    ) -> ImportMapping {
        if list.is_empty() {
            ImportMapping::Ignore
        } else if list.len() == 1 {
            ImportMapping::PrimaryAlternative(list.into_iter().next().unwrap(), context)
        } else {
            ImportMapping::Alternatives(
                list.into_iter()
                    .map(|s| ImportMapping::PrimaryAlternative(s, context).cell())
                    .collect(),
            )
        }
    }
}

impl AliasTemplate for ImportMappingVc {
    type Output<'a> = Pin<Box<dyn Future<Output = Result<ImportMappingVc>> + Send + 'a>>;

    fn replace<'a>(&'a self, capture: &'a str) -> Self::Output<'a> {
        Box::pin(async move {
            let this = &*self.await?;
            Ok(match this {
                ImportMapping::External(name) => {
                    if let Some(name) = name {
                        ImportMapping::External(Some(name.clone().replace('*', capture)))
                    } else {
                        ImportMapping::External(None)
                    }
                }
                ImportMapping::PrimaryAlternative(name, context) => {
                    ImportMapping::PrimaryAlternative(name.clone().replace('*', capture), *context)
                }
                ImportMapping::Direct(_) | ImportMapping::Ignore | ImportMapping::Empty => {
                    this.clone()
                }
                ImportMapping::Alternatives(alternatives) => ImportMapping::Alternatives(
                    alternatives
                        .iter()
                        .map(|mapping| mapping.replace(capture))
                        .try_join()
                        .await?,
                ),
                ImportMapping::Dynamic(replacement) => {
                    (*replacement.replace(capture).await?).clone()
                }
            }
            .cell())
        })
    }
}

#[turbo_tasks::value(shared)]
#[derive(Clone, Default)]
pub struct ImportMap {
    map: AliasMap<ImportMappingVc>,
}

impl ImportMap {
    /// Creates a new import map.
    pub fn new(map: AliasMap<ImportMappingVc>) -> ImportMap {
        Self { map }
    }

    /// Creates a new empty import map.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Extends the import map with another import map.
    pub fn extend(&mut self, other: &ImportMap) {
        let Self { map } = other.clone();
        self.map.extend(map);
    }

    /// Inserts an alias into the import map.
    pub fn insert_alias(&mut self, alias: AliasPattern, mapping: ImportMappingVc) {
        self.map.insert(alias, mapping);
    }

    /// Inserts an exact alias into the import map.
    pub fn insert_exact_alias<'a>(
        &mut self,
        pattern: impl Into<String> + 'a,
        mapping: ImportMappingVc,
    ) {
        self.map.insert(AliasPattern::exact(pattern), mapping);
    }

    /// Inserts a wildcard alias into the import map.
    pub fn insert_wildcard_alias<'a>(
        &mut self,
        prefix: impl Into<String> + 'a,
        mapping: ImportMappingVc,
    ) {
        self.map.insert(AliasPattern::wildcard(prefix, ""), mapping);
    }

    /// Inserts a wildcard alias with suffix into the import map.
    pub fn insert_wildcard_alias_with_suffix<'p, 's>(
        &mut self,
        prefix: impl Into<String> + 'p,
        suffix: impl Into<String> + 's,
        mapping: ImportMappingVc,
    ) {
        self.map
            .insert(AliasPattern::wildcard(prefix, suffix), mapping);
    }

    /// Inserts an alias that resolves an prefix always from a certain location
    /// to create a singleton.
    pub fn insert_singleton_alias<'a>(
        &mut self,
        prefix: impl Into<String> + 'a,
        context_path: FileSystemPathVc,
    ) {
        let prefix = prefix.into();
        let wildcard_prefix = prefix.clone() + "/";
        let wildcard_alias: String = prefix.clone() + "/*";
        self.insert_exact_alias(
            &prefix,
            ImportMapping::PrimaryAlternative(prefix.clone(), Some(context_path)).cell(),
        );
        self.insert_wildcard_alias(
            wildcard_prefix,
            ImportMapping::PrimaryAlternative(wildcard_alias, Some(context_path)).cell(),
        );
    }
}

#[turbo_tasks::value_impl]
impl ImportMapVc {
    /// Extends the underlying [ImportMap] with another [ImportMap].
    #[turbo_tasks::function]
    pub async fn extend(self, other: ImportMapVc) -> Result<Self> {
        let mut import_map = self.await?.clone_value();
        import_map.extend(&*other.await?);
        Ok(import_map.cell())
    }
}

#[turbo_tasks::value(shared)]
#[derive(Clone, Default)]
pub struct ResolvedMap {
    pub by_glob: Vec<(FileSystemPathVc, GlobVc, ImportMappingVc)>,
}

#[turbo_tasks::value(shared)]
#[derive(Clone, Debug)]
pub enum ImportMapResult {
    Result(ResolveResultVc),
    Alias(RequestVc, Option<FileSystemPathVc>),
    Alternatives(Vec<ImportMapResult>),
    NoEntry,
}

async fn import_mapping_to_result(
    mapping: ImportMappingVc,
    context: FileSystemPathVc,
    request: RequestVc,
) -> Result<ImportMapResult> {
    Ok(match &*mapping.await? {
        ImportMapping::Direct(result) => ImportMapResult::Result(*result),
        ImportMapping::External(name) => ImportMapResult::Result(
            ResolveResult::primary_with_references(
                name.as_ref().map_or_else(
                    || PrimaryResolveResult::OriginalReferenceExternal,
                    |req| PrimaryResolveResult::OriginalReferenceTypeExternal(req.to_string()),
                ),
                Vec::new(),
            )
            .into(),
        ),
        ImportMapping::Ignore => {
            ImportMapResult::Result(ResolveResult::primary(PrimaryResolveResult::Ignore).into())
        }
        ImportMapping::Empty => {
            ImportMapResult::Result(ResolveResult::primary(PrimaryResolveResult::Empty).into())
        }
        ImportMapping::PrimaryAlternative(name, context) => {
            let request = RequestVc::parse(Value::new(name.to_string().into()));

            ImportMapResult::Alias(request, *context)
        }
        ImportMapping::Alternatives(list) => ImportMapResult::Alternatives(
            list.iter()
                .map(|mapping| import_mapping_to_result_boxed(*mapping, context, request))
                .try_join()
                .await?,
        ),
        ImportMapping::Dynamic(replacement) => {
            (*replacement.result(context, request).await?).clone()
        }
    })
}

#[turbo_tasks::value_impl]
impl ValueToString for ImportMapResult {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        match self {
            ImportMapResult::Result(_) => Ok(StringVc::cell("Resolved by import map".to_string())),
            ImportMapResult::Alias(request, context) => {
                let s = if let Some(path) = context {
                    format!(
                        "aliased to {} inside of {}",
                        request.to_string().await?,
                        path.to_string().await?
                    )
                } else {
                    format!("aliased to {}", request.to_string().await?)
                };
                Ok(StringVc::cell(s))
            }
            ImportMapResult::Alternatives(alternatives) => {
                let strings = alternatives
                    .iter()
                    .map(|alternative| alternative.clone().cell().to_string())
                    .try_join()
                    .await?;
                let strings = strings
                    .iter()
                    .map(|string| string.as_str())
                    .collect::<Vec<_>>();
                Ok(StringVc::cell(strings.join(" | ")))
            }
            ImportMapResult::NoEntry => Ok(StringVc::cell("No import map entry".to_string())),
        }
    }
}

// This cannot be inlined within `import_mapping_to_result`, otherwise we run
// into the following error:
//     cycle detected when computing type of
//     `resolve::options::import_mapping_to_result::{opaque#0}`
fn import_mapping_to_result_boxed(
    mapping: ImportMappingVc,
    context: FileSystemPathVc,
    request: RequestVc,
) -> Pin<Box<dyn Future<Output = Result<ImportMapResult>> + Send>> {
    Box::pin(async move { import_mapping_to_result(mapping, context, request).await })
}

#[turbo_tasks::value_impl]
impl ImportMapVc {
    #[turbo_tasks::function]
    pub async fn lookup(
        self,
        context: FileSystemPathVc,
        request: RequestVc,
    ) -> Result<ImportMapResultVc> {
        let this = self.await?;
        // TODO lookup pattern
        if let Some(request_string) = request.await?.request() {
            if let Some(result) = this.map.lookup(&request_string).next() {
                return Ok(import_mapping_to_result(
                    result.try_join_into_self().await?.into_owned(),
                    context,
                    request,
                )
                .await?
                .into());
            }
        }
        Ok(ImportMapResult::NoEntry.into())
    }
}

#[turbo_tasks::value_impl]
impl ResolvedMapVc {
    #[turbo_tasks::function]
    pub async fn lookup(
        self,
        resolved: FileSystemPathVc,
        context: FileSystemPathVc,
        request: RequestVc,
    ) -> Result<ImportMapResultVc> {
        let this = self.await?;
        let resolved = resolved.await?;
        for (root, glob, mapping) in this.by_glob.iter() {
            let root = root.await?;
            if let Some(path) = root.get_path_to(&resolved) {
                if glob.await?.execute(path) {
                    return Ok(import_mapping_to_result(*mapping, context, request)
                        .await?
                        .into());
                }
            }
        }
        Ok(ImportMapResult::NoEntry.into())
    }
}

#[turbo_tasks::value(shared)]
#[derive(Clone, Debug, Default)]
pub struct ResolveOptions {
    pub extensions: Vec<String>,
    /// The locations where to resolve modules.
    pub modules: Vec<ResolveModules>,
    /// How to resolve packages.
    pub into_package: Vec<ResolveIntoPackage>,
    /// How to resolve in packages.
    pub in_package: Vec<ResolveInPackage>,
    /// An import map to use before resolving a request.
    pub import_map: Option<ImportMapVc>,
    /// An import map to use when a request is otherwise unresolveable.
    pub fallback_import_map: Option<ImportMapVc>,
    pub resolved_map: Option<ResolvedMapVc>,
    pub plugins: Vec<ResolvePluginVc>,
    pub placeholder_for_future_extensions: (),
}

#[turbo_tasks::value_impl]
impl ResolveOptionsVc {
    #[turbo_tasks::function]
    pub async fn modules(self) -> Result<ResolveModulesOptionsVc> {
        Ok(ResolveModulesOptions {
            modules: self.await?.modules.clone(),
        }
        .into())
    }

    /// Returns a new [ResolveOptionsVc] with its import map extended to include
    /// the given import map.
    #[turbo_tasks::function]
    pub async fn with_extended_import_map(self, import_map: ImportMapVc) -> Result<Self> {
        let mut resolve_options = self.await?.clone_value();
        resolve_options.import_map = Some(
            resolve_options
                .import_map
                .map(|current_import_map| current_import_map.extend(import_map))
                .unwrap_or(import_map),
        );
        Ok(resolve_options.into())
    }

    /// Returns a new [ResolveOptionsVc] with its fallback import map extended
    /// to include the given import map.
    #[turbo_tasks::function]
    pub async fn with_extended_fallback_import_map(self, import_map: ImportMapVc) -> Result<Self> {
        let mut resolve_options = self.await?.clone_value();
        resolve_options.fallback_import_map = Some(
            resolve_options
                .fallback_import_map
                .map(|current_import_map| current_import_map.extend(import_map))
                .unwrap_or(import_map),
        );
        Ok(resolve_options.into())
    }
}

#[turbo_tasks::value(shared)]
#[derive(Hash, Clone, Debug)]
pub struct ResolveModulesOptions {
    pub modules: Vec<ResolveModules>,
}

#[turbo_tasks::function]
pub async fn resolve_modules_options(options: ResolveOptionsVc) -> Result<ResolveModulesOptionsVc> {
    Ok(ResolveModulesOptions {
        modules: options.await?.modules.clone(),
    }
    .into())
}

#[turbo_tasks::value_trait]
pub trait ImportMappingReplacement {
    fn replace(&self, capture: &str) -> ImportMappingVc;
    fn result(&self, context: FileSystemPathVc, request: RequestVc) -> ImportMapResultVc;
}
