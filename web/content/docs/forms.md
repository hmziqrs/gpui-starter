---
title: Forms
description: Build validated forms in GPUI using gpui-form derive macros, koruma validation rules, and localized error messages with es-fluent
---

## Forms in gpui-starter

gpui-starter ships a working registration form that ties together three crates: **gpui-form** generates form state and UI bindings from derive macros, **koruma** provides composable validation rules, and **es-fluent** localizes every label and error message. The form lives in `src/features/pages/form_page.rs`.

This page walks through the full setup: defining a form model, wiring input subscriptions, rendering the UI, and handling submit with inline errors. For the deeper tutorial on writing custom validators, see the [form validation in Rust with koruma](/blog/form-validation-koruma-rust/) blog post.

## Defining a form model

A single struct with derive macros handles the setup:

```rust
#[derive(Clone, Debug, Default, EsFluentVariants, GpuiForm, Koruma, KorumaAllFluent)]
#[fluent_variants(keys = ["description", "label"])]
#[gpui_form(koruma(fluent))]
pub struct RegistrationForm {
    #[gpui_form(component(input))]
    #[koruma(NonEmptyValidation::<_>::builder())]
    pub name: String,

    #[gpui_form(component(input))]
    #[koruma(EmailValidation::<_>::builder())]
    pub email: String,

    #[gpui_form(component(input))]
    #[koruma(NonEmptyValidation::<_>::builder())]
    pub password: String,

    #[gpui_form(component(input))]
    #[koruma(PhoneNumberValidation::<_>::builder())]
    pub phone: String,

    #[gpui_form(component(input))]
    #[koruma(UrlValidation::<_>::builder())]
    pub website: String,
}
```

Each derive serves a specific purpose:

| Derive | What it generates |
|--------|-------------------|
| `GpuiForm` | `RegistrationFormFormFields` (entity holders for each input), `RegistrationFormFormValueHolder` (typed value storage), `RegistrationFormFormComponents` (factory methods to create input entities) |
| `Koruma` | A `validate()` method on the value holder that runs all field rules and returns `Result<(), ValidationErrors>` |
| `EsFluentVariants` | `RegistrationFormLabelVariants` and `RegistrationFormDescriptionVariants` enums for typed message lookup |
| `KorumaAllFluent` | Fluent-backed error messages for each validator, auto-localized to the current locale |

## Validation rules

The validators come from `koruma-collection`, which ships two categories:

**Collection validators** (in `koruma_collection::collection`):

| Validator | What it checks | When to use |
|-----------|---------------|-------------|
| `NonEmptyValidation` | Field is not empty or whitespace | Required fields: name, password |

**Format validators** (in `koruma_collection::format`):

| Validator | What it checks | Example valid input |
|-----------|---------------|---------------------|
| `EmailValidation` | Standard email format | `user@example.com` |
| `PhoneNumberValidation` | North American phone format | `(555) 123-4567` |
| `UrlValidation` | Valid URL with scheme | `https://example.com` |

All validators use the builder pattern. You can chain configuration methods on the builder to customize behavior (error messages, strictness levels). The `#[koruma(...)]` attribute on each field is all the wiring you need.

## Localized labels and error messages

The `#[fluent_variants(keys = ["description", "label"])]` attribute tells `es-fluent` to generate two enums: one for field labels and one for descriptions. These map directly to keys in your `.ftl` files.

From `i18n/en/gpui-starter.ftl`:

```ftl
registration_form_label_variants-name = Full Name
registration_form_label_variants-email = Email
registration_form_label_variants-password = Password
registration_form_label_variants-phone = Phone
registration_form_label_variants-website = Website

registration_form_description_variants-name = Enter your full name.
registration_form_description_variants-email = We'll never share your email with anyone else.
registration_form_description_variants-password = Choose a strong password.
registration_form_description_variants-phone = Format: (555) 123-4567
registration_form_description_variants-website = Optional. Your personal or company site.
```

Because `KorumaAllFluent` is derived alongside `EsFluentVariants`, validation error messages also resolve through the Fluent system. A missing name field produces "Enter your full name." in English or the translated equivalent in the active locale. See the [internationalization docs](/docs/i18n/) for how to add new locales.

## Wiring input state

`GpuiForm` generates a `RegistrationFormFormFields` struct that holds an `Entity<InputState>` per field. You create these in the page constructor and subscribe to change events:

```rust
pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
    let current_data = RegistrationFormFormValueHolder::default();

    // Create input entities via the generated factory methods
    let name_input = cx.new(|cx| RegistrationFormFormComponents::name_input(window, cx));
    let email_input = cx.new(|cx| RegistrationFormFormComponents::email_input(window, cx));
    // ... one per field

    // Subscribe to changes to keep current_data in sync
    let _subscriptions = vec![
        cx.subscribe(
            &name_input,
            |this: &mut FormPage, state: Entity<InputState>, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = state.read(cx).value();
                    this.current_data.name = if text.is_empty() {
                        None
                    } else {
                        Some(text.to_string())
                    };
                }
            },
        ),
        // ... one subscription per field
    ];

    Self {
        current_data,
        fields: RegistrationFormFormFields {
            name_input,
            email_input,
            // ... rest
        },
        agree_terms: false,
        submitted: false,
        touched: false,
        _subscriptions,
    }
}
```

The `touched` field is important. Validation only runs after the user clicks submit. This avoids showing errors on fields the user has not interacted with yet. It is a common UX pattern in form libraries.

## Rendering the form

Use `v_form()` and `field()` from `gpui_component` to build the layout. Each field gets a localized label, a required marker, a description, and an error slot:

```rust
v_form()
    .label_width(px(160.))
    .child(
        field()
            .label(crate::i18n::localize_message(
                &RegistrationFormLabelVariants::Name,
            ))
            .required(true)
            .description_fn({
                let error = error_for("name");
                let description = crate::i18n::localize_message(
                    &RegistrationFormDescriptionVariants::Name,
                );
                move |_, _| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().child(description.clone()))
                        .when_some(error.clone(), |el, err| {
                            el.child(div().text_color(danger).text_xs().child(err))
                        })
                }
            })
            .child(Input::new(&self.fields.name_input)),
    )
```

The `description_fn` closure captures both the help text and any validation error for that field. When `touched` is false, `error_for()` returns `None` and the user sees only the description. After submit, errors appear inline below the help text in the danger color.

### Extracting errors per field

Validation produces a `ValidationErrors` struct with typed accessors for each field:

```rust
let error_for = |field_name: &str| -> Option<String> {
    validation_errors.as_ref().and_then(|e| {
        let errs: Vec<String> = match field_name {
            "name" => e.name().all().iter().map(crate::i18n::localize_message).collect(),
            "email" => e.email().all().iter().map(crate::i18n::localize_message).collect(),
            // ... other fields
            _ => Vec::new(),
        };
        if errs.is_empty() { None } else { Some(errs.join("\n")) }
    })
};
```

Each call like `e.name()` returns the list of validation failures for that field. `localize_message` converts each failure to a Fluent string in the current locale. Multiple errors on the same field are joined with newlines.

## Handling submission

The submit button runs validation, checks the terms checkbox, and pushes a notification:

```rust
Button::new("submit")
    .primary()
    .label(crate::i18n::localize("form_submit", None))
    .on_click(cx.listener(|this, _, window, cx| {
        this.touched = true;
        let valid = this.current_data.validate().is_ok();
        if valid && this.agree_terms {
            this.submitted = true;
            window.push_notification(
                crate::i18n::localize("form_notification_submitted", None),
                cx,
            );
        } else if valid && !this.agree_terms {
            window.push_notification(
                crate::i18n::localize("form_notification_agree_terms", None),
                cx,
            );
        } else {
            window.push_notification(
                crate::i18n::localize("form_notification_fix_errors", None),
                cx,
            );
        }
        cx.notify();
    }))
```

There are three outcomes:

1. Valid and terms accepted: mark `submitted = true`, show success banner, push notification.
2. Valid but terms not accepted: push a notification asking the user to agree to terms.
3. Invalid: push a notification to fix errors. The next render cycle picks up `touched = true` and displays inline errors.

The reset button clears all input values, resets `touched` and `submitted`, and calls `cx.notify()` to re-render:

```rust
fn on_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self.current_data = RegistrationFormFormValueHolder::default();
    self.fields.name_input.update(cx, |s, cx| s.set_value("", window, cx));
    // ... clear other fields
    self.agree_terms = false;
    self.submitted = false;
    self.touched = false;
    cx.notify();
}
```

## Adding custom validators

If the built-in validators from `koruma-collection` do not cover your needs, you can write custom validators by implementing koruma's trait. The [form validation tutorial](/blog/form-validation-rust-tutorial/) covers this in detail with a worked example of a password strength validator.

## Related

- [Internationalization](/docs/i18n/): how Fluent message files work and how to add new languages
- [Notifications](/docs/notifications/): the `push_notification` API used in form submission
- [Architecture](/docs/architecture/): entity ownership patterns that apply to form state management
- [Testing](/docs/testing/): strategies for testing form validation logic in isolation
