use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;

use super::RenderType;
use crate::{
    bundlers::Bundler,
    util::{
        npm::{self, NpmPackage},
        wait_for_match,
    },
};

pub struct Turbopack {
    name: String,
    path: String,
    render_type: RenderType,
}

impl Turbopack {
    pub fn new(name: &str, path: &str, render_type: RenderType) -> Self {
        Turbopack {
            name: name.to_owned(),
            path: path.to_owned(),
            render_type,
        }
    }
}

impl Bundler for Turbopack {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_path(&self) -> &str {
        &self.path
    }

    fn render_type(&self) -> RenderType {
        self.render_type
    }

    fn prepare(&self, install_dir: &Path) -> Result<()> {
        npm::install(
            install_dir,
            &[
                NpmPackage::new("next", "13.1.7-canary.12"),
                // Dependency on this is inserted by swc's preset_env
                NpmPackage::new("@swc/helpers", "^0.4.11"),
            ],
        )
        .context("failed to install from npm")?;

        fs::write(
            install_dir.join("next.config.js"),
            include_bytes!("next.config.js"),
        )?;

        Ok(())
    }

    fn start_server(&self, test_dir: &Path) -> Result<(Child, String)> {
        let mut proc = Command::new(
            std::env::var("CARGO_BIN_EXE_next-dev")
                .unwrap_or_else(|_| std::env!("CARGO_BIN_EXE_next-dev").to_string()),
        )
        .args([
            test_dir
                .to_str()
                .ok_or_else(|| anyhow!("failed to convert test directory path to string"))?,
            "--no-open",
            "--port",
            "0",
        ])
        .stdout(Stdio::piped())
        .spawn()?;

        // Wait for the devserver address to appear in stdout.
        let addr = wait_for_match(
            proc.stdout
                .as_mut()
                .ok_or_else(|| anyhow!("missing stdout"))?,
            Regex::new("started server on .+, url: (.*)")?,
        )
        .ok_or_else(|| anyhow!("failed to find devserver address"))?;

        Ok((proc, addr))
    }
}
