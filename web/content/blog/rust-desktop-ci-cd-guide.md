---
title: "CI/CD for Rust desktop applications"
description: "Setting up continuous integration and deployment for Rust desktop apps: GitHub Actions, testing, and release automation."
date: 2026-06-05
tags: [Rust, CI/CD, guide]
draft: false
---

I pushed a tag, went to get coffee, and came back to a GitHub Release page with a signed DMG, a notarized macOS bundle, and an auto-update manifest published to a gh-pages branch. That is what a working CI/CD pipeline for a Rust desktop app looks like. Getting there took weeks of trial and error, most of it around Apple's code signing requirements and cross-platform build matrix configuration.

This post covers the pipeline I settled on. It runs on GitHub Actions, handles macOS builds with signing and notarization, and publishes releases with Ed25519-signed update manifests. The same patterns apply to Windows and Linux builds; I will note where the platform-specific bits differ.

## What the pipeline needs to do

A desktop app CI/CD pipeline has four jobs: check that the code compiles and passes tests, build a release binary, package it into an installable format, and publish it somewhere users can download. For auto-updating apps, publishing also means generating a manifest that the app can poll.

Web apps deploy to a server you control. Desktop apps ship to users' machines. That difference changes everything. You need code signing so the OS does not scare users with "unidentified developer" dialogs. You need notarization for macOS Gatekeeper. You need deterministic builds so the binary you tested is the binary you ship. And you need a way to tell installed apps that a new version exists.

## The CI stage: check, test, clippy, fmt

The CI stage runs on every push and pull request. Its job is to fail fast. Four parallel jobs, each doing one thing.

```yaml
jobs:
  check:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
      - run: cargo check

  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
      - run: cargo test

  clippy:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: clippy
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo
            target/
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
      - run: cargo clippy -- -D warnings

  fmt:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: rustfmt
      - run: cargo fmt --check
```

A few things to notice. The cache key includes the Cargo.lock hash, so a dependency change invalidates the cache. The `clippy` job runs with `-D warnings`, which means any clippy warning fails the build. This is aggressive, but it keeps the codebase clean. You can relax this during initial development and tighten it later.

I run these on `macos-latest` because GPUI requires Metal, which means macOS runners. If your framework supports Linux or Windows, use a matrix strategy to run the check stage on all target platforms. Rust cross-compilation from macOS to Linux is possible with `cross` or `cargo-zigbuild`, but I have found that native runners produce fewer surprises.

For testing strategy specifics beyond what CI runs, I wrote about this in a separate post on [testing strategies for Rust desktop apps](/blog/rust-desktop-testing-strategies/).

## Caching: the difference between 3-minute and 25-minute builds

Rust compilation is slow. A fresh `cargo build --release` for a mid-size GPUI app takes 15 to 25 minutes on GitHub Actions runners. With proper caching, incremental builds drop to 2 to 4 minutes. That is the difference between pushing a fix and waiting half your lunch break.

The cache configuration above handles this, but there is a subtlety. CI caches have a 10 GB limit per repository. A full `target/` directory for a release build can be 2 to 3 GB. If you have multiple branches or workflow runs in parallel, you will hit that limit. When you do, GitHub evicts the oldest caches.

One approach is to cache only `~/.cargo/registry` and `~/.cargo/git`, skipping the `target/` directory entirely. This saves the dependency download but recompiles everything from scratch. For a project that changes frequently, caching `target/` wins. For a library with infrequent builds, caching just the registry is simpler and less fragile.

## The release stage: building and signing

The release workflow triggers on version tags (`v*`) or manual dispatch. It has two jobs: build the binary and publish the release. Separating them means the publish step can retry without rebuilding.

Building a macOS app means producing a `.app` bundle, code signing it, optionally notarizing it, and packaging it into a DMG. Here is what the signing step looks like when you have a Developer ID certificate stored in repository secrets.

```yaml
- name: Import signing certificate
  if: env.APPLE_DEV_CERT_P12 != ''
  env:
    APPLE_DEV_CERT_P12: ${{ secrets.APPLE_DEV_CERT_P12 }}
    APPLE_DEV_CERT_PASSWORD: ${{ secrets.APPLE_DEV_CERT_PASSWORD }}
  run: |
    CERT_PATH="$RUNNER_TEMP/developer-id.p12"
    KEYCHAIN_PATH="$RUNNER_TEMP/signing.keychain-db"
    KEYCHAIN_PASSWORD="$(openssl rand -hex 16)"

    echo "$APPLE_DEV_CERT_P12" | base64 --decode > "$CERT_PATH"

    security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
    security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
    security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"

    security import "$CERT_PATH" \
      -P "$APPLE_DEV_CERT_PASSWORD" \
      -A -t cert -f pkcs12 \
      -k "$KEYCHAIN_PATH"
```

This creates a temporary keychain on the runner, imports the certificate, and makes it available to `codesign`. The keychain password is random and ephemeral. The certificate stays in memory for the duration of the job.

The fallback when no certificate is available is ad-hoc signing:

```yaml
- name: Ad-hoc codesign
  if: env.APPLE_DEV_CERT_P12 == ''
  run: codesign --force --deep --sign - "${{ steps.appbundle.outputs.path }}"
```

Ad-hoc signed apps will trigger Gatekeeper warnings on other people's machines. This is fine for internal testing or CI validation. It is not fine for distribution to users.

For [security considerations](/blog/rust-desktop-security-guide/) around code signing and update integrity, that topic deserves its own discussion. The short version: sign your updates, verify signatures on the client side, and never ship an app that downloads and runs unsigned binaries.

## Notarization: Apple's additional gate

Code signing proves the binary came from you. Notarization proves Apple scanned it for malware. Both are required for a smooth user experience on macOS. Without notarization, users get a dialog saying the app "cannot be opened because Apple cannot check it for malicious software."

The notarization step submits the DMG to Apple's servers and waits for a response:

```yaml
- name: Notarize
  run: |
    xcrun notarytool submit "$DMG_PATH" \
      --apple-id "$APPLE_ID" \
      --password "$APPLE_APP_PASSWORD" \
      --team-id "$APPLE_TEAM_ID" \
      --wait

    xcrun stapler staple "$DMG_PATH"
```

The `--wait` flag blocks until Apple finishes scanning, which usually takes 2 to 10 minutes. `stapler` attaches the notarization ticket to the DMG so the app can open without contacting Apple's servers. You need three secrets for this: your Apple ID email, an app-specific password (not your Apple account password), and your team ID.

## Publishing the release and update manifest

After the DMG is built and signed, the publish job creates a GitHub Release and generates an update manifest. The manifest is a JSON file that the app's auto-updater polls to check for new versions.

```json
{
  "version": "0.3.0",
  "release_notes": "Fixed sidebar collapse state persisting across restarts",
  "platforms": {
    "macos-aarch64": {
      "url": "https://github.com/user/repo/releases/download/v0.3.0/GPUI-Starter-0.3.0.dmg",
      "signature": "base64-encoded-ed25519-signature",
      "size": 8523412
    }
  }
}
```

The `signature` field is an Ed25519 signature of the DMG's SHA-256 hash. The app bundles the public key and verifies the signature before accepting an update. This prevents a compromised CDN or MITM attack from shipping malicious code, which I cover in the [security guide for Rust desktop apps](/blog/rust-desktop-security-guide/).

The manifest generation script downloads the DMG from the GitHub Release, computes its hash, signs it with a private key stored in repository secrets, and writes the JSON. Then it clones the `gh-pages` branch, places `manifest.json` at the root, and pushes.

On the client side, the updater service polls this manifest on a timer. Here is the Rust struct that deserializes it:

```rust
#[derive(Debug, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub platforms: std::collections::HashMap<String, PlatformAsset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlatformAsset {
    pub url: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub size: u64,
}
```

The updater compares the manifest version against the running app version, downloads the new DMG if available, verifies the Ed25519 signature against the bundled public key, and presents the update to the user. This runs as a background service inside the app, tracked through a state machine with statuses like `Checking`, `Available`, `Downloading`, and `ReadyToInstall`.

## Docs deployment as a bonus pipeline

If your project has a documentation site, you can add a second workflow for that. The [getting started guide](/docs/getting-started/) and other docs for gpui-starter deploy from the same repository, triggered by changes to the `web/` directory:

```yaml
on:
  push:
    branches: [master]
    paths:
      - 'web/**'
      - 'astro.config.mjs'
      - 'package.json'
```

Path-based triggers mean the docs workflow only runs when documentation files change. The build step uses Bun and Astro, and deployment goes to Cloudflare Pages via Wrangler. This is separate from the app release pipeline, which is the right call. Docs updates and app updates have different cadences.

## Common mistakes I have made

**Not pinning action versions.** Using `actions/cache@v4` is fine. Using `actions/cache@master` will break your builds when a major version lands. Pin to major version tags or, for production pipelines, to full commit SHAs.

**Skipping the cache.** I did this for two weeks because the cache configuration seemed complicated. Those two weeks cost me roughly 40 hours of waiting for builds. Set up the cache on day one.

**Not testing the release workflow locally.** GitHub Actions workflows are hard to debug because you can only test them by pushing. Use `act` (the local GitHub Actions runner) or at minimum test each step's shell script locally before wrapping it in YAML.

**Forgetting the `--wait` flag on notarization.** Without it, the step succeeds immediately and proceeds to staple a ticket that does not exist yet. The DMG ships without a notarization ticket, and users get the Gatekeeper dialog.

**Not rotating the signing key.** The Ed25519 private key for update signing lives in repository secrets. If it leaks, an attacker can sign malicious updates. Generate a new key pair, update the secret, and ship a new app version with the updated public key. This is painful, which is why you should keep the key in a secrets manager from the start.

## Putting it together

The full pipeline for a Rust desktop app on GitHub Actions looks like this: CI runs on every push with four parallel jobs (check, test, clippy, fmt). Release runs on tags with a build job that produces a signed, notarized DMG and a publish job that creates a GitHub Release with an Ed25519-signed update manifest. Docs deploy separately on path-based triggers.

The initial setup takes a day or two, mostly spent on Apple developer account configuration and certificate wrangling. After that, the pipeline runs itself. Push a tag, get a release. Merge a docs PR, get a deployed site.

If you want to see this pipeline in action with a real GPUI desktop application, [gpui-starter](https://github.com/freeoxide/gpui-starter) ships with all of these workflows configured out of the box: CI, release with signing and notarization, manifest generation, and docs deployment. The [getting started guide](/docs/getting-started/) walks through the setup including repository secrets and Apple developer configuration.
