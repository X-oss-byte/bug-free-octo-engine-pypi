use std::path::PathBuf;

use anyhow::Result;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::{
    client::APIClient,
    config::{
        default_user_config_path, get_repo_config_path, RepoConfig, RepoConfigLoader, UserConfig,
        UserConfigLoader,
    },
    ui::UI,
    Args,
};

pub(crate) mod bin;
pub(crate) mod daemon;
pub(crate) mod link;
pub(crate) mod login;
pub(crate) mod logout;

pub struct CommandBase {
    pub repo_root: PathBuf,
    pub ui: UI,
    user_config: OnceCell<UserConfig>,
    repo_config: OnceCell<RepoConfig>,
    args: Args,
}

impl CommandBase {
    pub fn new(args: Args, repo_root: PathBuf) -> Result<Self> {
        Ok(Self {
            repo_root,
            ui: args.ui(),
            args,
            repo_config: OnceCell::new(),
            user_config: OnceCell::new(),
        })
    }

    fn create_repo_config(&self) -> Result<()> {
        let repo_config_path = get_repo_config_path(&self.repo_root);

        let repo_config = RepoConfigLoader::new(repo_config_path)
            .with_api(self.args.api.clone())
            .with_login(self.args.login.clone())
            .with_team_slug(self.args.team.clone())
            .load()?;

        self.repo_config.set(repo_config)?;

        Ok(())
    }

    fn create_user_config(&self) -> Result<()> {
        let user_config = UserConfigLoader::new(default_user_config_path()?)
            .with_token(self.args.token.clone())
            .load()?;
        self.user_config.set(user_config)?;

        Ok(())
    }

    pub fn repo_config_mut(&mut self) -> Result<&mut RepoConfig> {
        if self.repo_config.get().is_none() {
            self.create_repo_config()?;
        }

        Ok(self.repo_config.get_mut().unwrap())
    }

    pub fn repo_config(&self) -> Result<&RepoConfig> {
        if self.repo_config.get().is_none() {
            self.create_repo_config()?;
        }

        Ok(self.repo_config.get().unwrap())
    }

    pub fn user_config_mut(&mut self) -> Result<&mut UserConfig> {
        if self.user_config.get().is_none() {
            self.create_user_config()?;
        }

        Ok(self.user_config.get_mut().unwrap())
    }

    pub fn user_config(&self) -> Result<&UserConfig> {
        if self.user_config.get().is_none() {
            self.create_user_config()?;
        }

        Ok(self.user_config.get().unwrap())
    }

    pub fn api_client(&mut self) -> Result<APIClient> {
        let repo_config = self.repo_config()?;
        let api_url = repo_config.api_url();
        APIClient::new(api_url)
    }

    pub fn daemon_file_root(&self) -> PathBuf {
        std::env::temp_dir().join("turbod").join(self.repo_hash())
    }

    fn repo_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.repo_root.to_str().unwrap().as_bytes());
        hex::encode(&hasher.finalize()[..8])
    }
}

#[cfg(test)]
mod test {
    use test_case::test_case;

    #[test_case("/tmp/turborepo", "6e0cfa616f75a61c"; "basic example")]
    #[test_case("", "e3b0c44298fc1c14"; "empty string ok")]
    fn test_repo_hash(path: &str, expected_hash: &str) {
        use std::path::PathBuf;

        use super::CommandBase;
        use crate::Args;

        let args = Args::default();
        let repo_root = PathBuf::from(path);
        let command_base = CommandBase::new(args, repo_root).unwrap();

        let hash = command_base.repo_hash();

        assert_eq!(hash, expected_hash);
        assert_eq!(hash.len(), 16);
    }
}
