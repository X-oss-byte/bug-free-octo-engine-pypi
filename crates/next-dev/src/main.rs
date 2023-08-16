#![feature(future_join)]
#![feature(min_specialization)]

use anyhow::Result;
#[cfg(feature = "cli")]
use clap::Parser;

#[global_allocator]
static ALLOC: turbo_malloc::TurboMalloc = turbo_malloc::TurboMalloc;

#[cfg(not(feature = "cli"))]
fn main() -> Result<()> {
    unimplemented!("Cannot run binary without CLI feature enabled");
}

#[tokio::main]
#[cfg(feature = "cli")]
async fn main() -> Result<()> {
    let options = next_dev::devserver_options::DevServerOptions::parse();

    if options.display_version {
        // Note: enabling git causes trouble with aarch64 linux builds with libz-sys
        println!(
            "Build Timestamp\t\t{:#?}",
            option_env!("VERGEN_BUILD_TIMESTAMP").unwrap_or_else(|| "N/A")
        );
        println!(
            "Build Version\t\t{:#?}",
            option_env!("VERGEN_BUILD_SEMVER").unwrap_or_else(|| "N/A")
        );
        println!(
            "Cargo Target Triple\t{:#?}",
            option_env!("VERGEN_CARGO_TARGET_TRIPLE").unwrap_or_else(|| "N/A")
        );
        println!(
            "Cargo Profile\t\t{:#?}",
            option_env!("VERGEN_CARGO_PROFILE").unwrap_or_else(|| "N/A")
        );

        return Ok(());
    }

    next_dev::start_server(&options).await
}
