# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-06-05

### Added

- Auto-updater with signed manifests and Ed25519 verification
- Crash report generation and storage for application panics
- Frame-time debugger for performance profiling
- Background WebSocket support (optional feature)
- gpui-query async data fetching library (TanStack Query-inspired)
- gpui-query-v2 next-gen async state management
- Vendored gpui-component library from longbridge
- Vendored gpui-form for declarative form handling
- Error boundary with action-based triggers
- Release workflow with codesigning and notarization
- Deploy workflow for Cloudflare Pages

### Changed

- 60 new blog posts covering GPUI development topics
- Improved documentation coverage across all pages
- Updated FAQ entries with better SEO targeting
- Enhanced structured data and meta optimization
- Internal linking improvements across all pages

## [0.2.0] - 2025-06-15

### Added

- Desktop notification system with native OS backends, in-app toast fallback, and persistent inbox
- Diagnostics page showing live app state, subsystem status, and debug actions
- Telemetry module with disabled/local/remote modes and consent gate
- Secure storage module with OS keyring integration (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- Single-instance guard with IPC forwarding for deep links
- First-run detection and setup experience
- Undo/redo stack with command pattern and keyboard shortcuts (Cmd+Z/Cmd+Y)
- Custom title bar replacing native window chrome with drag regions and traffic light support
- Global keyboard shortcuts (Alt+Space hotkey, Cmd+K launcher)
- Background task manager for async operations
- Connectivity state monitoring with network probes
- File logging with tracing-appender
- Capabilities registry for runtime feature detection
- Lifecycle state machine for app startup/shutdown/crash handling
- Configuration migration system for schema changes

### Changed

- Expanded documentation site with 5 new reference pages (command launcher, notifications, secure storage, routing, testing)
- Added 12 new blog posts covering GPUI development topics
- Added 8 new FAQ entries across Features and Advanced categories
- Updated llms.txt with comprehensive feature coverage

## [0.1.0] - 2025-05-15

### Added

- **Multi-page architecture** with sidebar navigation and page routing via GPUI
- **21 built-in themes** with live hot-reloading and custom theme support
- **Internationalization (i18n)** supporting English and Chinese (zh-CN) via `es-fluent`
- **Form validation** with `gpui-form` derive macros and `koruma` validation rules
- **Command launcher** (Cmd+K) with fuzzy search across all app actions
- **macOS system tray** integration with app icon and quick-access menu
- **Secure credential storage** via OS keychain (`keyring` crate)
- **SQLite database** integration with `rusqlite` for local data persistence
- **Animated app preview** component with Three.js wireframe scenes
- **Documentation site** built with Astro Starlight
- **Custom marketing landing page** with 3D animations and glassmorphism design
- **Privacy policy** and **terms of use** pages

### Changed

- Moved web sources from `src/web` to `web` directory for cleaner project structure
- Enabled `apple-native` feature for keyring on macOS
- Switched to blocking `reqwest` for connectivity checks

### Fixed

- Animation now pauses on hover for better UX
- Added logging to secure storage operations for debugging
