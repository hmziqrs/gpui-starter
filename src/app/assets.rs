//! Combined asset source.
//!
//! Merges the bundled gpui-component icon assets (`gpui_component_assets::Assets`)
//! with gpui-starter's own project assets (currently the shipped theme
//! definitions under `themes/`). Project assets take precedence on lookup so
//! an app-supplied file shadows a same-named component asset. This replaces
//! the bare `with_assets(Assets)` call in `main.rs` so the binary carries
//! both sets of resources.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Project-local embedded assets (theme JSON shipped under `themes/`).
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/themes"]
struct ProjectAssets;

/// A merged [`AssetSource`] combining gpui-component assets with project assets.
pub struct CombinedAssets {
    component: gpui_component_assets::Assets,
}

impl CombinedAssets {
    pub fn new() -> Self {
        Self {
            component: gpui_component_assets::Assets,
        }
    }
}

impl Default for CombinedAssets {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetSource for CombinedAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        // Project assets take precedence.
        if let Some(file) = ProjectAssets::get(path) {
            return Ok(Some(file.data));
        }
        // Fall back to the bundled component assets (icons, etc.).
        self.component.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut entries: Vec<SharedString> = ProjectAssets::iter()
            .filter(|p| p.starts_with(path))
            .map(Into::into)
            .collect();
        let mut component_entries = self.component.list(path)?;
        entries.append(&mut component_entries);
        Ok(entries)
    }
}
