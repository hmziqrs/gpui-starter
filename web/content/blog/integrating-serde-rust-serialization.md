---
title: "Data serialization in Rust desktop apps with serde"
description: "Using serde for data serialization in Rust desktop applications: JSON, TOML, and custom serialization patterns for config, persistence, and IPC."
date: 2026-06-05
tags: [Rust, serde, serialization]
draft: false
---

Every Rust desktop app serializes data. Config files, persisted user preferences, network requests, inter-process communication, crash reports. At some point you need to turn a Rust struct into bytes and back again. Serde is the crate that handles this. The Rust community standardised on it years ago.

This post covers the serialization patterns I use in GPUI desktop apps: JSON for persistence and IPC, TOML for configuration, and custom serializers for the awkward cases where neither fits cleanly. If you want to see how serialization fits into the rest of the app, the [architecture docs](/docs/architecture/) cover that.

## Why serde is the default

Serde splits serialization into two traits: `Serialize` and `Deserialize`. Data formats implement `Serializer` and `Deserializer`. Your types only implement the traits once, and they gain support for every format that exists. JSON, TOML, YAML, bincode, MessagePack, Postcard. Add the derive macros and you are done:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: String,
    pub language: String,
    pub window_bounds: WindowBounds,
    pub telemetry_enabled: bool,
    #[serde(default)]
    pub last_opened_project: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
```

The derive macros generate serialization code at compile time. No reflection, no runtime cost. The serialized output contains exactly the fields you defined, in the order you defined them, unless you ask for something different.

The `#[serde(default)]` attribute on `last_opened_project` means the deserializer produces `None` when the field is missing from the input data. This matters when you add fields to a struct after users already have config files on disk. Without it, deserialization fails on old files. With it, the new field gets a sensible default and the app keeps working. Always add `default` to new optional fields in types that get persisted.

## JSON for persistence and IPC

JSON is the default format for good reason. Every language can read it, every debugging tool can display it, and `serde_json` is fast. For a desktop app, JSON handles saving state to disk, talking to local services, and serializing crash report payloads.

```rust
use serde_json;
use std::fs;
use std::path::PathBuf;

fn save_config(config: &AppConfig, path: &PathBuf) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(config)?;
    fs::write(path, json)?;
    Ok(())
}

fn load_config(path: &PathBuf) -> anyhow::Result<AppConfig> {
    let json = fs::read_to_string(path)?;
    let config: AppConfig = serde_json::from_str(&json)?;
    Ok(config)
}
```

`to_string_pretty` adds indentation and newlines. Use it for files humans might open in a text editor. For network payloads or SQLite TEXT columns, `to_string` (no pretty printing) produces smaller output.

### Handling unknown fields

API responses and third-party file formats contain fields your structs do not define. By default, serde ignores unknown fields during deserialization. This is the right default. Do not change it unless you have a specific reason.

If you do need to catch unknown fields (for example, to warn users about unsupported config options), use `#[serde(deny_unknown_fields)]`:

```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictConfig {
    pub theme: String,
}
```

I use this sparingly. Forward compatibility (old code reading new data) matters more than strictness in most desktop app scenarios.

## TOML for configuration

TOML reads better than JSON for config files. Comments, multiline strings, and a cleaner syntax for flat key-value pairs. The `toml` crate integrates with serde's derives, so the same struct works for both formats:

```rust
use std::fs;
use std::path::PathBuf;

fn load_toml_config(path: &PathBuf) -> anyhow::Result<AppConfig> {
    let toml_str = fs::read_to_string(path)?;
    let config: AppConfig = toml::from_str(&toml_str)?;
    Ok(config)
}
```

The config file on disk looks like this:

```toml
theme = "catppuccin-mocha"
language = "en"
telemetry_enabled = false

[window_bounds]
x = 100.0
y = 100.0
width = 1200.0
height = 800.0
```

The user can edit this file in any text editor, add comments, and understand what each field does without knowing JSON syntax. That matters for developer tools where users inspect and modify config files directly.

One caveat: TOML does not support arbitrary nesting depth well. If your config structure goes three or four levels deep, flatten it with `#[serde(flatten)]` or restructure the types. TOML tables get verbose fast.

## Custom serialization for enums

Enums are where serde's default behavior stops being obvious. Consider a theme name type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    CatppuccinLatte,
    CatppuccinMocha,
    GruvboxDark,
    Nord,
    OneDark,
    TokyoNight,
}
```

With `rename_all = "kebab-case"`, the serialized value becomes `"catppuccin-latte"` instead of `"CatppuccinLatte"`. This matches what users type in config files and what the theme selector displays.

For externally tagged enums (the default), the JSON output wraps the variant name:

```json
{"CatppuccinLatte": null}
```

That is awkward for config files. You probably want the variant name as a plain string. The `#[serde(tag = "type")]` or internally tagged representation works better here, or you can write a custom serializer:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeName {
    Named(String),
}
```

The `untagged` attribute serializes the inner value directly, without the wrapper. For simple string enums, this produces `"catppuccin-mocha"` as plain text, which is what config files expect.

### Custom serialize and deserialize functions

When the default derives cannot express the transformation you need, write custom functions:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    pub id: String,
    pub timestamp: String,
    pub message: String,
    #[serde(
        serialize_with = "serialize_actions",
        deserialize_with = "deserialize_actions"
    )]
    pub actions: Vec<ErrorAction>,
}

fn serialize_actions<S>(actions: &Vec<ErrorAction>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&actions.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(","))
}

fn deserialize_actions<'de, D>(deserializer: D) -> Result<Vec<ErrorAction>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.split(',')
        .filter(|s| !s.is_empty())
        .map(|s| ErrorAction::from_str(s).map_err(serde::de::Error::custom))
        .collect()
}
```

This pattern stores a list of actions as a comma-separated string. Not the most elegant storage format, but it works for SQLite TEXT columns and log files where you want a single readable line per entry.

## The trait bound problem

Serde's derives add `Serialize` and `Deserialize` bounds to every generic parameter in your struct. This infects your type signatures quickly:

```rust
#[derive(Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub value: T,
    pub expires_at: u64,
}
```

This struct requires `T: Serialize + DeserializeOwned` anywhere you serialize or deserialize it. For concrete types this is invisible. For generic code, the bounds propagate up through every function that touches `CacheEntry<T>`.

The fix: keep serialization types separate from domain types. Define a `CacheEntryData` struct with concrete fields for serialization, and convert between it and the domain type with `From`/`Into` implementations. The extra struct costs nothing at runtime and keeps the generic bounds out of your business logic.

## Binary formats for IPC

JSON works fine for most desktop app needs. When you need smaller payloads or faster deserialization (for example, communicating between the main app process and a crash reporter subprocess), bincode is worth considering:

```rust
use bincode;

fn encode_crash_report(report: &CrashReport) -> anyhow::Result<Vec<u8>> {
    let bytes = bincode::encode_to_vec(report, bincode::config::standard())?;
    Ok(bytes)
}

fn decode_crash_report(bytes: &[u8]) -> anyhow::Result<CrashReport> {
    let (report, _len): (CrashReport, usize) = bincode::decode_from_slice(bytes, bincode::config::standard())?;
    Ok(report)
}
```

Bincode produces compact binary output with no field names or metadata. It is fast to serialize and deserialize because it writes bytes directly with no string parsing. The downside: the output is opaque. You cannot open it in a text editor or pipe it through `jq`. Use binary formats for machine-to-machine communication within your app, not for files the user might inspect.

## Error handling patterns

Serialization errors happen at runtime. The file is corrupted, a field has the wrong type, or the format changed between versions. Handle these without crashing:

```rust
fn load_config_safe(path: &PathBuf) -> AppConfig {
    match load_config(path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to load config from {}: {e}", path.display());
            eprintln!("Falling back to defaults");
            AppConfig::default()
        }
    }
}
```

For a desktop app, a corrupt config file should not prevent the app from starting. Log the error, fall back to defaults, and let the user keep working. You can write the defaults back to disk on the next save, which repairs the file.

The [error handling patterns](/blog/testing-gpui-applications/) we use in GPUI testing also apply here: match on the error kind, log useful context, and recover rather than crashing.

## What we skipped

This post does not cover serde's attributes exhaustively (there are dozens), YAML serialization (use TOML instead for config files), or Protocol Buffers / FlatBuffers (relevant for high-throughput IPC but overkill for most desktop apps). The [serde documentation](https://serde.rs/attributes.html) lists every attribute with examples.

For a working implementation of everything described here, check out the [gpui-starter repository](https://github.com/hmziqrs/gpui-boilerplate). The config module uses TOML with `serde(default)` for forward compatibility, the storage layer serializes structured data to JSON for SQLite columns, and the crash reporter uses serde to write machine-readable report files. The [getting started guide](/docs/getting-started/) walks through cloning and running the project.
