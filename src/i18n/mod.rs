//! Localised UI strings and runtime language selection.
//! 应用名称相关文案为 velora。
//!
//! This module owns language packs, system-locale matching, and the global
//! manager used by menus and editor UI. Visual styling remains in `theme`.

use std::path::Path;
pub(super) use std::sync::Arc;

pub(super) use anyhow::{Context as _, bail};
pub(super) use gpui::{App, Global};
pub(super) use serde::{Deserialize, Deserializer, Serialize};
pub(super) use serde_json::{Map, Value};

pub(super) use crate::config::{
    VeloraConfigDirs, object_without_empty_values, prune_empty_json_values, read_json_or_jsonc,
    sanitize_config_file_stem,
};


pub use manager::*;
pub use pack::*;
pub use strings::*;

mod de;
mod de_impl;
mod keys;
mod manager;
mod pack;
mod strings;
mod strings_api;

#[cfg(test)]
mod tests;
