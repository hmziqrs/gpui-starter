---
title: "Building accessible Rust desktop applications"
description: "A practical guide to accessibility in Rust desktop apps: keyboard navigation, screen reader support, color contrast, and ARIA roles with AccessKit."
date: 2026-06-05
tags: [Rust, accessibility, guide]
draft: false
---

Accessibility in desktop apps is one of those things most developers know they should care about but rarely prioritize until someone files a bug titled "app completely unusable with keyboard only." I get it. Deadlines are tight, features ship, and accessibility feels like something you bolt on later. Except later never comes.

The reality is that building accessible software from the start is cheaper than retrofitting it. This post covers keyboard navigation, screen reader support, color contrast, and semantic roles. I will use Rust examples throughout, specifically with [GPUI](https://zed.dev) and [AccessKit](https://accesskit.dev), but the principles apply regardless of framework.

## Why accessibility matters for desktop apps

Roughly 15% of the global population lives with some form of disability. That includes visual impairments, motor limitations, cognitive differences, and hearing loss. If your app requires a mouse and perfect vision to operate, you have excluded a significant chunk of potential users.

There is also a legal angle. Section 508 in the US, the European Accessibility Act, and similar regulations worldwide increasingly mandate that software sold to governments or large organizations must meet accessibility standards. The Web Content Accessibility Guidelines (WCAG) 2.1 at the AA level have become the de facto baseline, even for non-web applications.

On the technical side, accessible apps tend to be better architected. When you build proper keyboard navigation, you are forced to think about focus flow and component structure. When you add semantic roles to your UI elements, you make the codebase more readable too. Good accessibility and good engineering reinforce each other.

## Keyboard navigation

Every interactive element in your app must be reachable and operable with a keyboard alone. Tab moves focus between controls, Shift+Tab moves backward, Enter and Space activate buttons, and arrow keys move through lists and menus.

In GPUI, focus management starts with `FocusHandle`. Every interactive component should own one:

```rust
use gpui::{FocusHandle, Focusable};

struct MyButton {
    focus_handle: FocusHandle,
    label: String,
    on_click: Box<dyn Fn(&mut Window, &mut App)>,
}

impl Focusable for MyButton {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
```

When you render the button, you tell GPUI to track focus on that handle. The framework handles the rest: drawing focus rings, forwarding keyboard events, and integrating with the platform's accessibility bridge.

A few rules I follow for keyboard navigation:

Tab order should match visual order. If your layout has a sidebar, main content area, and footer, keyboard users should encounter those regions in the same sequence as sighted users. Avoid using CSS or absolute positioning to visually reorder elements without also adjusting the focus order.

Provide visible focus indicators. The default 2px blue outline is fine. What is not fine is `outline: none` with nothing replacing it. GPUI draws focus rings automatically, but if you customize them, make sure the ring has at least 3:1 contrast against the background.

Support Escape to dismiss. Modals, dropdowns, and popovers should close when the user presses Escape. This is expected behavior and its absence is immediately noticeable to keyboard users.

## Screen reader support with AccessKit

Screen readers interpret your UI through an accessibility tree, which is a parallel representation of your visual layout. Each node in this tree has a role (button, heading, text field), a name (the label), and sometimes a value or state. If the tree is missing or incomplete, the screen reader has nothing to work with.

AccessKit is a Rust library that bridges your UI framework and the platform's native accessibility API. On macOS it talks to NSAccessibility, on Windows it uses UI Automation, and on Linux it connects to ATK. You write your accessibility tree once and AccessKit handles the platform-specific translation.

Here is what setting up a basic accessibility node looks like:

```rust
use accesskit::{Node, Role, StringEncoding};

fn build_accessibility_node(label: &str, role: Role) -> Node {
    let mut node = Node::new(role);
    node.set_name(StringEncoding::PlainText, label.to_string());
    node
}

// Usage in a button component
let button_node = build_accessibility_node("Submit form", Role::Button);
```

The roles you will use most often:

| Role | Use for |
|------|---------|
| `Role::Button` | Clickable actions |
| `Role::TextInput` | Text fields |
| `Role::CheckBox` | Toggle options |
| `Role::Slider` | Numeric ranges |
| `Role::Tab` | Tab bar items |
| `Role::Heading` | Section titles |
| `Role::List` | Collections of items |
| `Role::Dialog` | Modal overlays |

Do not skip naming your nodes. A button with no accessible label is announced as "button" by screen readers, which tells the user nothing. If your button displays an icon instead of text, provide a label through the accessibility node even though there is no visible text.

In the gpui-starter project, accessibility initialization lives in a service module:

```rust
pub fn initialize(cx: &mut App) {
    let _role = accesskit::Role::Window;
    let snapshot = AccessibilitySnapshot::default();
    cx.set_global(snapshot);
}
```

This registers the top-level window role and sets up a global snapshot that tracks whether the accessibility bridge is active. You can extend this pattern to build out the full tree as your UI renders.

## Color contrast and visual design

WCAG 2.1 AA requires a minimum contrast ratio of 4.5:1 for normal text and 3:1 for large text (18px bold or 24px regular). These numbers are not arbitrary. They represent the threshold where text becomes legible for users with moderate low vision or color deficiencies.

I check contrast ratios with [WebAIM's contrast checker](https://webaim.org/resources/contrastchecker/). Yes, it is a web tool, but the math is the same for desktop.

A few practical guidelines:

Never rely on color alone to convey information. If a form field turns red on validation error, also show an icon or text label. Red-green color blindness affects about 8% of men, and a red border with no text is invisible to them.

Test with high contrast mode. On Windows, users can enable High Contrast mode, which overrides your app's colors. Make sure your layouts still work when the OS enforces its own palette. GPUI's theme system helps here because you can define high-contrast variants alongside your regular themes.

Dark mode is not optional. Users with light sensitivity or certain visual conditions depend on dark themes. If you ship only a light theme, you are excluding them. The [theming guide](/docs/themes/) covers how to set up multiple themes with hot-reload.

## Semantic structure and ARIA roles

Web developers have ARIA attributes to add semantics to HTML. Desktop apps have AccessKit roles, which serve the same purpose. The idea is identical: give the accessibility tree enough information to describe what each element is and how it behaves.

Landmark roles help screen reader users jump between sections of your app. Think of them as chapter headings:

```rust
use accesskit::Role;

// Navigation sidebar
let nav_node = Node::new(Role::Navigation);
nav_node.set_name(StringEncoding::PlainText, "Main navigation".to_string());

// Main content area
let main_node = Node::new(Role::MainContent);
main_node.set_name(StringEncoding::PlainText, "Content".to_string());
```

When a screen reader encounters `Role::Navigation`, it can announce "navigation region" and let the user jump directly to it with a shortcut key. Without that role, the sidebar is just a generic group of buttons with no semantic meaning.

Live regions are another tool worth knowing about. If your app shows notifications or status updates, you want those announced automatically without requiring the user to move focus. AccessKit supports `aria-live` equivalents through its live region API. A toast notification with a live region role will be read aloud the moment it appears.

## Testing accessibility

Automated testing catches maybe 30% of accessibility issues. The rest requires manual testing with actual assistive technology. Here is my testing checklist:

1. Unplug your mouse. Try using the entire app with only the keyboard. Can you reach every control? Can you activate every button? Does focus ever disappear into a black hole?
2. Turn on VoiceOver (macOS) or NVDA (Windows). Close your eyes and try to complete a core task. Listen to what the screen reader announces. Is it enough to understand what is on screen?
3. Run a contrast audit. Check every text/background combination. Fix anything below 4.5:1.
4. Test at 200% zoom. Does your layout still work when the user scales text up? Overflow and clipped content are common failures here.

For automated checks in Rust, you can write tests that verify your accessibility tree structure:

```rust
#[test]
fn test_button_has_accessible_name() {
    let node = build_accessibility_node("Save changes", Role::Button);
    assert_eq!(node.name(), Some("Save changes"));
    assert_eq!(node.role(), Role::Button);
}
```

This does not replace manual testing, but it catches regressions where someone forgets to set a label after a refactor. The [testing guide](/docs/testing/) has more on writing these kinds of assertions.

## Common mistakes I have seen

Custom widgets with no role. You built a fancy toggle switch out of a styled div. It looks great. Screen readers see nothing. Always set the appropriate role (`Role::CheckBox` or `Role::Switch`) on custom components.

Missing focus trap in modals. When a dialog opens, focus should move into it. When it closes, focus should return to the element that triggered it. Without this, keyboard users tab into the void behind the overlay.

Dynamic content with no announcement. A loading spinner finishes and new content appears, but the screen reader says nothing. Use live regions to announce state changes.

Inconsistent shortcuts. Cmd+W closes a tab in one view and does nothing in another. Pick a consistent key binding scheme and stick with it across the entire app. Read the [command launcher docs](/docs/command-launcher/) for one approach to managing keyboard shortcuts centrally.

## Summary

Building accessible Rust desktop apps comes down to four things: every interactive element gets a keyboard path, every visual element gets a semantic role, every text element meets contrast requirements, and every state change gets announced when it matters. None of this is rocket science, but it does require intentional effort.

If you are building a GPUI application and want a starting point that already has AccessKit integration, keyboard navigation, focus management, and theme support built in, check out [gpui-starter](/docs/getting-started/). It is a production-ready boilerplate that handles these accessibility basics so you can focus on your app's actual features. The source code is on [GitHub](https://github.com/hmziqrs/gpui-boilerplate).
