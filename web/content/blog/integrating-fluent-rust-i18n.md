---
title: "Fluent i18n in Rust: internationalization for desktop apps"
description: "Using Mozilla Fluent for internationalization in Rust desktop apps: message format, plural rules, and locale handling with the es-fluent crate."
date: 2026-06-05
tags: [Rust, i18n, Fluent]
draft: false
---

I have worked on desktop apps where "internationalization" meant a JSON file full of key-value pairs and a helper function called `t("key")`. It got us through the first release. Then we added Russian, and the plural rules broke. Then Arabic, and gender agreement broke. Then we realized "You have 3 new messages" needs different sentence structures depending on who is reading it, and the JSON approach stopped working.

Mozilla's Fluent system exists because Firefox hit all of these problems at once. The `es-fluent` crate brings that system to Rust desktop apps. This post covers how it works, what the tradeoffs are, and why I prefer it over hand-rolling i18n with JSON.

## Why Fluent instead of JSON

A flat key-value map covers button labels and page titles. That accounts for maybe 60% of localized strings. The other 40% involves plurals, variable interpolation, grammatical gender, and context-dependent phrasing. These are not edge cases. They are normal features of most human languages.

English has two plural categories: "one" and "other." Russian has four: "one," "few," "many," and "other." Arabic has six. Chinese grammar does not mark plurality for most nouns. If you store `"items": "{} items"` in a JSON file, you have baked English grammar into your string template. A Chinese translator looking at that template has no way to express that their language does not use plural markers here.

Fluent solves this by making the `.ftl` file a small language rather than a data format. Each message can contain selectors that branch on plural category, grammatical gender, or any variable you pass in. The translator controls sentence structure. Your Rust code supplies data and gets back a finished string.

## Message format basics

A Fluent message has an identifier and a value. The simplest form looks like this:

```ftl
home_title = Welcome to GPUI Starter
home_subtitle = A boilerplate for building desktop apps with GPUI
```

That already works like a JSON key-value map. But Fluent messages can also carry attributes for different contexts:

```ftl
settings_language = Language
settings_language_english = English
settings_language_simplified_chinese = 简体中文
```

The difference shows up when you add variables and selectors.

## Plural rules with selectors

Here is an English plural message:

```ftl
items-count = { $count ->
    [one] {$count} item
   *[other] {$count} items
}
```

The asterisk marks the default variant. When `$count` is 1, Fluent picks `[one]`. For any other number it falls back to `*[other]`.

The same message in simplified Chinese needs only the default branch, because Chinese does not distinguish singular from plural the way English does:

```ftl
items-count = { $count ->
   *[other] {$count} 个项目
}
```

Each locale file declares only the plural categories its language uses. Add Polish later (which has "one," "few," "many," and "other") and you just add those branches in the Polish `.ftl` file. No Rust code changes.

Compare this with a JSON approach: you would invent a plural key naming convention (`items_one`, `items_other`, `items_many`), teach every translator about it, and write Rust code that looks up the correct key. Fluent handles the branching inside the `.ftl` file, and the translator owns the sentence structure.

## Setting up es-fluent in a Rust project

The `es-fluent` crate provides the runtime. `es-fluent-build` handles compile-time code generation from your `.ftl` files. `es-fluent-manager-embedded` bundles everything into the binary so you do not ship loose files.

First, define your locales in `Cargo.toml`:

```toml
[dependencies]
es-fluent = { version = "0.16", features = ["derive"] }
es-fluent-build = "0.16"
es-fluent-lang = "0.16"
es-fluent-manager-embedded = "0.16"
```

Then create your `.ftl` files under an `i18n/` directory:

```
i18n/
  en/
    app.ftl
  zh-CN/
    app.ftl
```

Define a languages enum. The `#[es_fluent_language]` macro generates the locale matching code:

```rust
use es_fluent::EsFluent;
use es_fluent_lang::es_fluent_language;
use strum::EnumIter;

#[es_fluent_language]
#[derive(Clone, Copy, Debug, EnumIter, EsFluent, PartialEq)]
pub enum Languages {}
```

That empty enum looks wrong at first glance. The `#[es_fluent_language]` macro populates it at compile time from the `.ftl` files it finds. You do not manually list language variants. The build step reads the directory structure and generates the variants for you. If you add a new locale directory and rebuild, the enum picks up the new variant without any code changes.

## Runtime initialization and locale switching

At startup, detect the system locale and initialize the i18n system:

```rust
use std::sync::OnceLock;
use es_fluent_manager_embedded::EmbeddedI18n;

static I18N: OnceLock<EmbeddedI18n> = OnceLock::new();

pub fn init_i18n(lang: es_fluent::unic_langid::LanguageIdentifier)
    -> Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let i18n = EmbeddedI18n::try_new_with_language(lang)?;
    let _ = I18N.set(i18n);
    Ok(())
}
```

The `OnceLock` ensures initialization happens once. `EmbeddedI18n` bundles all locale data into the binary at compile time, so there is no file I/O at runtime and no risk of missing translation files on the user's system.

To look up a localized string:

```rust
pub fn localize(id: &str, args: Option<&HashMap<&str, FluentValue>>) -> String {
    i18n().localize(id, args).unwrap_or_else(|| id.to_string())
}
```

The fallback to the raw `id` means missing translations never crash the app. You see the message key in the UI, which is not ideal but not fatal.

Switching locales at runtime requires selecting a new language on the embedded manager and refreshing the UI:

```rust
pub fn set_locale(locale: &str, cx: &mut App) {
    let _ = i18n().select_language(
        locale.parse().unwrap_or_else(|_| langid!("en")),
    );
    cx.refresh_windows();
}
```

The `refresh_windows` call tells GPUI to re-render. Every view picks up the new strings on the next frame with no manual state tracking for which components need updating. The locale change propagates through the render cycle.

## Compile-time validation

This is the feature that sold me on `es-fluent` over other Rust Fluent bindings. The build step parses your `.ftl` files and generates typed message structs. If you reference a message ID that does not exist, you get a compile error. If you pass the wrong number of arguments to a plural message, you get a compile error.

Contrast this with runtime string lookups, where a typo in a message ID silently falls back to showing the key name. Compile-time validation catches translation bugs before the app ships, not after a user files a bug about a weird label on their screen.

The `FluentMessage` trait ties into this system:

```rust
use es_fluent::FluentMessage;

pub fn localize_message<T: FluentMessage + ?Sized>(message: &T) -> String {
    i18n().localize_message(message)
}
```

Messages that satisfy `FluentMessage` are generated by the build macro. You pass them around as typed values rather than raw strings. This works well with derive macros on form structs, where each field label and description is type-checked against the `.ftl` files at compile time. See the [form validation guide](/blog/form-validation-koruma-rust/) for a concrete example.

## The tradeoffs

Fluent is not the right choice for every project.

The `.ftl` syntax has a learning curve. Translators used to editing JSON or YAML files need to learn selectors, variants, and the whitespace rules. For a project with three labels and one locale, that overhead is not justified. Use a `HashMap<&str, String>` and move on.

Compile-time code generation means longer build times. In a large project with many locales, the macro expansion adds a noticeable pause to `cargo build`. You catch translation errors at build time rather than in production. I consider that a good deal, but it is a real cost.

The `es-fluent` ecosystem is smaller than `rust-i18n` or `fluent-rs`. Fewer tutorials, fewer Stack Overflow answers. The documentation is adequate but short. I spent an afternoon reading the Fluent specification on `projectfluent.org` before the macro system clicked. If you need community support, `fluent-rs` (the official Mozilla Rust binding) has a larger user base, though it lacks compile-time validation.

## Practical tips from shipping this

Keep your `.ftl` files organized with section comments. Group messages by feature or page. A flat file with 200 unorganized messages becomes hard to maintain fast:

```ftl
## SettingsPage

settings_title = Settings
settings_dark_mode = Dark Mode
settings_language = Language

## FormPage

form_page_title = Create Account
form_submit = Create Account
form_reset = Reset
```

Always provide an English fallback. The `en/` directory is your source of truth. Other locales translate from it. If a translation is missing for a key that exists in English, Fluent falls back to the English value rather than showing the raw key. This requires setting the fallback language in your setup.

Detect the system locale on startup using `sys-locale`, but let the user override it. Some users run their OS in English but prefer a different language in specific apps. Save the user's choice in your config so it survives restarts.

## Closing thoughts

Fluent's core idea is that translation is a programming problem, not a data problem. Giving translators a small language to express grammatical rules produces better localized software than a flat map of keys ever will. The `es-fluent` crate adds compile-time safety on top, which is the kind of tradeoff Rust developers tend to appreciate.

If you want to see this setup working in a real app, the [gpui-starter](https://github.com/hmziqrs/gpui-boilerplate) project ships with Fluent i18n configured for English and simplified Chinese. The [getting started guide](/docs/getting-started/) walks through the full setup, and the [i18n documentation](/docs/i18n/) covers adding new locales and writing `.ftl` messages in detail. You can also read about [how GPUI compares to other Rust GUI frameworks](/blog/gpui-vs-iced-elm-architecture/) if you are evaluating options.
