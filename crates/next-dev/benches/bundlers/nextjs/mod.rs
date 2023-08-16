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

#[derive(Debug)]
pub enum NextJsVersion {
    V11,
    V12,
    V13,
}

#[derive(Debug)]
pub struct NextJs {
    version: NextJsVersion,
    name: String,
    path: String,
    render_type: RenderType,
}

impl NextJs {
    pub fn new(version: NextJsVersion, name: &str, path: &str, render_type: RenderType) -> Self {
        Self {
            name: name.to_owned(),
            path: path.to_owned(),
            render_type,
            version,
        }
    }
}

impl Bundler for NextJs {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_path(&self) -> &str {
        &self.path
    }

    fn render_type(&self) -> RenderType {
        self.render_type
    }

    fn react_version(&self) -> &str {
        self.version.react_version()
    }

    fn prepare(&self, install_dir: &Path) -> Result<()> {
        npm::install(
            install_dir,
            &[NpmPackage::new("next", self.version.version())],
        )
        .context("failed to install `next` module")?;

        if matches!(self.version, NextJsVersion::V13) {
            fs::write(
                install_dir.join("next.config.js"),
                include_bytes!("next.config.js"),
            )?;
        }
        Ok(())
    }

    fn start_server(&self, test_dir: &Path) -> Result<(Child, String)> {
        // Using `node_modules/.bin/next` would sometimes error with `Error: Cannot find
        // module '../build/output/log'`
        let mut proc = Command::new("node")
            .args([
                test_dir
                    .join("node_modules")
                    .join("next")
                    .join("dist")
                    .join("bin")
                    .join("next")
                    .to_str()
                    .unwrap(),
                "dev",
                "--port",
                // Next.js currently has a bug where requests for port 0 are ignored and it falls
                // back to the default 3000. Use portpicker instead.
                &portpicker::pick_unused_port()
                    .ok_or_else(|| anyhow!("failed to pick unused port"))?
                    .to_string(),
            ])
            .current_dir(test_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("failed to run `next` command")?;

        let addr = wait_for_match(
            proc.stdout
                .as_mut()
                .ok_or_else(|| anyhow!("missing stdout"))?,
            Regex::new("started server.*url: (.*)")?,
        )
        .ok_or_else(|| anyhow!("failed to find devserver address"))?;

        Ok((proc, format!("{addr}/page")))
    }
}

impl NextJsVersion {
    /// Returns the version of Next.js to install from npm.
    pub fn version(&self) -> &'static str {
        match self {
            NextJsVersion::V11 => "^11",
            NextJsVersion::V12 => "^12",
            NextJsVersion::V13 => "^13",
        }
    }

    /// Returns the version of React to install from npm alongside this version
    /// of Next.js.
    pub fn react_version(&self) -> &'static str {
        match self {
            NextJsVersion::V11 => "^17.0.2",
            NextJsVersion::V12 => "^18.2.0",
            NextJsVersion::V13 => "^18.2.0",
        }
    }
}
