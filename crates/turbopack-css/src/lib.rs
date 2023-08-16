#![feature(min_specialization)]
#![feature(box_patterns)]
#![feature(iter_intersperse)]
#![feature(int_roundings)]
#![feature(arbitrary_self_types)]
#![feature(async_fn_in_trait)]

mod asset;
pub mod chunk;
mod code_gen;
pub mod embed;
mod module_asset;
pub(crate) mod parse;
mod path_visitor;
pub(crate) mod references;
pub(crate) mod transform;
pub(crate) mod util;

pub use asset::CssModuleAsset;
pub use module_asset::ModuleCssModuleAsset;
pub use transform::{CssInputTransform, CssInputTransforms};

use crate::references::import::ImportAssetReference;

#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(PartialOrd, Ord, Hash, Debug, Copy, Clone)]
pub enum CssModuleAssetType {
    Global,
    Module,
}

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    turbopack_ecmascript::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}
