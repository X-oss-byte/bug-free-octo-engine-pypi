pub use ::include_dir::{self, include_dir};
use anyhow::Result;
use turbo_tasks::TransientInstance;

use crate::{embed::EmbeddedFileSystemVc, DiskFileSystemVc, FileSystemVc};

#[turbo_tasks::function]
pub async fn directory_from_relative_path(name: &str, path: String) -> Result<FileSystemVc> {
    let disk_fs = DiskFileSystemVc::new(name.to_string(), path);
    disk_fs.await?.start_watching()?;

    Ok(disk_fs.into())
}

#[turbo_tasks::function]
pub async fn directory_from_include_dir(
    name: &str,
    dir: TransientInstance<&'static include_dir::Dir<'static>>,
) -> Result<FileSystemVc> {
    Ok(EmbeddedFileSystemVc::new(name.to_string(), dir).into())
}

/// Returns an embedded [FileSystemVc] for the given path.
///
/// This will embed a directory's content into the binary and
/// create an [EmbeddedFileSystemVc].
///
/// If you enable the `dynamic_embed_contents` feature, calling
/// the macro will return a [DiskFileSystemVc].
///
/// This enables dynamic linking (and hot reloading) of embedded files/dirs.
/// A binary built with `dynamic_embed_contents` enabled is **is not portable**,
/// only the directory path will be embedded into the binary.
#[macro_export]
macro_rules! embed_directory {
    ($name:tt, $path:tt) => {{        // make sure the path contains `$CARGO_MANIFEST_DIR`
        assert!($path.contains("$CARGO_MANIFEST_DIR"));
        // make sure `CARGO_MANIFEST_DIR` is the only env variable in the path
        assert!(!$path.replace("$CARGO_MANIFEST_DIR", "").contains('$'));

        turbo_tasks_fs::embed_directory_internal!($name, $path)
    }};
}

#[cfg(feature = "dynamic_embed_contents")]
#[macro_export]
#[doc(hidden)]
macro_rules! embed_directory_internal {
    ($name:tt, $path:tt) => {{
        // make sure the types the `include_dir!` proc macro refers to are in scope
        use turbo_tasks_fs::embed::include_dir;

        let path = $path.replace("$CARGO_MANIFEST_DIR", env!("CARGO_MANIFEST_DIR"));

        turbo_tasks_fs::embed::directory_from_relative_path($name, path)
    }};
}

#[cfg(not(feature = "dynamic_embed_contents"))]
#[macro_export]
#[doc(hidden)]
macro_rules! embed_directory_internal {
    ($name:tt, $path:tt) => {{
        // make sure the types the `include_dir!` proc macro refers to are in scope
        use turbo_tasks_fs::embed::include_dir;

        static dir: include_dir::Dir<'static> = turbo_tasks_fs::embed::include_dir!($path);

        turbo_tasks_fs::embed::directory_from_include_dir(
            $name,
            turbo_tasks::TransientInstance::new(&dir),
        )
    }};
}
