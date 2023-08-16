use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
};

use nom::Finish;
use turbopath::{AbsoluteSystemPathBuf, RelativeUnixPathBuf};

use crate::{package_deps::GitHashes, read_git_error, wait_for_success, Error};

pub fn git_ls_tree(root_path: &AbsoluteSystemPathBuf) -> Result<GitHashes, Error> {
    let mut hashes = GitHashes::new();
    let mut git = Command::new("git")
        .args(["ls-tree", "-r", "-z", "HEAD"])
        .current_dir(root_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = git
        .stdout
        .as_mut()
        .ok_or_else(|| Error::git_error("failed to get stdout for git ls-tree"))?;
    let mut stderr = git
        .stderr
        .take()
        .ok_or_else(|| Error::git_error("failed to get stderr for git ls-tree"))?;
    let result = read_ls_tree(stdout, &mut hashes);
    if let Err(err) = result {
        return Err(read_git_error(&mut stderr).unwrap_or(err));
    }
    wait_for_success(git, &mut stderr, "git ls-tree", root_path)?;
    Ok(hashes)
}

fn read_ls_tree<R: Read>(reader: R, hashes: &mut GitHashes) -> Result<(), Error> {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        {
            let bytes_read = reader.read_until(b'\0', &mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            {
                let entry = parse_ls_tree(&buffer)?;
                let hash = String::from_utf8(entry.hash.to_vec())?;
                let path = RelativeUnixPathBuf::new(entry.filename)?;
                hashes.insert(path, hash);
            }
        }
    }
    Ok(())
}

struct LsTreeEntry<'a> {
    filename: &'a [u8],
    hash: &'a [u8],
}

fn parse_ls_tree(i: &[u8]) -> Result<LsTreeEntry<'_>, Error> {
    match nom_parse_ls_tree(i).finish() {
        Ok((_, entry)) => Ok(entry),
        Err(e) => Err(Error::git_error(format!(
            "failed to parse git-ls-tree: {}",
            String::from_utf8_lossy(e.input)
        ))),
    }
}

fn nom_parse_ls_tree(i: &[u8]) -> nom::IResult<&[u8], LsTreeEntry<'_>> {
    let (i, _) = nom::bytes::complete::is_not(" ")(i)?;
    let (i, _) = nom::character::complete::space1(i)?;
    let (i, _) = nom::bytes::complete::is_not(" ")(i)?;
    let (i, _) = nom::character::complete::space1(i)?;
    let (i, hash) = nom::bytes::complete::take(40usize)(i)?;
    let (i, _) = nom::bytes::complete::take(1usize)(i)?;
    let (i, filename) = nom::bytes::complete::is_not(" \0")(i)?;
    Ok((i, LsTreeEntry { filename, hash }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use turbopath::RelativeUnixPathBuf;

    use crate::{ls_tree::read_ls_tree, package_deps::GitHashes};

    fn to_hash_map(pairs: &[(&str, &str)]) -> GitHashes {
        HashMap::from_iter(pairs.into_iter().map(|(path, hash)| {
            (
                RelativeUnixPathBuf::new(path.as_bytes()).unwrap(),
                hash.to_string(),
            )
        }))
    }

    #[test]
    fn test_ls_tree() {
        let tests: &[(&str, &[(&str, &str)])] = &[
            (
                "100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\tpackage.json\0",
                &[("package.json", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")],
            ),
            (
                // missing nul byte
                "100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\tpackage.json",
                &[("package.json", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")],
            ),
            (
                "100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\t\t\000100644 blob \
                 e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\t\"\000100644 blob \
                 5b999efa470b056e329b4c23a73904e0794bdc2f\t\n\000100644 blob \
                 f44f57fff95196c5f7139dfa0b96875f1e9650a9\t.gitignore\000100644 blob \
                 33dbaf21275ca2a5f460249d941cbc27d5da3121\tREADME.md\000040000 tree \
                 7360f2d292aec95907cebdcbb412a6bf2bd10f8a\tapps\000100644 blob \
                 9ec2879b24ce2c817296eebe2cb3846f8e4751ea\tpackage.json\000040000 tree \
                 5759aadaea2cde55468a61e7104eb0a9d86c1d30\tpackages\000100644 blob \
                 33d0621ee2f4da4a2f6f6bdd51a42618d181e337\tturbo.json\0",
                &[
                    ("\t", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
                    ("\"", "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"),
                    ("\n", "5b999efa470b056e329b4c23a73904e0794bdc2f"),
                    (".gitignore", "f44f57fff95196c5f7139dfa0b96875f1e9650a9"),
                    ("README.md", "33dbaf21275ca2a5f460249d941cbc27d5da3121"),
                    ("apps", "7360f2d292aec95907cebdcbb412a6bf2bd10f8a"),
                    ("package.json", "9ec2879b24ce2c817296eebe2cb3846f8e4751ea"),
                    ("packages", "5759aadaea2cde55468a61e7104eb0a9d86c1d30"),
                    ("turbo.json", "33d0621ee2f4da4a2f6f6bdd51a42618d181e337"),
                ],
            ),
        ];
        for (input, expected) in tests {
            let input_bytes = input.as_bytes();
            let mut hashes = GitHashes::new();
            let expected = to_hash_map(expected);
            read_ls_tree(input_bytes, &mut hashes).unwrap();
            assert_eq!(hashes, expected);
        }
    }
}
