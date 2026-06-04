---
question: "How do I add form validation in a GPUI Rust app?"
description: "gpui-starter uses gpui-form derive macros with koruma validation rules for type-safe, localized form handling. Covers built-in validators, custom rules, inline errors, and the touched pattern."
category: "Features"
order: 8
---

gpui-starter validates forms through two crates: `gpui-form` generates form state and UI bindings from derive macros on your struct, and `koruma` runs composable validation rules against the field values. Error messages go through Fluent so they follow the active locale without extra work.

## Defining a form with validation

One struct with derive macros does the wiring. Each field gets a component type from `gpui-form` and a validator from `koruma-collection`:

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

Four derives on one struct, each with a specific job:

| Derive | What it produces |
|--------|-----------------|
| `GpuiForm` | Entity holders, typed value storage, factory methods for input entities |
| `Koruma` | A `validate()` method that runs all field rules and returns `Result<(), ValidationErrors>` |
| `EsFluentVariants` | Typed enums for looking up labels and descriptions from Fluent |
| `KorumaAllFluent` | Fluent-backed error messages for every validator, localized to the current locale |

## Built-in validators

`koruma-collection` ships validators in two categories:

**Collection validators** (`koruma_collection::collection`):

| Validator | Checks | Use for |
|-----------|--------|---------|
| `NonEmptyValidation` | Field is not empty or whitespace-only | Required fields: name, password |

**Format validators** (`koruma_collection::format`):

| Validator | Checks | Example valid input |
|-----------|--------|---------------------|
| `EmailValidation` | Standard email format | `user@example.com` |
| `PhoneNumberValidation` | North American phone format | `(555) 123-4567` |
| `UrlValidation` | Valid URL with scheme | `https://example.com` |

All validators use the builder pattern. You can chain configuration methods on the builder to customize strictness or error messages.

## How validation and rendering connect

The `GpuiForm` derive generates a `RegistrationFormFormFields` struct that holds an `Entity<InputState>` per field. You create these in the page constructor and subscribe to change events to keep a `current_data` struct in sync.

Validation runs on submit, not on every keystroke. A `touched` boolean tracks whether the user has attempted to submit. Errors only render when `touched` is true, so the form stays clean until the user acts.

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

Three outcomes: valid with terms accepted (success), valid without terms (reminder notification), or invalid (fix-errors notification plus inline error display on the next render).

## Inline errors per field

Validation produces a `ValidationErrors` struct with typed accessors for each field. You extract errors and render them next to the relevant input:

```rust
let error_for = |field_name: &str| -> Option<String> {
    validation_errors.as_ref().and_then(|e| {
        let errs: Vec<String> = match field_name {
            "name" => e.name().all().iter().map(crate::i18n::localize_message).collect(),
            "email" => e.email().all().iter().map(crate::i18n::localize_message).collect(),
            _ => Vec::new(),
        };
        if errs.is_empty() { None } else { Some(errs.join("\n")) }
    })
};
```

`localize_message` converts each validation failure to a Fluent string in the current locale. Multiple errors on the same field are joined with newlines.

## Custom validators

If the built-in validators do not cover your use case, you can implement koruma's validation trait directly. The [form validation tutorial](/blog/form-validation-rust-tutorial/) walks through building a password strength validator from scratch. The [koruma deep dive](/blog/form-validation-koruma-rust/) explains the trait interface and how to compose custom rules with the built-in ones.

## Localized labels and descriptions

The `#[fluent_variants(keys = ["description", "label"])]` attribute generates enums that map to keys in your `.ftl` files:

```ftl
registration_form_label_variants-name = Full Name
registration_form_description_variants-name = Enter your full name.
```

Because `KorumaAllFluent` is derived alongside `EsFluentVariants`, validation error messages also resolve through Fluent. A missing name field produces the localized help text, not a hard-coded English string. See the [i18n docs](/docs/i18n/) for how to add new locales.

## Where to go next

- [Forms documentation](/docs/forms/) for the full API reference, including wiring input subscriptions and the reset flow.
- [Form validation tutorial](/blog/form-validation-rust-tutorial/) for a hands-on walkthrough of adding validation to a registration form.
- [Form validation with koruma](/blog/form-validation-koruma-rust/) for the conceptual overview of how gpui-form and koruma fit together.
- [Notifications](/docs/notifications/) for the `push_notification` API used during form submission.
- [Architecture](/docs/architecture/) for the entity ownership patterns that underlie form state management.
