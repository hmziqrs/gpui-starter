//! End-to-end tests for navigation: sidebar page registration, deep-link
//! route parsing, and view construction.

use gpui_starter::routes::AppRoute;
use gpui_starter::sidebar::Page;

// ---------------------------------------------------------------------------
// Sidebar pages
// ---------------------------------------------------------------------------

#[test]
fn test_sidebar_pages_exist() {
    let all = Page::all();
    // The app must expose exactly eleven pages.
    assert_eq!(
        all.len(),
        11,
        "expected 11 sidebar pages, got {}",
        all.len()
    );

    // Every page variant that AppRoot::active_page_view matches on must be
    // present in the canonical list.
    let expected = [
        Page::Home,
        Page::Form,
        Page::HttpLab,
        Page::HttpLabTesting,
        Page::Settings,
        Page::Notifications,
        Page::Diagnostics,
        Page::QueryDevTools,
        Page::QueryPlayground,
        Page::QueryDevToolsV2,
        Page::About,
    ];

    for page in &expected {
        assert!(
            all.contains(page),
            "Page::{page:?} missing from Page::all()"
        );
    }

    // Each page must return a non-empty title.
    for page in all {
        assert!(
            !page.title().is_empty(),
            "Page::{page:?} has an empty title"
        );
    }
}

// ---------------------------------------------------------------------------
// Route parsing (deep links)
// ---------------------------------------------------------------------------

#[test]
fn test_route_parsing() {
    // --- Top-level page routes -------------------------------------------
    let cases = &[
        ("gpui-starter://home", AppRoute::Page(Page::Home)),
        ("gpui-starter://form", AppRoute::Page(Page::Form)),
        ("gpui-starter://http", AppRoute::Page(Page::HttpLab)),
        (
            "gpui-starter://httplab-testing",
            AppRoute::Page(Page::HttpLabTesting),
        ),
        ("gpui-starter://settings", AppRoute::Page(Page::Settings)),
        (
            "gpui-starter://notifications",
            AppRoute::Page(Page::Notifications),
        ),
        (
            "gpui-starter://diagnostics",
            AppRoute::Page(Page::Diagnostics),
        ),
        ("gpui-starter://about", AppRoute::Page(Page::About)),
        // Sub-route
        (
            "gpui-starter://settings/notifications",
            AppRoute::SettingsNotifications,
        ),
    ];

    for (url, expected) in cases {
        let parsed =
            AppRoute::parse_deep_link(url).unwrap_or_else(|e| panic!("failed to parse {url}: {e}"));
        assert_eq!(
            parsed, *expected,
            "parse_deep_link({url}): expected {expected:?}, got {parsed:?}"
        );
    }

    // --- Round-trip: parse → to_url → parse ------------------------------
    for (url, _expected) in cases {
        let route = AppRoute::parse_deep_link(url).unwrap();
        let serialized = route.to_url();
        let reparsed = AppRoute::parse_deep_link(&serialized).unwrap();
        assert_eq!(
            reparsed, route,
            "round-trip failed for {url}: {route:?} → {serialized} → {reparsed:?}"
        );
    }

    // --- Invalid inputs --------------------------------------------------
    let bad = &[
        "https://example.com",
        "gpui-starter://missing-page",
        "gpui-starter://settings/..%2Fetc",
        "",
    ];

    for url in bad {
        assert!(
            AppRoute::parse_deep_link(url).is_err(),
            "expected parse failure for {url:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// View construction
// ---------------------------------------------------------------------------

/// Confirm that SettingsPage is publicly exported and its type can be
/// referenced. Full construction requires a GPUI window context (Theme,
/// LocaleState, and NativeNotificationState globals), so this test verifies
/// the route resolves to the Settings page and the module is importable.
#[test]
fn test_settings_page_loads() {
    // Verify the route parses correctly.
    let route = AppRoute::parse_deep_link("gpui-starter://settings").unwrap();
    assert_eq!(route, AppRoute::Page(Page::Settings));
    assert_eq!(route.page_for_render(), Page::Settings);

    // Verify the view type is exported and constructible in a GPUI context.
    // SettingsPage::new(window, cx) requires Theme, LocaleState, and
    // NativeNotificationState globals, so we confirm the type resolves.
    let _type_check: fn() = || {
        fn _assert_settings_page_constructible() {
            // This function exists solely to prove the type is public and
            // has the expected constructor signature at compile time.
            // Runtime construction requires a full GPUI window context.
            let _: fn(
                &mut gpui::Window,
                &mut gpui::Context<gpui_starter::views::SettingsPage>,
            ) -> gpui_starter::views::SettingsPage = gpui_starter::views::SettingsPage::new;
        }
    };
}

/// AboutPage is a zero-argument struct.  The test confirms the type is
/// publicly exported and can be instantiated without a GPUI context.
#[test]
fn test_about_page_loads() {
    let _page = gpui_starter::views::AboutPage::new();
}
