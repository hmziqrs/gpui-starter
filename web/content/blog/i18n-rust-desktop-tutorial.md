---
title: "Adding internationalization to a Rust desktop app with Fluent"
description: "Tutorial on adding multi-language support to a Rust desktop app using Mozilla Fluent message format. Covers .ftl files, plural selectors, and runtime switching with English and Chinese examples."
date: 2026-06-05
tags: [Rust, i18n, tutorial]
draft: false
---

I have shipped desktop apps that only spoke English. It was fine until it was not. Users in China, Japan, and Germany started filing issues asking for localized interfaces, and the "fix" was always the same awkward hack: a JSON file full of string keys and a helper function that looked them up at runtime. It worked for static labels. It fell apart the moment we needed plurals, variable interpolation, or any kind of grammatical agreement. If you have tried to handle Russian plural rules inside Rust code, you know the pain.

This tutorial shows how to add internationalization to a Rust desktop app using [Mozilla's Fluent](https://projectfluent.org/) message format. I will use English and simplified Chinese as the two working languages because they have very different grammatical structures, which makes the tradeoffs visible. The same approach works for any locale combination.

## Why Fluent and not JSON

A flat JSON file maps keys to strings. That covers maybe 70% of what a localized app needs. Button labels, page titles, menu items. The remaining 30% is where things get interesting.

Plural forms differ across languages. English has "one" and "other." Russian has four categories. Arabic has six. Chinese grammar does not mark plurality at all for most nouns. If you store `"items": "{} items"` in a JSON file, you have baked English grammar into your string. A Chinese translator seeing that template has no way to remove the plural "s" without asking you to change the code.

Fluent solves this by making the `.ftl` file a small language, not a data format. Each message can contain selectors that branch on plural category, grammatical gender, or any other variable you pass in. The translator controls the sentence structure. Your Rust code just supplies the data.

## Setting up the project structure

Create an `i18n/` directory at your project root with one subdirectory per locale:

```
i18n/
  en/
    messages.ftl
  zh-CN/
    messages.ftl
```

Each `.ftl` file holds all the translated strings for that locale. The English file is your source of truth. Other locales translate from it.

You also need a small config file, `i18n.toml`, at the project root:

```toml
assets_dir = "i18n"
fallback_language = "en"
```

This tells the build system where to find translation files and which locale to use when a message is missing.

## Writing Fluent messages

A Fluent message has an identifier, a value, and optional attributes. The simplest form is a plain string:

```ftl
home_title = Welcome to GPUI Starter
home_subtitle = A boilerplate for building desktop apps with GPUI
home_get_started = Get Started
```

The same file in simplified Chinese:

```ftl
home_title = 欢迎使用 GPUI Starter
home_subtitle = 使用 GPUI 构建桌面应用的模板
home_get_started = 开始使用
```

That covers static text. The real value shows up in the next section.

### Variables and interpolation

You can embed variables in a message using `{$variable_name}`:

```ftl
about_version = GPUI Starter v{ $version }
```

In Chinese:

```ftl
about_version = GPUI Starter v{ $version }
```

The version string is the same in both languages, so the translation is identical. But the point is that the translator decides where `$version` appears. In a language that reads right-to-left, the translator might place it differently without you touching any Rust code.

### Plural selectors

This is the main advantage of Fluent over JSON. English has two plural categories: "one" and "other." A selector looks like this:

```ftl
items-count = { $count ->
    [one] {$count} item
   *[other] {$count} items
}
```

The asterisk marks the default variant. When `$count` is 1, Fluent picks `[one]`. For any other number it falls back to `*[other]`.

Chinese does not distinguish singular from plural for most nouns. The translator just needs the default:

```ftl
items-count = { $count ->
   *[other] {$count} 个项目
}
```

No `[one]` branch. No special case. The Chinese file declares only what its grammar requires. If you later add Russian, which has "one," "few," "many," and "other," the translator adds all four branches to the Russian `.ftl` file. No Rust code changes.

### Mixing variables with selectors

A more realistic example that combines a user name with a count:

```ftl
unread-messages = { $count ->
    [one] {$user} has {$count} unread message
   *[other] {$user} has {$count} unread messages
}
```

Chinese version:

```ftl
unread-messages = { $user } 有 { $count } 条未读消息
```

The Chinese version does not even use a selector. The translator decided the sentence structure does not need one. Your Rust code passes the same `$count` and `$user` arguments regardless of locale. The `.ftl` file handles the rest.

### Attributes for context

Sometimes the same concept needs different text in different contexts. A page title in a navigation sidebar is shorter than the same title in a page header. Fluent attributes handle this:

```ftl
page-home = Home
page-form = Form
page-settings = Settings
page-about = About
```

Chinese:

```ftl
page-home = 首页
page-form = 表单
page-settings = 设置
page-about = 关于
```

If you need a short form for a sidebar and a longer form for a header, you add attributes:

```ftl
page-settings = Settings
    .short = Settings
    .description = Application settings and preferences
```

You access the value with `page-settings` and the attribute with `page-settings.description`.

## Calling Fluent from Rust

The `es-fluent` crate embeds your `.ftl` files at compile time and generates type-safe accessors. If a Fluent message has broken syntax, you get a compile error. You find out about a broken translation before the app runs.

The i18n service is a thin wrapper:

```rust
use std::{collections::HashMap, sync::OnceLock};

use es_fluent::{FluentLocalizer as _, FluentMessage, FluentValue};
use es_fluent_manager_embedded::EmbeddedI18n;

es_fluent_manager_embedded::define_i18n_module!();

static I18N: OnceLock<EmbeddedI18n> = OnceLock::new();

pub fn init_i18n(
    lang: es_fluent::unic_langid::LanguageIdentifier,
) -> Result<(), Box<dyn std::error::Error>> {
    let i18n = EmbeddedI18n::try_new_with_language(lang)?;
    let _ = I18N.set(i18n);
    Ok(())
}

pub fn i18n() -> &'static EmbeddedI18n {
    I18N.get_or_init(|| {
        EmbeddedI18n::try_new().expect("embedded i18n fallback must succeed")
    })
}

pub fn localize(
    id: &str,
    args: Option<&HashMap<&str, FluentValue<'_>>>,
) -> String {
    i18n().localize(id, args).unwrap_or_else(|| id.to_string())
}
```

The `define_i18n_module!()` macro scans your `.ftl` files at build time and produces the `EmbeddedI18n` type. The `OnceLock` ensures initialization happens once and the reference is valid for the rest of the process lifetime.

The fallback in `localize` is deliberate. If a message ID is missing, you get the ID string itself in the UI. That is much easier to debug than an empty label or a panic. You see "settings_test_native_notification" on screen and you know exactly which key to add.

Using it in a view component looks like this:

```rust
div()
    .child(crate::i18n::localize("home_title", None))
    .child(crate::i18n::localize("home_subtitle", None))
```

Pass `None` for static strings. Pass a `HashMap` with your variables for messages that use interpolation:

```rust
use std::collections::HashMap;
use es_fluent::FluentValue;

let mut args = HashMap::new();
args.insert("count", FluentValue::from(5));
args.insert("user", FluentValue::from("Alice"));

let text = crate::i18n::localize("unread-messages", Some(&args));
```

## Runtime language switching

Desktop apps should let users switch languages without restarting. The approach I use stores the current locale in a global state, updates the Fluent bundle, and triggers a re-render:

```rust
pub fn set_locale(locale: &str, cx: &mut App) {
    let _ = crate::i18n::i18n().select_language(
        locale.parse()
            .unwrap_or_else(|_| es_fluent::unic_langid::langid!("en")),
    );
    cx.set_global::<LocaleState>(
        LocaleState(SharedString::from(locale.to_string()))
    );
    // Persist the choice
    crate::app_state::update_config(cx, |config| {
        config.locale = locale.to_string();
    });
    cx.refresh_windows();
}
```

Three things happen here. `select_language` tells the i18n bundle to resolve messages from the new locale's `.ftl` file. The locale gets persisted to the app config so it survives restarts. Then `refresh_windows()` tells the framework to re-render every open window.

Because every view calls `localize` on each render pass, the re-render pulls fresh strings from the newly selected locale. The switch takes effect on the next frame. No cache to invalidate, no state to migrate. The system is pull-based: views ask for text when they draw, and the i18n layer returns whatever the current locale says.

On startup, detect the system locale and fall back to English if detection fails:

```rust
pub fn detect_system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en".to_string())
}
```

Check if the user has a persisted preference first. If not, use the system locale. This gives users a sensible default on first launch while respecting their explicit choice on subsequent launches.

## Detecting missing translations at build time

The biggest advantage of compile-time embedding is catching mistakes early. When you add a new message to the English `.ftl` file but forget to add it to the Chinese one, you have two safety nets.

First, if the `.ftl` syntax itself is broken, `cargo build` fails with a clear error pointing to the file and line. Second, the `es-fluent` fallback mechanism returns the message ID as a string at runtime, so missing translations show up as visible keys in the UI rather than silent gaps.

I recommend adding a CI step that parses all `.ftl` files and reports which message IDs are present in one locale but missing from another. A simple diff of message IDs catches most localization gaps before they reach users.

## Tradeoffs to know about

Fluent is not the right choice for every project. If your app has five screens and only needs English, a `const` string or a small enum is simpler and has zero overhead. Fluent adds a build step, compile-time embedding, and a runtime lookup cost.

The `.ftl` syntax is also something translators need to learn. It is not JSON, and tools like Crowdin or Transifex have varying levels of Fluent support. For small teams where the developer also writes translations, this is fine. For large localization agencies, you may need to invest in tooling or documentation.

On the other hand, if your app supports three or more languages, especially languages with complex plural rules or grammatical gender, Fluent saves you from writing and maintaining that logic in Rust. The translation file owns the grammar. Your code owns the data. That separation is worth the setup cost.

## Closing notes

You now have a working internationalization setup for a Rust desktop app. You saw how Fluent `.ftl` files handle static strings, variable interpolation, and plural selectors without baking grammar rules into your Rust code. You saw how to embed translations at compile time with `es-fluent`, switch languages at runtime, and detect missing translations before they ship.

The [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) project ships with this exact i18n setup out of the box, including English and simplified Chinese locales, a settings page for language switching, and compile-time validation. The [getting started guide](/docs/getting-started/) walks through the full setup in about ten minutes.
