---
title: "Form validation in Rust with koruma and gpui-form"
description: "Learn how to build validated forms in GPUI apps using koruma derive macros and composable rules. Covers localised errors, the touched pattern, and custom validators."
date: 2026-05-21
tags: [Rust, GPUI, forms]
draft: false
---

Every desktop app needs forms. Login screens, settings panels, search bars with filters. And every form needs validation. In most Rust UI frameworks, that means manually wiring up change handlers, tracking which fields the user has touched, collecting error messages, and rendering them next to the right input. It's repetitive work that's easy to get wrong.

gpui-starter ships with two crates that handle most of this boilerplate: `gpui-form` generates form UI and state from a struct definition, and `koruma` handles validation with composable rules. Together they turn a form that would take 200 lines of glue code into something declared through attributes.

## The derive macro approach

Most UI frameworks make you define your data model and your form UI separately. You write a struct for the data, then build the form by hand, then write a validation function, then wire the errors back to the UI. Four separate things that all need to stay in sync.

`gpui-form` collapses this into one struct:

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

Each field declares its UI component through `gpui_form`, its validation through `koruma`, and its label through `fluent_variants`. The derive macros generate input state management, validation logic, and typed label/description accessors at compile time. You never write a `match` statement to figure out which field failed. The generated code handles that.

## Validation rules

`koruma` ships four built-in validators through `koruma-collection`:

- `NonEmptyValidation` rejects empty strings
- `EmailValidation` checks email format
- `PhoneNumberValidation` validates phone number patterns
- `UrlValidation` ensures proper URL structure

All of them use the builder pattern. This also means validators are composable. You can configure a phone number validator to accept specific formats, or chain multiple rules on a single field:

```rust
#[koruma(
    NonEmptyValidation::<_>::builder(),
    PhoneNumberValidation::<_>::builder()
)]
pub phone: String,
```

When you call `validate()` on the struct, every rule runs and collects errors into a `ValidationErrors` map. You get all the problems at once, not just the first one. I find this preferable to fail-fast validation because users can fix everything in one pass instead of submitting, fixing one thing, submitting again, and so on.

## Localized error messages

Validation errors need to be readable. Hard-coded English strings do not work for an app that supports multiple languages. `koruma` integrates with the same [Fluent](/docs/i18n/) system that gpui-starter uses for all its UI text.

The `KorumaAllFluent` derive macro generates Fluent message keys for every validation rule on every field. Combined with `EsFluentVariants`, you get typed accessors for labels and descriptions too:

```ftl
# src/i18n/en.ftl
registration_form_label_variants-name = Full Name
registration_form_label_variants-email = Email
registration_form_description_variants-email = We'll never share your email with anyone else.

registration_form_koruma_variants-name-non-empty = Name is required.
registration_form_koruma_variants-email-email = Please enter a valid email address.
registration_form_koruma_variants-phone-phone-number = Please enter a valid phone number.
```

```ftl
# src/i18n/zh-CN.ftl
registration_form_label_variants-name = 姓名
registration_form_koruma_variants-name-non-empty = 请输入姓名。
registration_form_koruma_variants-email-email = 请输入有效的电子邮件地址。
```

When a user switches language at runtime, the validation messages update on the next frame. No extra code required. This is one of the things I like about building on Fluent rather than rolling a custom i18n layer: the message resolution is already wired into the rendering pipeline, so runtime language switching is free.

## Building the form UI

The `gpui-component` crate provides `v_form()` and `field()` builders for layout, labels, required indicators, and error display. Here is a complete registration form view:

```rust
fn render_form(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
    let errors = if self.touched {
        self.current_data.validate().err()
    } else {
        None
    };

    v_form()
        .label_width(px(160.))
        .child(
            field()
                .label(localize_message(&RegistrationFormLabelVariants::Name))
                .required(true)
                .error(errors.as_ref().and_then(|e| e.get("name")))
                .child(Input::new(&self.fields.name_input)),
        )
        .child(
            field()
                .label(localize_message(&RegistrationFormLabelVariants::Email))
                .required(true)
                .description(localize_message(
                    &RegistrationFormDescriptionVariants::Email,
                ))
                .error(errors.as_ref().and_then(|e| e.get("email")))
                .child(Input::new(&self.fields.email_input)),
        )
        .child(
            Button::new("submit")
                .primary()
                .label("Create Account")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.touched = true;
                    let valid = this.current_data.validate().is_ok();
                    if valid && this.agree_terms {
                        this.submitted = true;
                        window.push_notification("Form submitted successfully!", cx);
                    }
                })),
        )
}
```

The important pattern here is the `touched` flag. Validation only runs after the user has attempted to submit. Before that, fields show no errors. This prevents the aggressive "everything is red" problem that forms have when they validate on every keystroke. I have shipped forms both ways, and the touched pattern produces far fewer support tickets from confused users.

The `errors` variable is an `Option<ValidationErrors>`. Each field extracts its own error with `.get("field_name")`, which returns `Option<String>`. The `field()` builder renders the error message inline, directly below the input, in the theme's error color.

## What the macros generate at compile time

Here is what happens during compilation:

- `GpuiForm` generates a `FormFields` struct that holds input state for each field, plus methods to extract current values into your data struct
- `Koruma` generates a `validate()` method that runs every validator and collects errors into a `HashMap<String, String>`
- `KorumaAllFluent` generates Fluent message lookups for each validation error, so error strings come from `.ftl` files instead of hard-coded strings
- `EsFluentVariants` generates the `LabelVariants` and `DescriptionVariants` enums with typed accessors per field

You can see the full working example in `src/views/form_page.rs` in the gpui-starter repo. For details on how gpui-form generates component bindings, the [component architecture docs](/docs/components/) break down the rendering pipeline.

## Writing custom validators

The built-in rules cover common cases. For anything domain-specific, such as credit card Luhn checks, postal codes by country, or custom business logic, you implement `koruma`'s `Validation` trait directly:

```rust
struct PostalCodeValidation;

impl Validation for PostalCodeValidation {
    type Value = String;
    fn validate(&self, value: &Self::Value) -> Result<(), String> {
        if value.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-') {
            Ok(())
        } else {
            Err("Invalid postal code format.".into())
        }
    }
}
```

Then use it with the same `#[koruma(...)]` attribute on your struct fields. The trait is straightforward: take a reference to the value, return `Ok(())` or `Err(String)`. The error message here is a plain string, not a Fluent key. If you need localized errors for custom validators, you will want to return a Fluent key as the error string and resolve it at render time. The [forms documentation](/docs/forms/) covers this pattern along with conditional validation and custom Fluent error keys.

If you are new to GPUI itself and want to see how forms fit into the broader app structure, check out the [building your first GPUI app](/blog/hello-gpui/) guide.
