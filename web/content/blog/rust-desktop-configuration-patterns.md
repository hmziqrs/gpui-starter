---
title: "Configuration patterns for Rust desktop applications"
description: "How to handle configuration in Rust desktop apps: file formats, defaults, migrations, and user preferences with serde, TOML, and layered config."
date: 2026-06-05
tags: [Rust, configuration, patterns]
draft: false
---

Configuration is one of those things you build on day two and live with for the life of the project. Pick the wrong format and every config change becomes a chore. Skip defaults and your app crashes on first launch. Ignore migrations and users wake up to broken settings after an update. I have made all three mistakes, so here is what I do now.

## Where to put the file

Desktop apps have standard locations for configuration. On macOS it is `~/Library/Application Support/`. On Linux it follows XDG: `~/.config/`. On Windows it is `%APPDATA%`. You should not hardcode any of these.

The `directories` crate resolves the right path per platform:

```rust
use directories::ProjectDirs;

let proj = ProjectDirs::from("com", "MyCompany", "MyApp")
    .expect("Could not determine config directory");

let config_dir = proj.config_dir();
// macOS: ~/Library/Application Support/com.MyCompany.MyApp
// Linux: ~/.config/myapp
// Windows: C:\Users\Alice\AppData\Roaming\MyCompany\MyApp
```

One call and you get the right directory on every platform. I create the directory if it does not exist, then read or write the config file inside it:

```rust
use std::fs;

fn ensure_config_dir(proj: &ProjectDirs) -> &Path {
    let dir = proj.config_dir();
    if !dir.exists() {
        fs::create_dir_all(dir).expect("Failed to create config directory");
    }
    dir
}
```

This is the same pattern I use in [gpui-starter's architecture](/docs/architecture/) for resolving the settings path at startup.

## Choosing a format

You have three realistic options for config files in a Rust desktop app: TOML, JSON, and RON.

**TOML** is my default choice. It is readable, supports comments, and handles nested structures well. Rust developers know it from `Cargo.toml`. The `toml` crate integrates with serde directly.

**JSON** works if you expect other tools or scripts to read the config. It is universal. The downside is no comments, which users find frustrating in config files.

**RON** (Rusty Object Notation) is the best fit when your config closely mirrors Rust types. It supports enums with variants, tuples, and other Rust-specific structures that TOML handles awkwardly. The tradeoff is that non-Rust tools cannot read it.

For most desktop apps, TOML hits the sweet spot. Here is a typical config struct:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub window: WindowConfig,
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub features: FeatureConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppearanceConfig {
    pub theme: String,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
}

fn default_font_size() -> u32 {
    14
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FeatureConfig {
    #[serde(default)]
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub auto_update: bool,
}
```

Notice the `#[serde(default)]` annotations. These matter for forward compatibility, and I will explain why in the next section.

## Defaults are not optional

Every field in your config must have a default. For a desktop app there is no way around this. The user might delete the config file. You might ship a new version with new fields. If deserialization fails on a missing key, the app crashes or shows nothing.

There are three ways to set defaults with serde.

**Field-level defaults** using `#[serde(default)]` or `#[serde(default = "fn_name")]`:

```rust
#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub wal_mode: bool,
}

fn default_port() -> u16 { 5432 }
```

**Struct-level defaults** using `#[serde(default)]` on the whole struct. This requires a `Default` impl:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct FeatureConfig {
    #[serde(default)]
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub auto_update: bool,
}
```

Both approaches work. I use struct-level `Default` when every field has a sensible zero value (booleans, empty strings, zero numbers). I use field-level functions when defaults need actual values like `14` for font size or `"dracula"` for theme.

You can also implement defaults on the full config struct and use that as the base layer, which is what the next pattern does.

## Layered configuration

The most reliable config systems layer multiple sources. The idea: start with hardcoded defaults, overlay a config file, then optionally apply environment variable overrides. Each layer only needs to specify what it changes.

The `config` crate handles this out of the box:

```rust
use config::{Config, File, Environment};

fn load_config() -> Result<AppConfig, config::ConfigError> {
    let run_mode = std::env::var("RUN_MODE")
        .unwrap_or_else(|_| "production".into());

    let settings = Config::builder()
        // Layer 1: hardcoded defaults
        .set_default("window.width", 1200)?
        .set_default("window.height", 800)?
        .set_default("appearance.theme", "dark")?
        .set_default("appearance.font_size", 14)?
        .set_default("features.telemetry_enabled", false)?
        .set_default("features.auto_update", true)?
        // Layer 2: config file (optional, user may not have one)
        .add_source(
            File::with_name("config/default")
                .required(false)
        )
        // Layer 3: environment-specific overrides
        .add_source(
            File::with_name(&format!("config/{}", run_mode))
                .required(false)
        )
        // Layer 4: environment variables (highest priority)
        .add_source(
            Environment::with_prefix("MYAPP")
                .separator("_")
                .try_parsing(true)
        )
        .build()?;

    settings.try_deserialize()
}
```

Later sources override earlier ones. If the config file sets `window.width` to 1600 but the user also has `MYAPP_WINDOW_WIDTH=1000` in their environment, the env var wins. This is the same model that [12-factor apps](https://12factor.net/config) use, and it works just as well for desktop apps.

I use this layered approach in my own projects. The defaults live in code. The config file lives in the platform-specific directory from `ProjectDirs`. Environment variables are rarely needed for desktop apps, but they are useful for testing and CI.

## Config migrations

Here is a scenario that will happen: you ship version 1.0 with a config that has `theme: "dark"`. In version 1.1 you rename it to `appearance.theme` and add `appearance.accent_color`. Users who upgrade will have a config file that no longer matches your structs.

There are two strategies for this.

**Strategy 1:宽容的反序列化** (lenient deserialization). Use `#[serde(default)]` on every field and accept that old keys get silently dropped. This works if you do not care about preserving unknown keys:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)] // opt in to strict mode, or leave this off
pub struct AppConfig {
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub features: FeatureConfig,
}
```

With `deny_unknown_fields` removed, old keys like `theme: "dark"` at the top level are ignored. New structures get their defaults. The user loses the old setting, but the app does not crash.

**Strategy 2: explicit migration.** Keep a version field in the config and run migration functions:

```rust
#[derive(Debug, Deserialize)]
pub struct VersionedConfig {
    pub version: u32,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

fn migrate(raw: serde_json::Value) -> serde_json::Value {
    let version = raw.get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    match version {
        0 => migrate_v0_to_v1(raw),
        1 => migrate_v1_to_v2(raw),
        _ => raw,
    }
}
```

I prefer strategy 2 for any app that users will run for more than a few months. It is more code upfront, but it means you can rename fields, restructure sections, and change types without losing user preferences.

Users spend time configuring your app the way they like. Trashing their settings on upgrade is one of the fastest ways to lose trust. I cover this in the context of [state persistence in Rust desktop apps](/blog/rust-desktop-state-management/) as well.

## Saving config atomically

Writing config files has a failure mode that catches people off guard: if the app crashes mid-write, you get a partial file and the user loses all their settings. The fix is to write to a temporary file and atomically rename it:

```rust
use std::io::Write;
use std::path::Path;

fn save_config(config: &AppConfig, path: &Path) -> std::io::Result<()> {
    let tmp_path = path.with_extension("tmp");

    let mut file = fs::File::create(&tmp_path)?;
    let contents = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?; // ensure data reaches disk

    fs::rename(&tmp_path, path) // atomic on same filesystem
}
```

`fs::rename` is atomic on POSIX systems when source and destination are on the same filesystem. On Windows it is nearly atomic for small files. This pattern, write-to-temp-then-rename, is how databases handle their write-ahead logs. It works.

## Reactive config updates

Desktop apps often need to react to config changes without restarting. Maybe the user toggles a theme or changes font size in a settings panel. The simplest approach is to reload the full config from disk when the user saves settings:

```rust
fn reload_config(cx: &mut AppContext) -> Result<AppConfig, Box<dyn std::error::Error>> {
    let proj = ProjectDirs::from("com", "MyCompany", "MyApp")
        .ok_or("Could not resolve config directory")?;
    let config_path = proj.config_dir().join("config.toml");

    if config_path.exists() {
        let contents = fs::read_to_string(&config_path)?;
        let config: AppConfig = toml::from_str(&contents)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}
```

For hot-reload during development, you can watch the config file with `notify-rs` and reload on change. I do not recommend this for production. File watchers are finicky across platforms, and users should not be editing config files by hand anyway. If your settings UI writes the file, your settings UI should update the in-memory state at the same time.

## Common mistakes

**Putting secrets in config files.** API keys, tokens, and passwords should go through the OS keychain, not a TOML file. I wrote about this in the [secure credential storage](/blog/secure-credential-storage/) post. Use the `keyring` crate on macOS (it wraps the Security framework) or `secret-service` on Linux.

**Not handling the "first launch" case.** If `config.toml` does not exist, that is not an error. It means this is a fresh install. Return defaults and write them out so the user has a file to edit.

**Making config too granular.** If you expose every internal constant as a config option, you will never be able to change your internals without breaking configs. Config should represent user-facing preferences, not implementation details. The boundary is judgment, but a good rule of thumb: if changing a value requires code changes to make sense, it should not be in the config file.

## Putting it together

The pattern I use in production is straightforward:

1. Define config structs with `Serialize` and `Deserialize`.
2. Put `#[serde(default)]` on every field.
3. Resolve the config path with `ProjectDirs`.
4. Load from file, falling back to `Default` on missing or malformed files.
5. Save atomically with write-to-temp-then-rename.
6. Include a `version` field and migrate when it changes.

This handles first launch, upgrades, corrupted files, and cross-platform paths. It is boring code, which is the point. Configuration should be the least interesting part of your app.

If you want to see these patterns in a working codebase, check out [gpui-starter](https://github.com/freeoxide/gpui-starter). It is a production-ready GPUI boilerplate with TOML-based configuration, theme hot-reload, and platform-aware paths. The [getting started guide](/docs/getting-started/) walks through the full setup including config initialization.
