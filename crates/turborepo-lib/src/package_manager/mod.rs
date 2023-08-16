mod npm;
mod pnpm;
mod yarn;

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Result};
use itertools::{Either, Itertools};
use regex::Regex;
use serde::{Deserialize, Serialize};
use turbopath::AbsoluteSystemPath;
use wax::{Any, Glob, Pattern};

use crate::{
    commands::CommandBase,
    package_json::PackageJson,
    package_manager::{npm::NpmDetector, pnpm::PnpmDetector, yarn::YarnDetector},
    ui::UNDERLINE,
};

#[derive(Debug, Deserialize)]
struct PnpmWorkspace {
    pub packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PackageJsonWorkspaces {
    workspaces: Workspaces,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(untagged)]
enum Workspaces {
    TopLevel(Vec<String>),
    Nested { packages: Vec<String> },
}

impl AsRef<[String]> for Workspaces {
    fn as_ref(&self) -> &[String] {
        match self {
            Workspaces::TopLevel(packages) => packages.as_slice(),
            Workspaces::Nested { packages } => packages.as_slice(),
        }
    }
}

impl From<Workspaces> for Vec<String> {
    fn from(value: Workspaces) -> Self {
        match value {
            Workspaces::TopLevel(packages) => packages,
            Workspaces::Nested { packages } => packages,
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Berry,
    Npm,
    Pnpm,
    Pnpm6,
    Yarn,
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Do not change these without also changing `GetPackageManager` in
        // packagemanager.go
        match self {
            PackageManager::Berry => write!(f, "berry"),
            PackageManager::Npm => write!(f, "npm"),
            PackageManager::Pnpm => write!(f, "pnpm"),
            PackageManager::Pnpm6 => write!(f, "pnpm6"),
            PackageManager::Yarn => write!(f, "yarn"),
        }
    }
}

#[derive(Debug)]
pub struct Globs {
    inclusions: Any<'static>,
    exclusions: Any<'static>,
    raw_inclusions: Vec<String>,
    raw_exclusions: Vec<String>,
}

impl PartialEq for Globs {
    fn eq(&self, other: &Self) -> bool {
        // Use the literals for comparison, not the compiled globs
        self.raw_inclusions == other.raw_inclusions && self.raw_exclusions == other.raw_exclusions
    }
}

impl Eq for Globs {}

impl Globs {
    pub fn new<S: Into<String>>(
        inclusions: Vec<S>,
        exclusions: Vec<S>,
    ) -> Result<Self, wax::BuildError> {
        // take ownership of the inputs
        let raw_inclusions: Vec<String> = inclusions
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<String>>();
        let raw_exclusions: Vec<String> = exclusions
            .into_iter()
            .map(|s| s.into())
            .collect::<Vec<String>>();
        let inclusion_globs = raw_inclusions
            .iter()
            .map(|s| Glob::new(s.as_ref()).map(|g| g.into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        let exclusion_globs = raw_exclusions
            .iter()
            .map(|s| Glob::new(s.as_ref()).map(|g| g.into_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            inclusions: wax::any(inclusion_globs)?,
            exclusions: wax::any(exclusion_globs)?,
            raw_inclusions,
            raw_exclusions,
        })
    }

    pub fn test(&self, root: &Path, target: PathBuf) -> Result<bool> {
        let search_value = target
            .strip_prefix(root)?
            .to_str()
            .ok_or_else(|| anyhow!("The relative path is not UTF8."))?;

        let includes = self.inclusions.is_match(search_value);
        let excludes = self.exclusions.is_match(search_value);

        Ok(includes && !excludes)
    }
}

impl PackageManager {
    /// Returns a list of globs for the package workspace.
    /// NOTE: We return a `Vec<PathBuf>` instead of a `GlobSet` because we
    /// may need to iterate through these globs and a `GlobSet` doesn't allow
    /// that.
    ///
    /// # Arguments
    ///
    /// * `root_path`:
    ///
    /// returns: Result<Option<Globs>, Error>
    ///
    /// # Examples
    ///
    /// ```
    /// ```
    pub fn get_workspace_globs(&self, root_path: &AbsoluteSystemPath) -> Result<Option<Globs>> {
        let globs = match self {
            PackageManager::Pnpm | PackageManager::Pnpm6 => {
                let workspace_yaml =
                    fs::read_to_string(root_path.join_component("pnpm-workspace.yaml"))?;
                let pnpm_workspace: PnpmWorkspace = serde_yaml::from_str(&workspace_yaml)?;
                if pnpm_workspace.packages.is_empty() {
                    return Ok(None);
                } else {
                    pnpm_workspace.packages
                }
            }
            PackageManager::Berry | PackageManager::Npm | PackageManager::Yarn => {
                let package_json_text =
                    fs::read_to_string(root_path.join_component("package.json"))?;
                let package_json: PackageJsonWorkspaces = serde_json::from_str(&package_json_text)?;

                if package_json.workspaces.as_ref().is_empty() {
                    return Ok(None);
                } else {
                    package_json.workspaces.into()
                }
            }
        };

        let (inclusions, exclusions) = globs.into_iter().partition_map(|glob| {
            if glob.starts_with('!') {
                Either::Right(glob[1..].to_string())
            } else {
                Either::Left(glob)
            }
        });

        match Globs::new(inclusions, exclusions) {
            Ok(globs) => Ok(Some(globs)),
            Err(err) => Err(anyhow!("Error building globs: {}", err)),
        }
    }

    pub fn get_package_manager(base: &CommandBase, pkg: Option<&PackageJson>) -> Result<Self> {
        // We don't surface errors for `read_package_manager` as we can fall back to
        // `detect_package_manager`
        if let Some(package_json) = pkg {
            if let Ok(Some(package_manager)) = Self::read_package_manager(package_json) {
                return Ok(package_manager);
            }
        }

        Self::detect_package_manager(base)
    }

    // Attempts to read the package manager from the package.json
    fn read_package_manager(pkg: &PackageJson) -> Result<Option<Self>> {
        let Some(package_manager) = &pkg.package_manager else {
            return Ok(None)
        };

        let (manager, version) = Self::parse_package_manager_string(package_manager)?;
        let version = version.parse()?;
        let manager = match manager {
            "npm" => Some(PackageManager::Npm),
            "yarn" => Some(YarnDetector::detect_berry_or_yarn(&version)?),
            "pnpm" => Some(PnpmDetector::detect_pnpm6_or_pnpm(&version)?),
            _ => None,
        };

        Ok(manager)
    }

    fn detect_package_manager(base: &CommandBase) -> Result<PackageManager> {
        let mut detected_package_managers = PnpmDetector::new(&base.repo_root)
            .chain(NpmDetector::new(&base.repo_root))
            .chain(YarnDetector::new(&base.repo_root))
            .collect::<Result<Vec<_>>>()?;

        match detected_package_managers.len() {
            0 => {
                let url = base.ui.apply(
                    UNDERLINE.apply_to("https://nodejs.org/api/packages.html#packagemanager"),
                );
                Err(anyhow!(
                    "We did not find a package manager specified in your root package.json. \
                     Please set the \"packageManager\" property in your root package.json ({url}) \
                     or run `npx @turbo/codemod add-package-manager` in the root of your monorepo."
                ))
            }
            1 => Ok(detected_package_managers.pop().unwrap()),
            _ => Err(anyhow!(
                "We detected multiple package managers in your repository: {}. Please remove one \
                 of them.",
                detected_package_managers.into_iter().join(", ")
            )),
        }
    }

    pub(crate) fn parse_package_manager_string(manager: &str) -> Result<(&str, &str)> {
        let package_manager_pattern =
            Regex::new(r"(?P<manager>npm|pnpm|yarn)@(?P<version>\d+\.\d+\.\d+(-.+)?)")?;
        if let Some(captures) = package_manager_pattern.captures(manager) {
            let manager = captures.name("manager").unwrap().as_str();
            let version = captures.name("version").unwrap().as_str();
            Ok((manager, version))
        } else {
            Err(anyhow!(
                "We could not parse packageManager field in package.json, expected: {}, received: \
                 {}",
                package_manager_pattern,
                manager
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tempfile::tempdir;
    use turbopath::AbsoluteSystemPathBuf;

    use super::*;
    use crate::{get_version, ui::UI, Args};

    struct TestCase {
        name: String,
        package_manager: String,
        expected_manager: String,
        expected_version: String,
        expected_error: bool,
    }

    #[test]
    fn test_parse_package_manager_string() {
        let tests = vec![
            TestCase {
                name: "errors with a tag version".to_owned(),
                package_manager: "npm@latest".to_owned(),
                expected_manager: "".to_owned(),
                expected_version: "".to_owned(),
                expected_error: true,
            },
            TestCase {
                name: "errors with no version".to_owned(),
                package_manager: "npm".to_owned(),
                expected_manager: "".to_owned(),
                expected_version: "".to_owned(),
                expected_error: true,
            },
            TestCase {
                name: "requires fully-qualified semver versions (one digit)".to_owned(),
                package_manager: "npm@1".to_owned(),
                expected_manager: "".to_owned(),
                expected_version: "".to_owned(),
                expected_error: true,
            },
            TestCase {
                name: "requires fully-qualified semver versions (two digits)".to_owned(),
                package_manager: "npm@1.2".to_owned(),
                expected_manager: "".to_owned(),
                expected_version: "".to_owned(),
                expected_error: true,
            },
            TestCase {
                name: "supports custom labels".to_owned(),
                package_manager: "npm@1.2.3-alpha.1".to_owned(),
                expected_manager: "npm".to_owned(),
                expected_version: "1.2.3-alpha.1".to_owned(),
                expected_error: false,
            },
            TestCase {
                name: "only supports specified package managers".to_owned(),
                package_manager: "pip@1.2.3".to_owned(),
                expected_manager: "".to_owned(),
                expected_version: "".to_owned(),
                expected_error: true,
            },
            TestCase {
                name: "supports npm".to_owned(),
                package_manager: "npm@0.0.1".to_owned(),
                expected_manager: "npm".to_owned(),
                expected_version: "0.0.1".to_owned(),
                expected_error: false,
            },
            TestCase {
                name: "supports pnpm".to_owned(),
                package_manager: "pnpm@0.0.1".to_owned(),
                expected_manager: "pnpm".to_owned(),
                expected_version: "0.0.1".to_owned(),
                expected_error: false,
            },
            TestCase {
                name: "supports yarn".to_owned(),
                package_manager: "yarn@111.0.1".to_owned(),
                expected_manager: "yarn".to_owned(),
                expected_version: "111.0.1".to_owned(),
                expected_error: false,
            },
        ];

        for case in tests {
            let result = PackageManager::parse_package_manager_string(&case.package_manager);
            let Ok((received_manager, received_version)) = result else {
                assert!(case.expected_error, "{}: received error", case.name);
                continue
            };

            assert_eq!(received_manager, case.expected_manager);
            assert_eq!(received_version, case.expected_version);
        }
    }

    #[test]
    fn test_read_package_manager() -> Result<()> {
        let mut package_json = PackageJson {
            package_manager: Some("npm@8.19.4".to_string()),
        };
        let package_manager = PackageManager::read_package_manager(&package_json)?;
        assert_eq!(package_manager, Some(PackageManager::Npm));

        package_json.package_manager = Some("yarn@2.0.0".to_string());
        let package_manager = PackageManager::read_package_manager(&package_json)?;
        assert_eq!(package_manager, Some(PackageManager::Berry));

        package_json.package_manager = Some("yarn@1.9.0".to_string());
        let package_manager = PackageManager::read_package_manager(&package_json)?;
        assert_eq!(package_manager, Some(PackageManager::Yarn));

        package_json.package_manager = Some("pnpm@6.0.0".to_string());
        let package_manager = PackageManager::read_package_manager(&package_json)?;
        assert_eq!(package_manager, Some(PackageManager::Pnpm6));

        package_json.package_manager = Some("pnpm@7.2.0".to_string());
        let package_manager = PackageManager::read_package_manager(&package_json)?;
        assert_eq!(package_manager, Some(PackageManager::Pnpm));

        Ok(())
    }

    #[test]
    fn test_detect_multiple_package_managers() -> Result<()> {
        let repo_root = tempdir()?;
        let repo_root_path = AbsoluteSystemPathBuf::new(repo_root.path())?;
        let base = CommandBase::new(
            Args::default(),
            repo_root_path,
            get_version(),
            UI::new(true),
        )?;

        let package_lock_json_path = repo_root.path().join(npm::LOCKFILE);
        File::create(&package_lock_json_path)?;
        let pnpm_lock_path = repo_root.path().join(pnpm::LOCKFILE);
        File::create(pnpm_lock_path)?;

        let error = PackageManager::detect_package_manager(&base).unwrap_err();
        assert_eq!(
            error.to_string(),
            "We detected multiple package managers in your repository: pnpm, npm. Please remove \
             one of them."
        );

        fs::remove_file(&package_lock_json_path)?;

        let package_manager = PackageManager::detect_package_manager(&base)?;
        assert_eq!(package_manager, PackageManager::Pnpm);

        Ok(())
    }

    #[test]
    fn test_get_workspace_globs() {
        let cwd = AbsoluteSystemPathBuf::cwd().unwrap();
        let repo_root = cwd
            .ancestors()
            .find(|path| path.join_component(".git").exists())
            .unwrap();
        let with_yarn = repo_root.join_components(&["examples", "with-yarn"]);
        let package_manager = PackageManager::Npm;
        let globs = package_manager
            .get_workspace_globs(with_yarn.as_absolute_path())
            .unwrap()
            .unwrap();

        let expected = Globs::new(vec!["apps/*", "packages/*"], vec![]).unwrap();
        assert_eq!(globs, expected);
    }

    #[test]
    fn test_globs_test() {
        struct TestCase {
            globs: Globs,
            root: PathBuf,
            target: PathBuf,
            output: Result<bool>,
        }

        let tests = [TestCase {
            globs: Globs::new(vec!["d/**".to_string()], vec![]).unwrap(),
            root: PathBuf::from("/a/b/c"),
            target: PathBuf::from("/a/b/c/d/e/f"),
            output: Ok(true),
        }];

        for test in tests {
            match test.globs.test(&test.root, test.target) {
                Ok(value) => assert_eq!(value, test.output.unwrap()),
                Err(value) => assert_eq!(value.to_string(), test.output.unwrap_err().to_string()),
            };
        }
    }

    #[test]
    fn test_nested_workspace_globs() -> Result<()> {
        let top_level: PackageJsonWorkspaces =
            serde_json::from_str("{ \"workspaces\": [\"packages/**\"]}")?;
        assert_eq!(top_level.workspaces.as_ref(), vec!["packages/**"]);
        let nested: PackageJsonWorkspaces =
            serde_json::from_str("{ \"workspaces\": {\"packages\": [\"packages/**\"]}}")?;
        assert_eq!(nested.workspaces.as_ref(), vec!["packages/**"]);
        Ok(())
    }
}
