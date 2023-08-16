use std::path::PathBuf;

pub fn globwalk<'a>(
    root: PathBuf,
    files_only: bool,
    include_patterns: &'a [String],
    exclude_patterns: &'a [String],
) -> impl Iterator<Item = String> + 'a {
    let files = walkdir::WalkDir::new(root);
    files.into_iter().filter_map(move |f| {
        let f = match f.ok() {
            Some(f) => f,
            None => return None,
        };
        let str = f.path().to_string_lossy().to_string();

        if files_only && f.file_type().is_dir() {
            return None;
        }

        for pattern in exclude_patterns {
            if glob_match::glob_match(pattern, &str) {
                return None;
            }
        }

        if include_patterns
            .iter()
            .any(|pattern| glob_match::glob_match(pattern, &str))
        {
            Some(str)
        } else {
            None
        }
    })
}
