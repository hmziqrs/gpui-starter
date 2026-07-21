# Contributing to GPUI Starter

Thanks for your interest in contributing. This guide covers everything you need to get started.

## Development Setup

**Requirements:**

- macOS (the app uses macOS-specific APIs for tray icons, hotkeys, and notifications)
- Rust nightly toolchain

**Install the nightly toolchain:**

```sh
rustup toolchain install nightly
```

**Clone and build:**

```sh
git clone https://github.com/freeoxide/gpui-starter.git
cd gpui-app
cargo build
```

The first build takes a while because it compiles GPUI and all dependencies. Subsequent builds are faster thanks to incremental compilation.

## Development Workflow

Use the provided shell script for fast iteration. It builds the binary, wraps it in a `.app` bundle, and signs it locally:

```sh
bash scripts/macos-dev-app.sh
```

Open the printed `.app` path to run the app. Repeat after each code change.

## Code Style

- Run `cargo fmt` before committing. All formatting decisions go through rustfmt.
- Run `cargo clippy` and fix any warnings. The CI pipeline will reject code that triggers clippy lints.
- Match the existing patterns in the codebase. Look at nearby files for conventions on imports, module structure, error handling, and naming.

## Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add user preference for default locale
fix: resolve crash when sidebar is toggled rapidly
docs: update CONTRIBUTING.md with theme guide
refactor: extract notification logic into its own module
test: add unit tests for route matching
chore: bump gpui-component dependency
```

Keep the subject line under 72 characters. Use the body for anything that needs explanation beyond the diff.

## How to Add Things

### New Page

1. Create a new file in `src/views/` (e.g. `src/views/my_page.rs`).
2. Implement a render function using the GPUI component patterns you see in existing views like `home.rs` or `settings.rs`.
3. Register the module in `src/views/mod.rs`.
4. Add a route in `src/routes.rs` and a sidebar entry if applicable.

### New Command

1. Define the command in `src/commands.rs` following the existing pattern.
2. Register the command handler in the relevant view or in `src/app.rs`.
3. Bind a keyboard shortcut in `src/shortcuts.rs` if the command should be accessible from the keyboard.

### New Theme

1. Create a JSON file in `themes/` (e.g. `themes/my-theme.json`).
2. Follow the structure of an existing theme file like `themes/gruvbox.json` or `themes/tokyonight.json`.
3. The theme will be discoverable by filename at runtime.

### New Locale

1. Create a directory under `i18n/` named after the locale code (e.g. `i18n/fr/`).
2. Add a `.ftl` (Fluent) file inside it mirroring the structure of `i18n/en/gpui-starter.ftl`.
3. Register the locale in the i18n setup within `src/i18n.rs`.

## Pull Request Process

1. **Fork** the repository and create a branch from `master`:
   ```sh
   git checkout -b feat/my-feature
   ```
2. **Commit** your changes with a conventional commit message.
3. **Push** to your fork and open a pull request against `master`.
4. Ensure `cargo fmt --check`, `cargo clippy`, and `cargo test` all pass locally before pushing.
5. Describe what the PR does and why in the description. Link any related issues.

Maintainers will review and merge. Small, focused PRs are easier to review and land faster.

## Testing

Run the full test suite:

```sh
cargo test
```

Add tests for any new functionality. Integration-style tests for GPUI views go in the `src/testing.rs` module following the existing patterns there. Unit tests for pure logic can live in the same file as the code they test, behind `#[cfg(test)]`.

## Architecture

For a high-level overview of the codebase structure, modules, and data flow, see [docs/gpui-architecture.md](docs/gpui-architecture.md).
