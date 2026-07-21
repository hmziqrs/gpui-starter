---
title: "Making Rust desktop apps accessible with AccessKit"
description: "Integrating AccessKit for accessibility in Rust GUI apps: screen readers, keyboard navigation, and ARIA roles."
date: 2026-06-05
tags: [Rust, accessibility, AccessKit]
draft: false
---

Most Rust GUI frameworks ship without accessibility support. You get a window on screen, maybe some input handling, but nothing that talks to VoiceOver on macOS or NVDA on Windows. If you're building a tool for yourself, you might not notice. If you're building software other people use, that's a problem. Roughly 15% of the global population has some form of disability, and screen reader users are the most underserved group in desktop software.

AccessKit fixes this for Rust. It provides a cross-platform accessibility tree that bridges your UI to the operating system's assistive technology APIs. This post covers how it works, how to integrate it, and where the rough edges are.

## What AccessKit actually does

AccessKit is not a rendering library. It's a data structure and a bridge. You describe your UI as a tree of nodes, each with a role (button, text field, heading), optional labels, states (checked, expanded, disabled), and supported actions (click, focus, increment). AccessKit sends that tree to the OS accessibility API. VoiceOver, Narrator, Orca, and NVDA all read from those same OS APIs.

The core types are `Node`, `Role`, `Tree`, and `TreeUpdate`. A `Node` represents one accessible element. A `Tree` is the root container with metadata. A `TreeUpdate` is how you push changes: you send a list of changed nodes and AccessKit reconciles them with what the OS already knows about.

A minimal tree looks like this:

```rust
use accesskit::{Node, NodeId, Role, Tree, TreeUpdate};

let root_id = NodeId(0);
let button_id = NodeId(1);

let root = Node::new(Role::Window)
    .with_label("My App")
    .with_children([button_id]);

let button = Node::new(Role::Button)
    .with_label("Submit")
    .add_action(accesskit::Action::Focus);

let update = TreeUpdate {
    nodes: vec![
        (root_id, root),
        (button_id, button),
    ],
    tree: Some(Tree::new(root_id)),
    tree_id: accesskit::TreeId::ROOT,
    focus: root_id,
};
```

Every node has a `Role`. The `Role` enum has over 180 variants derived from the ARIA specification: `Button`, `TextInput`, `CheckBox`, `Slider`, `TabList`, `MenuItem`, `Dialog`, `Heading`, `Link`, `Table`, `ListBox`, and so on. When VoiceOver encounters a node with `Role::Button`, it announces it as a button and tells the user they can press it. The role is the most important property you set.

## The integration pattern

AccessKit is framework-agnostic. It doesn't know about GPUI, Iced, egui, or any other Rust UI library. Integration works in three steps.

First, you create an accessibility tree when your window opens. This is the initial snapshot of your UI structure.

Second, you send `TreeUpdate` values whenever the UI changes. A button getting disabled, a tab becoming active, a dialog opening. Each update carries the changed nodes and the current focus state.

Third, you receive `ActionRequest` values from the OS. When a screen reader user presses Enter on a button, AccessKit sends you an `ActionRequest` with `Action::Default` targeting that button's `NodeId`. You handle it the same way you'd handle a keyboard event: dispatch it to your app logic.

Chromium and Firefox use this model internally. AccessKit exposes it as a Rust-native API instead of a C++ one.

## Building an accessible tree for a real UI

Consider a settings page with a labeled text input and a checkbox. The accessible tree needs to express the semantic relationships between these elements, not just their existence.

```rust
use accesskit::{Node, NodeId, Role, Tree, TreeUpdate};

fn build_settings_tree() -> TreeUpdate {
    let root_id = NodeId(0);
    let name_group_id = NodeId(1);
    let name_label_id = NodeId(2);
    let name_input_id = NodeId(3);
    let notify_checkbox_id = NodeId(4);

    let root = Node::new(Role::Window)
        .with_label("Settings")
        .with_children([name_group_id, notify_checkbox_id]);

    // Group the label and input together so the screen reader
    // announces "Name, edit text" instead of two unrelated elements.
    let name_group = Node::new(Role::Group)
        .with_label("Display name")
        .with_children([name_input_id]);

    let name_input = Node::new(Role::TextInput)
        .with_label("Display name")
        .with_value("Alice")
        .add_action(accesskit::Action::Focus);

    let notify_checkbox = Node::new(Role::CheckBox)
        .with_label("Enable notifications")
        .with_checked_state(accesskit::CheckedState::True)
        .add_action(accesskit::Action::Focus);

    TreeUpdate {
        nodes: vec![
            (root_id, root),
            (name_group_id, name_group),
            (name_input_id, name_input),
            (notify_checkbox_id, notify_checkbox),
        ],
        tree: Some(Tree::new(root_id)),
        tree_id: accesskit::TreeId::ROOT,
        focus: name_input_id,
    }
}
```

The `Group` node wrapping the label and input is important. Without it, VoiceOver announces the label and the text field as separate, unrelated items. With the group, it announces "Display name, edit text, Alice" as a single logical unit. Web developers use the same technique with `<fieldset>` and `aria-labelledby`, but applied to a native tree.

Focus tracking matters too. The `focus` field in `TreeUpdate` tells the OS which node has keyboard focus. You must send the current focus with every update, even if it hasn't changed. If you forget, the screen reader loses track of where the user is.

## Handling actions from assistive technology

When a screen reader user activates a button or toggles a checkbox, you get an `ActionRequest`. You need to handle these just like mouse clicks and keyboard events.

```rust
fn handle_action_request(request: &accesskit::ActionRequest) {
    match request.action {
        accesskit::Action::Default => {
            // The user activated this node (pressed Enter on a button,
            // double-tapped on mobile, etc.)
            match request.target_node {
                id if id == NodeId(4) => toggle_notifications(),
                _ => {}
            }
        }
        accesskit::Action::Focus => {
            // The user moved focus to this node
            focus_element(request.target_node);
        }
        accesskit::Action::SetValue => {
            // A text input value changed via the screen reader
            if let Some(accesskit::ActionData::Value(value)) = &request.data {
                update_input_value(request.target_node, value.clone());
            }
        }
        _ => {}
    }
}
```

The `Action::Default` is the most common one. It fires when a screen reader user "clicks" a button, toggles a checkbox, or activates a link. `Action::SetValue` fires when the user types into a text field through the assistive technology interface. There's also `Action::Increment` and `Action::Decrement` for sliders and spin buttons.

The tradeoff here is that you're implementing two input paths: one for keyboard/mouse and one for accessibility actions. In practice, you can often merge them. Both paths end up calling the same `toggle_notifications()` function. The difference is just how the event arrives.

## Keyboard navigation is not optional

Screen reader users navigate with the keyboard. Tab moves between interactive elements. Arrow keys move within groups (between radio buttons in a group, between items in a list). Enter and Space activate things. Escape closes dialogs.

If your app doesn't handle keyboard focus correctly, none of the AccessKit integration matters. The screen reader follows the focus. If focus jumps to an unexpected place or disappears entirely, the user is lost.

The [testing GPUI applications guide](/blog/testing-gpui-applications/) covers focus testing in detail, but the rule to remember is: every interactive element needs a focus target, and focus order should match visual order. If you have a dialog with a close button and a text field, Tab should go from the text field to the close button, not skip around based on DOM-like declaration order.

## Where things get complicated

AccessKit handles the data model well. The integration points are where it gets tricky.

Tree updates need to be complete. When you send a `TreeUpdate`, every changed node must include all its properties, not just the ones that changed. If a button had `Role::Button` and a label in the previous update, and you send an update that changes only the disabled state, you still need to include the role and label. This isn't hard to get right, but it's easy to get wrong, and the failure mode is a silent missing property that confuses the screen reader.

Focus must be sent with every update. I mentioned this already, but it's worth repeating because it's the single most common bug I've seen. If you push a tree update without the focus field, the screen reader assumes nothing is focused and stops reading.

Platform differences exist. VoiceOver on macOS, Narrator on Windows, and Orca on Linux all interpret the same accessibility tree slightly differently. VoiceOver is the most forgiving. Narrator is strict about role hierarchy. Orca has quirks with live regions. You need to test on at least macOS and Windows before shipping.

The crate itself is version 0.21. The API is stable in practice but the semver contract says it can change. Pin your dependency version.

## AccessKit in gpui-starter

The gpui-starter project includes AccessKit as a dependency. The integration is in `src/services/accessibility.rs`. It initializes a global `AccessibilitySnapshot` that reports whether the AccessKit bridge is active and what capabilities are available:

```rust
use gpui::{App, Global};

#[derive(Clone, Debug)]
pub struct AccessibilitySnapshot {
    pub accesskit_linked: bool,
    pub bridge_enabled: bool,
    pub status: String,
}

pub fn initialize(cx: &mut App) {
    let _role = accesskit::Role::Window;
    let snapshot = AccessibilitySnapshot::default();
    cx.set_global(snapshot);
}
```

The snapshot is wired into the diagnostics page, so you can see the current accessibility status alongside other system capabilities. The full bridge (two-way communication with the OS accessibility API) is marked as a work in progress. The current state provides keyboard and focus support as a baseline, with the full AccessKit tree bridge coming in a future release.

This is the honest state of AccessKit integration in most Rust GUI frameworks right now. The types are there, the tree model is solid, and the OS bridge works. The missing piece is automatic tree generation from your UI components. Right now you build the tree manually. A future version of GPUI will likely generate AccessKit nodes from elements automatically, like Chromium generates accessibility nodes from the DOM.

## What I'd recommend

Start with keyboard navigation. Make sure every button, input, and interactive element can be reached with Tab and activated with Enter or Space. Test this without a mouse. If you can't use your app with only a keyboard, a screen reader won't be able to either.

Then add AccessKit nodes for your main UI surfaces. Start with the window root, navigation, and primary actions. You don't need to annotate every decorative element. Screen readers skip nodes without labels or roles.

Test with VoiceOver on macOS. Press Cmd+F5 to enable it, then use Tab and arrow keys to navigate your app. VoiceOver will tell you what it sees. If it announces "unknown" for an element, that element is missing a role. If it says nothing, the element isn't in the tree.

For a working example of the patterns described here, check out [gpui-starter](/docs/getting-started/) which includes AccessKit integration, keyboard navigation, and a diagnostics page showing accessibility status. The source is on [GitHub](https://github.com/freeoxide/gpui-starter). For more on building desktop apps with this stack, see the [architecture overview](/blog/building-desktop-apps-rust-gpui/) and the [state management guide](/blog/state-management-gpui-entities/).
