use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use git2::{DiffFormat, DiffOptions, Repository};
use path_slash::PathBufExt;
use turborepo_paths::{
    fs_util, project::ProjectRoot, project_relative_path::ProjectRelativePath,
    AbsoluteNormalizedPath, AbsoluteNormalizedPathBuf,
};

use crate::Error;

/// Finds the changed files in a repository between index and working directory
/// (unstaged changes) and between two commits. Includes untracked files,
/// i.e. files not yet in git
///
/// # Arguments
///
/// * `repo_root`: The root of the repository. Guaranteed to be the root.
/// * `commit_range`: If Some, the range of commits that should be searched for
///   changes
/// * `monorepo_root`: The path to which the results should be relative. Must be
///   an absolute path
///
/// returns: Result<HashSet<String, RandomState>, Error>
pub fn changed_files(
    git_repo_root: PathBuf,
    turborepo_root: PathBuf,
    commit_range: Option<(&str, &str)>,
) -> Result<HashSet<String>, Error> {
    // Initialize repository at repo root
    let repo = Repository::open(&git_repo_root)?;
    let git_repo_root = ProjectRoot::new(fs_util::canonicalize(git_repo_root)?)?;

    let turborepo_root = fs_util::canonicalize(turborepo_root)?;
    let relative_path_to_turborepo_root = git_repo_root.relativize(&turborepo_root)?;

    let mut files = HashSet::new();
    add_changed_files_from_unstaged_changes(
        &git_repo_root,
        &repo,
        relative_path_to_turborepo_root.as_ref(),
        &turborepo_root,
        &mut files,
    )?;

    if let Some((from_commit, to_commit)) = commit_range {
        add_changed_files_from_commits(
            &git_repo_root,
            &repo,
            relative_path_to_turborepo_root.as_ref(),
            &turborepo_root,
            &mut files,
            from_commit,
            to_commit,
        )?;
    }

    Ok(files)
}

// Gets the system version of `monorepo_root` and `file_path` by calling
// `fs_util::canonicalize`, then strips the `monorepo_root` from the
// `file_path`.
fn get_stripped_system_file_path(
    repo_root: &ProjectRoot,
    file_path: &Path,
    turborepo_root: &AbsoluteNormalizedPath,
) -> Result<PathBuf, Error> {
    // We know the path is relative to the repo root so we can convert it to a
    // ProjectRelativePath
    let project_relative_file_path = ProjectRelativePath::new(file_path)?;
    // Which we then resolve to an absolute path
    let absolute_file_path = repo_root.resolve(project_relative_file_path);
    // Then we call canonicalize to get a system path instead of a Unix style path
    let path = {
        let raw_path_str = absolute_file_path
            .as_os_str()
            .to_str()
            .ok_or(Error::PathError(anyhow!(
                "failed to convert path from git: {}",
                absolute_file_path.display()
            )))?;
        AbsoluteNormalizedPathBuf::new(PathBuf::from_slash(raw_path_str))?
    };

    // NOTE: In the original Go code, `Rel` works even if the base path is not a
    // prefix of the original path. In Rust, `strip_prefix` returns an
    // error if the base path is not a prefix of the original path.
    // However since we're passing a pathspec to `git2` we know that the
    // base path is a prefix of the original path.
    Ok(path.as_path().strip_prefix(turborepo_root)?.to_path_buf())
}

fn add_changed_files_from_unstaged_changes(
    git_repo_root: &ProjectRoot,
    repo: &Repository,
    relative_path_to_turborepo_root: &ProjectRelativePath,
    turborepo_root: &AbsoluteNormalizedPath,
    files: &mut HashSet<String>,
) -> Result<(), Error> {
    let mut options = DiffOptions::new();
    options.include_untracked(true);
    options.recurse_untracked_dirs(true);

    options.pathspec(relative_path_to_turborepo_root.to_string());

    let diff = repo.diff_index_to_workdir(None, Some(&mut options))?;

    for delta in diff.deltas() {
        let file = delta.old_file();
        if let Some(file_path) = file.path() {
            let stripped_file_path =
                get_stripped_system_file_path(git_repo_root, file_path, turborepo_root)?;

            files.insert(
                stripped_file_path
                    .to_str()
                    .ok_or_else(|| Error::NonUtf8Path(stripped_file_path.to_path_buf()))?
                    .to_string(),
            );
        }
    }

    Ok(())
}

fn add_changed_files_from_commits(
    git_repo_root: &ProjectRoot,
    repo: &Repository,
    relative_path_to_turborepo_root: &ProjectRelativePath,
    turborepo_root: &AbsoluteNormalizedPath,
    files: &mut HashSet<String>,
    from_commit: &str,
    to_commit: &str,
) -> Result<(), Error> {
    let from_commit_ref = repo.revparse_single(from_commit)?;
    let to_commit_ref = repo.revparse_single(to_commit)?;
    let from_commit = from_commit_ref.peel_to_commit()?;
    let to_commit = to_commit_ref.peel_to_commit()?;
    let from_tree = from_commit.tree()?;
    let to_tree = to_commit.tree()?;

    let mut options = DiffOptions::new();
    options.pathspec(relative_path_to_turborepo_root.to_string());

    let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), Some(&mut options))?;
    diff.print(DiffFormat::NameOnly, |_, _, _| true)?;

    for delta in diff.deltas() {
        let file = delta.old_file();
        if let Some(file_path) = file.path() {
            let stripped_path =
                get_stripped_system_file_path(git_repo_root, file_path, turborepo_root)?;

            files.insert(
                stripped_path
                    .to_str()
                    .ok_or_else(|| Error::NonUtf8Path(stripped_path.to_path_buf()))?
                    .to_string(),
            );
        }
    }

    Ok(())
}

/// Finds the content of a file at a previous commit. Assumes file is in a git
/// repository
///
/// # Arguments
///
/// * `repo_root`: The root of the repository
/// * `from_commit`: The commit hash to checkout
/// * `file_path`: The path to the file
///
/// returns: Result<String, Error>
pub fn previous_content(
    repo_root: PathBuf,
    from_commit: &str,
    file_path: PathBuf,
) -> Result<Vec<u8>, Error> {
    let repo = Repository::open(&repo_root)?;
    let repo_root = ProjectRoot::new(fs_util::canonicalize(repo_root)?)?;
    let from_commit_ref = repo.revparse_single(from_commit)?;
    let from_commit = from_commit_ref.peel_to_commit()?;
    let from_tree = from_commit.tree()?;

    // Canonicalize so strip_prefix works properly
    let file_path = fs_util::canonicalize(file_path)?;

    let relative_path = repo_root.relativize(&file_path)?;

    let file = from_tree.get_path(Path::new(relative_path.as_str()))?;
    let blob = repo.find_blob(file.id())?;
    let content = blob.content();

    Ok(content.to_vec())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        env::set_current_dir,
        fs,
        path::{Path, PathBuf},
    };

    use git2::{Oid, Repository};

    use super::{changed_files, previous_content};
    use crate::Error;

    fn commit_file(
        repo: &Repository,
        path: &Path,
        previous_commit: Option<Oid>,
    ) -> Result<Oid, Error> {
        let mut index = repo.index()?;
        index.add_path(path)?;
        let tree_oid = index.write_tree()?;
        index.write()?;
        let tree = repo.find_tree(tree_oid)?;
        let previous_commit = previous_commit
            .map(|oid| repo.find_commit(oid))
            .transpose()?;

        Ok(repo.commit(
            Some("HEAD"),
            &repo.signature()?,
            &repo.signature()?,
            "Commit",
            &tree,
            previous_commit
                .as_ref()
                .as_ref()
                .map(std::slice::from_ref)
                .unwrap_or_default(),
        )?)
    }

    fn commit_delete(repo: &Repository, path: &Path, previous_commit: Oid) -> Result<Oid, Error> {
        let mut index = repo.index()?;
        index.remove_path(path)?;
        let tree_oid = index.write_tree()?;
        index.write()?;
        let tree = repo.find_tree(tree_oid)?;
        let previous_commit = repo.find_commit(previous_commit)?;

        Ok(repo.commit(
            Some("HEAD"),
            &repo.signature()?,
            &repo.signature()?,
            "Commit",
            &tree,
            std::slice::from_ref(&&previous_commit),
        )?)
    }

    #[test]
    fn test_changed_files() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let monorepo_root = repo_root.path();
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;
        let file = repo_root.path().join("foo.js");
        fs::write(file, "let z = 0;")?;

        // First commit (we need a base commit to compare against)
        let first_commit_oid = commit_file(&repo, Path::new("foo.js"), None)?;

        // Now change another file
        let new_file = repo_root.path().join("bar.js");
        fs::write(new_file, "let y = 1;")?;

        // Test that uncommitted file is marked as changed
        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            monorepo_root.to_path_buf(),
            None,
        )?;
        assert_eq!(files, HashSet::from(["bar.js".to_string()]));

        // Now commit file
        let second_commit_oid = commit_file(&repo, Path::new("bar.js"), Some(first_commit_oid))?;

        // Test that only second file is marked as changed when we check commit range
        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            monorepo_root.to_path_buf(),
            Some((
                first_commit_oid.to_string().as_str(),
                second_commit_oid.to_string().as_str(),
            )),
        )?;
        assert_eq!(files, HashSet::from(["bar.js".to_string()]));

        // Create a file nested in subdir
        fs::create_dir_all(repo_root.path().join("subdir"))?;
        let new_file = repo_root.path().join("subdir").join("baz.js");
        fs::write(new_file, "let x = 2;")?;

        // Test that `monorepo_root` filters out files not in the specified directory
        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            repo_root.path().join("subdir"),
            Some((
                first_commit_oid.to_string().as_str(),
                second_commit_oid.to_string().as_str(),
            )),
        )?;
        assert_eq!(files, HashSet::from(["baz.js".to_string()]));

        Ok(())
    }

    #[test]
    fn test_changed_files_with_root_as_relative() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;
        let file = repo_root.path().join("foo.js");
        fs::write(file, "let z = 0;")?;

        // First commit (we need a base commit to compare against)
        commit_file(&repo, Path::new("foo.js"), None)?;

        // Now change another file
        let new_file = repo_root.path().join("bar.js");
        fs::write(new_file, "let y = 1;")?;

        // Test that uncommitted file is marked as changed with the parameters that Go
        // will pass
        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            repo_root.path().to_path_buf(),
            None,
        )?;
        assert_eq!(files, HashSet::from(["bar.js".to_string()]));

        Ok(())
    }

    // Tests that we can use a subdir as the monorepo_root path
    // (occurs when the monorepo is nested inside a subdirectory of git repository)
    #[test]
    fn test_changed_files_with_subdir_as_monorepo_root() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;

        fs::create_dir(repo_root.path().join("subdir"))?;
        // Create additional nested directory to test that we return a system path
        // and not a normalized unix path
        fs::create_dir(repo_root.path().join("subdir").join("src"))?;

        let file = repo_root.path().join("subdir").join("foo.js");
        fs::write(file, "let z = 0;")?;
        let first_commit = commit_file(&repo, Path::new("subdir/foo.js"), None)?;

        let new_file = repo_root.path().join("subdir").join("src").join("bar.js");
        fs::write(new_file, "let y = 1;")?;

        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            repo_root.path().join("subdir"),
            None,
        )?;

        #[cfg(unix)]
        {
            assert_eq!(files, HashSet::from(["src/bar.js".to_string()]));
        }

        #[cfg(windows)]
        {
            assert_eq!(files, HashSet::from(["src\\bar.js".to_string()]));
        }

        commit_file(&repo, Path::new("subdir/src/bar.js"), Some(first_commit))?;

        let files = super::changed_files(
            repo_root.path().to_path_buf(),
            repo_root.path().join("subdir"),
            Some((
                first_commit.to_string().as_str(),
                repo.head()?.peel_to_commit()?.id().to_string().as_str(),
            )),
        )?;

        #[cfg(unix)]
        {
            assert_eq!(files, HashSet::from(["src/bar.js".to_string()]));
        }

        #[cfg(windows)]
        {
            assert_eq!(files, HashSet::from(["src\\bar.js".to_string()]));
        }

        Ok(())
    }

    #[test]
    fn test_previous_content() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;

        let file = repo_root.path().join("foo.js");
        fs::write(&file, "let z = 0;")?;

        let first_commit_oid = commit_file(&repo, Path::new("foo.js"), None)?;
        fs::write(&file, "let z = 1;")?;
        let second_commit_oid = commit_file(&repo, Path::new("foo.js"), Some(first_commit_oid))?;

        let content = previous_content(
            repo_root.path().to_path_buf(),
            first_commit_oid.to_string().as_str(),
            file.clone(),
        )?;

        assert_eq!(content, b"let z = 0;");

        let content = previous_content(
            repo_root.path().to_path_buf(),
            second_commit_oid.to_string().as_str(),
            file,
        )?;
        assert_eq!(content, b"let z = 1;");

        set_current_dir(repo_root.path())?;

        // Check that relative paths work as well
        let content = previous_content(
            PathBuf::from("."),
            second_commit_oid.to_string().as_str(),
            PathBuf::from("./foo.js"),
        )?;
        assert_eq!(content, b"let z = 1;");

        let content = previous_content(
            repo_root.path().to_path_buf(),
            second_commit_oid.to_string().as_str(),
            PathBuf::from("./foo.js"),
        )?;
        assert_eq!(content, b"let z = 1;");

        Ok(())
    }

    #[test]
    fn test_revparse() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;

        let file = repo_root.path().join("foo.js");
        fs::write(&file, "let z = 0;")?;

        let first_commit_oid = commit_file(&repo, Path::new("foo.js"), None)?;
        fs::write(&file, "let z = 1;")?;
        let second_commit_oid = commit_file(&repo, Path::new("foo.js"), Some(first_commit_oid))?;

        let revparsed_head = repo.revparse_single("HEAD")?;
        assert_eq!(revparsed_head.id(), second_commit_oid);
        let revparsed_head_minus_1 = repo.revparse_single("HEAD~1")?;
        assert_eq!(revparsed_head_minus_1.id(), first_commit_oid);

        Ok(())
    }

    #[test]
    fn test_deleted_files() -> Result<(), Error> {
        let repo_root = tempfile::tempdir()?;
        let repo = Repository::init(repo_root.path())?;
        let mut config = repo.config()?;
        config.set_str("user.name", "test")?;
        config.set_str("user.email", "test@example.com")?;

        let file = repo_root.path().join("foo.js");
        let file_path = Path::new("foo.js");
        fs::write(&file, "let z = 0;")?;

        let first_commit_oid = commit_file(&repo, &file_path, None)?;

        fs::remove_file(&file)?;
        let _second_commit_oid = commit_delete(&repo, &file_path, first_commit_oid)?;

        let first_commit_sha = first_commit_oid.to_string();
        let git_repo_root = repo_root.path().to_owned();
        let turborepo_root = repo_root.path().to_owned();
        let files = changed_files(
            git_repo_root,
            turborepo_root,
            Some((&first_commit_sha, "HEAD")),
        )?;
        assert_eq!(files, HashSet::from(["foo.js".to_string()]));
        Ok(())
    }
}
