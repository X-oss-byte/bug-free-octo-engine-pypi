use std::{
    collections::{HashMap, HashSet},
    fmt,
};

use petgraph::graph::{Graph, NodeIndex};
use tracing::warn;
use turbopath::{
    AbsoluteSystemPath, AbsoluteSystemPathBuf, AnchoredSystemPathBuf, RelativeUnixPathBuf,
};
use turborepo_lockfiles::Lockfile;

use super::{Entry, Package, PackageGraph, WorkspaceName, WorkspaceNode};
use crate::{package_json::PackageJson, package_manager::PackageManager};

pub struct PackageGraphBuilder<'a> {
    repo_root: &'a AbsoluteSystemPath,
    root_package_json: PackageJson,
    is_single_package: bool,
    package_manager: Option<PackageManager>,
    package_jsons: Option<HashMap<AbsoluteSystemPathBuf, PackageJson>>,
    lockfile: Option<Box<dyn Lockfile>>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not resolve workspaces: {0}")]
    PackageManager(#[from] crate::package_manager::Error),
    #[error(
        "Failed to add workspace \"{name}\" from \"{path}\", it already exists at \
         \"{existing_path}\""
    )]
    DuplicateWorkspace {
        name: String,
        path: String,
        existing_path: String,
    },
    #[error("path error: {0}")]
    TurboPath(#[from] turbopath::PathError),
    #[error("unable to parse workspace package.json: {0}")]
    PackageJson(#[from] crate::package_json::Error),
    #[error("package.json must have a name field")]
    PackageJsonMissingName,
    #[error(transparent)]
    Lockfile(#[from] turborepo_lockfiles::Error),
    #[error("TODO lockfile errors")]
    Todo,
}

impl<'a> PackageGraphBuilder<'a> {
    pub fn new(repo_root: &'a AbsoluteSystemPath, root_package_json: PackageJson) -> Self {
        Self {
            repo_root,
            root_package_json,
            is_single_package: false,
            package_manager: None,
            package_jsons: None,
            lockfile: None,
        }
    }

    pub fn with_single_package_mode(mut self, is_single: bool) -> Self {
        self.is_single_package = is_single;
        self
    }

    #[allow(dead_code)]
    pub fn with_package_manger(mut self, package_manager: Option<PackageManager>) -> Self {
        self.package_manager = package_manager;
        self
    }

    #[allow(dead_code)]
    pub fn with_package_jsons(
        mut self,
        package_jsons: Option<HashMap<AbsoluteSystemPathBuf, PackageJson>>,
    ) -> Self {
        self.package_jsons = package_jsons;
        self
    }

    #[allow(dead_code)]
    pub fn with_lockfile(mut self, lockfile: Option<Box<dyn Lockfile>>) -> Self {
        self.lockfile = lockfile;
        self
    }

    pub fn build(self) -> Result<PackageGraph, Error> {
        let is_single_package = self.is_single_package;
        let state = BuildState::new(self)?;
        match is_single_package {
            true => Ok(state.build_single_package_graph()),
            false => {
                let state = state.parse_package_jsons()?;
                let state = state.resolve_lockfile()?;
                Ok(state.build())
            }
        }
    }
}

struct BuildState<'a, S> {
    repo_root: &'a AbsoluteSystemPath,
    single: bool,
    package_manager: PackageManager,
    workspaces: HashMap<WorkspaceName, Entry>,
    workspace_graph: Graph<WorkspaceNode, ()>,
    node_lookup: HashMap<WorkspaceNode, NodeIndex>,
    lockfile: Option<Box<dyn Lockfile>>,
    package_jsons: Option<HashMap<AbsoluteSystemPathBuf, PackageJson>>,
    state: std::marker::PhantomData<S>,
}

// Allows us to perform workspace discovery and parse package jsons
enum ResolvedPackageManager {}

// Allows us to build the workspace graph and list over external dependencies
enum ResolvedWorkspaces {}

// Allows us to collect all transitive deps
enum ResolvedLockfile {}

impl<'a, S> BuildState<'a, S> {
    fn add_node(&mut self, node: WorkspaceNode) -> NodeIndex {
        let idx = self.workspace_graph.add_node(node.clone());
        self.node_lookup.insert(node, idx);
        idx
    }

    fn add_root_workspace(&mut self) {
        let root_index = self.add_node(WorkspaceNode::Root);
        let root_workspace = self.add_node(WorkspaceNode::Workspace(WorkspaceName::Root));
        self.workspace_graph
            .add_edge(root_workspace, root_index, ());
    }
}

impl<'a> BuildState<'a, ResolvedPackageManager> {
    fn new(
        builder: PackageGraphBuilder<'a>,
    ) -> Result<BuildState<'a, ResolvedPackageManager>, crate::package_manager::Error> {
        let PackageGraphBuilder {
            repo_root,
            root_package_json,
            is_single_package: single,
            package_manager,
            package_jsons,
            lockfile,
        } = builder;
        let package_manager = package_manager.map_or_else(
            || PackageManager::get_package_manager(repo_root, Some(&root_package_json)),
            Ok,
        )?;
        let mut workspaces = HashMap::new();
        workspaces.insert(
            WorkspaceName::Root,
            Entry {
                package_json: root_package_json,
                ..Default::default()
            },
        );

        Ok(BuildState {
            repo_root,
            single,
            package_manager,
            workspaces,
            lockfile,
            package_jsons,
            workspace_graph: Graph::new(),
            node_lookup: HashMap::new(),
            state: std::marker::PhantomData,
        })
    }

    fn add_json(
        &mut self,
        package_json_path: AbsoluteSystemPathBuf,
        json: PackageJson,
    ) -> Result<(), Error> {
        let relative_json_path =
            AnchoredSystemPathBuf::relative_path_between(self.repo_root, &package_json_path);
        let name = WorkspaceName::Other(json.name.clone().ok_or(Error::PackageJsonMissingName)?);
        let entry = Entry {
            package_json: json,
            package_json_path: relative_json_path,
            ..Default::default()
        };
        if let Some(existing) = self.workspaces.insert(name.clone(), entry) {
            let path = self
                .workspaces
                .get(&name)
                .expect("just inserted entry to be present")
                .package_json_path
                .clone();
            return Err(Error::DuplicateWorkspace {
                name: name.to_string(),
                path: path.to_string(),
                existing_path: existing.package_json_path.to_string(),
            });
        }
        self.add_node(WorkspaceNode::Workspace(name));
        Ok(())
    }

    // need our own type
    fn parse_package_jsons(mut self) -> Result<BuildState<'a, ResolvedWorkspaces>, Error> {
        // The root workspace will be present
        // we either read from disk or just read the map
        self.add_root_workspace();
        let package_jsons = self.package_jsons.take().map_or_else(
            || {
                // we need to parse the package jsons
                let mut jsons = HashMap::new();
                for path in self.package_manager.get_package_jsons(self.repo_root)? {
                    let json = PackageJson::load(&path)?;
                    jsons.insert(path, json);
                }
                Ok(jsons)
            },
            Result::<_, Error>::Ok,
        )?;

        for (path, json) in package_jsons {
            self.add_json(path, json)?;
        }

        let Self {
            repo_root,
            single,
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            lockfile,
            ..
        } = self;
        Ok(BuildState {
            repo_root,
            single,
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            lockfile,
            package_jsons: None,
            state: std::marker::PhantomData,
        })
    }

    fn build_single_package_graph(mut self) -> PackageGraph {
        self.add_root_workspace();
        let Self {
            single,
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            lockfile,
            ..
        } = self;
        debug_assert!(single, "expected single package graph");
        PackageGraph {
            workspace_graph,
            node_lookup,
            workspaces,
            package_manager,
            lockfile,
        }
    }
}

impl<'a> BuildState<'a, ResolvedWorkspaces> {
    fn connect_internal_dependencies(&mut self) -> Result<(), Error> {
        let split_deps = self
            .workspaces
            .iter()
            .map(|(name, entry)| {
                // TODO avoid clone
                (
                    name.clone(),
                    Dependencies::new(
                        self.repo_root,
                        &entry.package_json_path,
                        &self.workspaces,
                        entry.package_json.all_dependencies(),
                    ),
                )
            })
            .collect::<Vec<_>>();
        for (name, deps) in split_deps {
            let entry = self
                .workspaces
                .get_mut(&name)
                .expect("workspace present in ");
            let Dependencies { internal, external } = deps;
            let node_idx = self
                .node_lookup
                .get(&WorkspaceNode::Workspace(name))
                .expect("unable to find workspace node index");
            if internal.is_empty() {
                let root_idx = self
                    .node_lookup
                    .get(&WorkspaceNode::Root)
                    .expect("root node should have index");
                self.workspace_graph.add_edge(*node_idx, *root_idx, ());
            }
            for dependency in internal {
                let dependency_idx = self
                    .node_lookup
                    .get(&WorkspaceNode::Workspace(dependency))
                    .expect("unable to find workspace node index");
                self.workspace_graph
                    .add_edge(*node_idx, *dependency_idx, ());
            }
            entry.unresolved_external_dependencies = Some(external);
        }

        Ok(())
    }

    fn populate_lockfile(&mut self) -> Result<Box<dyn Lockfile>, Error> {
        // TODO actual lockfile parsing
        self.lockfile.take().map_or_else(|| Err(Error::Todo), Ok)
    }

    fn resolve_lockfile(mut self) -> Result<BuildState<'a, ResolvedLockfile>, Error> {
        self.connect_internal_dependencies()?;

        let lockfile = match self.populate_lockfile() {
            Ok(lockfile) => Some(lockfile),
            Err(e) => {
                warn!(
                    "Issues occurred when constructing package graph. Turbo will function, but \
                     some features may not be available: {}",
                    e
                );
                None
            }
        };

        let Self {
            repo_root,
            single,
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            ..
        } = self;
        Ok(BuildState {
            repo_root,
            single,
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            lockfile,
            package_jsons: None,
            state: std::marker::PhantomData,
        })
    }
}

impl<'a> BuildState<'a, ResolvedLockfile> {
    fn all_external_dependencies(&self) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        self.workspaces
            .values()
            .map(|entry| {
                let workspace_path = entry.package_json_path.to_unix()?;
                let workspace_string = workspace_path.as_str();
                let external_deps = entry
                    .unresolved_external_dependencies
                    .as_ref()
                    .map(|deps| {
                        deps.iter()
                            .map(|Package { name, version }| {
                                (name.to_string(), version.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok((workspace_string.to_string(), external_deps))
            })
            .collect()
    }

    fn populate_transitive_dependencies(&mut self) -> Result<(), Error> {
        let Some(lockfile) = self
            .lockfile
            .as_deref() else {
                return Ok(())
            };

        let mut closures = turborepo_lockfiles::all_transitive_closures(
            lockfile,
            self.all_external_dependencies()?,
        )?;
        for (_, entry) in self.workspaces.iter_mut() {
            entry.transitive_dependencies = closures.remove(&entry.unix_dir_str()?);
        }
        Ok(())
    }

    fn build(mut self) -> PackageGraph {
        if let Err(e) = self.populate_transitive_dependencies() {
            warn!("Unable to calculate transitive closures: {}", e);
        }
        let Self {
            package_manager,
            workspaces,
            workspace_graph,
            node_lookup,
            lockfile,
            ..
        } = self;
        PackageGraph {
            workspace_graph,
            node_lookup,
            workspaces,
            package_manager,
            lockfile,
        }
    }
}

struct Dependencies {
    internal: HashSet<WorkspaceName>,
    external: HashSet<Package>,
}

impl Dependencies {
    pub fn new<'a, I: IntoIterator<Item = (&'a String, &'a String)>>(
        repo_root: &AbsoluteSystemPath,
        workspace_json_path: &AnchoredSystemPathBuf,
        workspaces: &HashMap<WorkspaceName, Entry>,
        dependencies: I,
    ) -> Self {
        let resolved_workspace_json_path = repo_root.resolve(workspace_json_path);
        let workspace_dir = resolved_workspace_json_path
            .parent()
            .expect("package.json path should have parent");
        let mut internal = HashSet::new();
        let mut external = HashSet::new();
        for (name, version) in dependencies.into_iter() {
            // TODO implement borrowing for workspaces to allow for zero copy queries
            let workspace_name = WorkspaceName::Other(name.clone());
            let is_internal = workspaces
                .get(&workspace_name)
                // This is the current Go behavior, in the future we might not want to paper over a
                // missing version
                .map(|e| e.package_json.version.as_deref().unwrap_or_default())
                .map_or(false, |workspace_version| {
                    DependencyVersion::new(version).matches_workspace_package(
                        workspace_version,
                        &workspace_dir,
                        repo_root,
                    )
                });
            if is_internal {
                internal.insert(workspace_name);
            } else {
                external.insert(Package {
                    name: name.clone(),
                    version: version.clone(),
                });
            }
        }
        Self { internal, external }
    }
}

struct DependencyVersion<'a> {
    protocol: Option<&'a str>,
    version: &'a str,
}

impl<'a> DependencyVersion<'a> {
    fn new(qualified_version: &'a str) -> Self {
        qualified_version.split_once(':').map_or(
            Self {
                protocol: None,
                version: qualified_version,
            },
            |(protocol, version)| Self {
                protocol: Some(protocol),
                version,
            },
        )
    }

    fn is_external(&self) -> bool {
        // The npm protocol for yarn by default still uses the workspace package if the
        // workspace version is in a compatible semver range. See https://github.com/yarnpkg/berry/discussions/4015
        // For now, we will just assume if the npm protocol is being used and the
        // version matches its an internal dependency which matches the existing
        // behavior before this additional logic was added.

        // TODO: extend this to support the `enableTransparentWorkspaces` yarn option
        self.protocol.map_or(false, |p| p != "npm")
    }

    fn matches_workspace_package(
        &self,
        package_version: &str,
        cwd: &AbsoluteSystemPath,
        root: &AbsoluteSystemPath,
    ) -> bool {
        match self.protocol {
            Some("workspace") => {
                // TODO: Since support at the moment is non-existent for workspaces that contain
                // multiple versions of the same package name, just assume its a
                // match and don't check the range for an exact match.
                true
            }
            Some("file") | Some("link") => {
                // Default to internal if we have the package but somehow cannot get the path
                RelativeUnixPathBuf::new(self.version)
                    .and_then(|file_path| cwd.join_unix_path(file_path))
                    .map_or(true, |dep_path| root.contains(&dep_path))
            }
            Some(_) if self.is_external() => {
                // Other protocols are assumed to be external references ("github:", etc)
                false
            }
            _ if self.version == "*" => true,
            _ => {
                // If we got this far, then we need to check the workspace package version to
                // see it satisfies the dependencies range to determin whether
                // or not its an internal or external dependency.
                let constraint = node_semver::Range::parse(self.version);
                let version = node_semver::Version::parse(package_version);

                // For backwards compatibility with existing behavior, if we can't parse the
                // version then we treat the dependency as an internal package
                // reference and swallow the error.

                // TODO: some package managers also support tags like "latest". Does extra
                // handling need to be added for this corner-case
                constraint
                    .ok()
                    .zip(version.ok())
                    .map_or(true, |(constraint, version)| constraint.satisfies(&version))
            }
        }
    }
}

impl<'a> fmt::Display for DependencyVersion<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.protocol {
            Some(protocol) => f.write_fmt(format_args!("{}:{}", protocol, self.version)),
            None => f.write_str(self.version),
        }
    }
}

impl Entry {
    fn unix_dir_str(&self) -> Result<String, Error> {
        let unix = self.package_json_path.to_unix()?;
        Ok(unix.to_string())
    }
}

#[cfg(test)]
mod test {
    use test_case::test_case;
    use turbopath::AbsoluteSystemPathBuf;

    use super::*;

    #[test_case("1.2.3", "1.2.3", true ; "handles exact match")]
    #[test_case("1.2.3", "^1.0.0", true ; "handles semver range satisfied")]
    #[test_case("2.3.4", "^1.0.0", false ; "handles semver range not satisfied")]
    #[test_case("1.2.3", "workspace:1.2.3", true ; "handles workspace protocol with version")]
    #[test_case("1.2.3", "workspace:*", true ; "handles workspace protocol with no version")]
    #[test_case("1.2.3", "workspace:../other-packages/", true ; "handles workspace protocol with relative path")]
    #[test_case("1.2.3", "npm:^1.2.3", true ; "handles npm protocol with satisfied semver range")]
    #[test_case("2.3.4", "npm:^1.2.3", false ; "handles npm protocol with not satisfied semver range")]
    #[test_case("1.2.3", "1.2.2-alpha-123abcd.0", false ; "handles pre-release versions")]
    // for backwards compatability with the code before versions were verified
    #[test_case("sometag", "1.2.3", true ; "handles non-semver package version")]
    // for backwards compatability with the code before versions were verified
    #[test_case("1.2.3", "sometag", true ; "handles non-semver dependency version")]
    #[test_case("1.2.3", "file:../libB", true ; "handles file:.. inside repo")]
    #[test_case("1.2.3", "file:../../../otherproject", false ; "handles file:.. outside repo")]
    #[test_case("1.2.3", "link:../libB", true ; "handles link:.. inside repo")]
    #[test_case("1.2.3", "link:../../../otherproject", false ; "handles link:.. outside repo")]
    #[test_case("0.0.0-development", "*", true ; "handles development versions")]
    fn test_matches_workspace_package(package_version: &str, range: &str, expected: bool) {
        let root = AbsoluteSystemPathBuf::new(if cfg!(windows) {
            "C:\\some\\repo"
        } else {
            "/some/repo"
        })
        .unwrap();
        let pkg_dir = root.join_components(&["packages", "libA"]);

        assert_eq!(
            DependencyVersion::new(range).matches_workspace_package(
                package_version,
                &pkg_dir,
                &root
            ),
            expected
        );
    }

    #[test]
    fn test_duplicate_package_names() {
        let root =
            AbsoluteSystemPathBuf::new(if cfg!(windows) { r"C:\repo" } else { "/repo" }).unwrap();
        let builder = PackageGraphBuilder::new(
            &root,
            PackageJson {
                name: Some("root".into()),
                ..Default::default()
            },
        )
        .with_package_manger(Some(PackageManager::Npm))
        .with_package_jsons(Some({
            let mut map = HashMap::new();
            map.insert(
                root.join_component("a"),
                PackageJson {
                    name: Some("foo".into()),
                    ..Default::default()
                },
            );
            map.insert(
                root.join_component("b"),
                PackageJson {
                    name: Some("foo".into()),
                    ..Default::default()
                },
            );
            map
        }));
        assert!(matches!(
            builder.build(),
            Err(Error::DuplicateWorkspace { .. })
        ))
    }
}
