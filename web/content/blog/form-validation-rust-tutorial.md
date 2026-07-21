---
title: "Form validation in Rust desktop apps: a practical tutorial"
description: "How to add form validation to a Rust GUI app using gpui-form and koruma. Email, phone, URL validators with localized errors."
date: 2026-06-05
tags: [Rust, forms, tutorial]
draft: false
---

Form validation in Rust GUI apps tends to be an afterthought. You build the UI, wire up the buttons, and then realize you need to check that email field before it hits your backend. Most Rust UI frameworks leave you to figure this out alone: write a validator function, call it on submit, render the error somewhere near the input. Repeat for every field.

This tutorial shows you how to add validation to a GPUI form using `gpui-form` and `koruma`. I'll walk through a registration form with email, phone, and URL fields, with localized error messages that update when the user switches language.

If you're looking for the conceptual overview of how these crates fit together, check out the [earlier post on form validation with koruma](/blog/form-validation-koruma-rust/). This tutorial is the hands-on version.

## What we're building

A registration form with five fields: name, email, password, phone, and website. Each field has a validation rule. Errors appear after the user clicks submit, not before. Messages come from Fluent translation files, not hard-coded strings.

Here's what the user sees:

1. A clean form with labels and optional descriptions
2. No red text until they attempt to submit
3. All errors shown at once (not one at a time)
4. Error messages in the user's chosen language

## Step 1: Define the form struct

Start with a plain struct and add the derive macros:

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

Five derives on one struct. Each one does something specific:

- `GpuiForm` generates input state management (a `FormFields` struct that tracks typed input per field)
- `Koruma` generates a `validate()` method that runs every rule and returns a typed error map
- `KorumaAllFluent` generates Fluent message keys for each validation error
- `EsFluentVariants` generates label and description enums per field
- `Default` gives you an empty form to start from

The `#[gpui_form(component(input))]` attribute tells the macro to use a standard text input for each field. The `#[koruma(...)]` attribute attaches a validation rule.

## Step 2: Add Fluent translations

Create two Fluent files. English first:

```ftl
# src/i18n/en.ftl
registration_form_label_variants-name = Full Name
registration_form_label_variants-email = Email Address
registration_form_label_variants-password = Password
registration_form_label_variants-phone = Phone Number
registration_form_label_variants-website = Website

registration_form_description_variants-email = We'll never share your email.
registration_form_description_variants-website = Optional. Your personal or company site.

registration_form_koruma_variants-name-non-empty = Name is required.
registration_form_koruma_variants-email-email = Please enter a valid email address.
registration_form_koruma_variants-password-non-empty = Password is required.
registration_form_koruma_variants-phone-phone-number = Please enter a valid phone number.
registration_form_koruma_variants-website-url = Please enter a valid URL.
```

Then Chinese:

```ftl
# src/i18n/zh-CN.ftl
registration_form_label_variants-name = 姓名
registration_form_label_variants-email = 电子邮件
registration_form_label_variants-password = 密码
registration_form_label_variants-phone = 电话号码
registration_form_label_variants-website = 网站

registration_form_koruma_variants-name-non-empty = 请输入姓名。
registration_form_koruma_variants-email-email = 请输入有效的电子邮件地址。
registration_form_koruma_variants-password-non-empty = 请输入密码。
registration_form_koruma_variants-phone-phone-number = 请输入有效的电话号码。
registration_form_koruma_variants-website-url = 请输入有效的网址。
```

The key format is `{struct_snake_case}_koruma_variants-{field}-{rule}`. The `KorumaAllFluent` derive generates these keys automatically. If you add a new field or a new rule, you just add the corresponding Fluent message. The compiler won't let you forget because the generated code references these keys by name.

When the user switches language at runtime, the error messages update on the next validation pass. You don't need to do anything extra.

## Step 3: Build the view

Now the UI part. You need a view that holds the form state, tracks whether the user has attempted submission, and renders the fields:

```rust
pub struct RegistrationPage {
    fields: <RegistrationForm as GpuiFormTrait>::FormFields,
    current_data: RegistrationForm,
    touched: bool,
    agree_terms: bool,
    submitted: bool,
}
```

The `FormFields` type is generated by the `GpuiForm` derive. It holds an `Input` state for each field. You don't define this yourself.

The render method:

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
            field()
                .label(localize_message(&RegistrationFormLabelVariants::Phone))
                .error(errors.as_ref().and_then(|e| e.get("phone")))
                .child(Input::new(&self.fields.phone_input)),
        )
        .child(
            field()
                .label(localize_message(&RegistrationFormLabelVariants::Website))
                .description(localize_message(
                    &RegistrationFormDescriptionVariants::Website,
                ))
                .error(errors.as_ref().and_then(|e| e.get("website")))
                .child(Input::new(&self.fields.website_input)),
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
                        window.push_notification("Account created!", cx);
                    }
                })),
        )
}
```

The `touched` flag is the main UX decision here. Validation runs only after the first submit attempt. Before that, `errors` is `None` and every field renders clean. After the first submit, errors appear inline.

This avoids the common problem where a form shows red validation errors the moment the page loads, before the user has typed anything. That behavior is irritating in web apps, and worse in desktop apps where the form might be the first thing on screen.

## Step 4: Wire up input changes

The generated `FormFields` struct holds `Input` state per field. You need to sync changes back to `current_data` so that `validate()` checks the latest values:

```rust
fn sync_from_fields(&mut self, cx: &App) {
    self.current_data.name = self.fields.name_input.value(cx).to_string();
    self.current_data.email = self.fields.email_input.value(cx).to_string();
    self.current_data.password = self.fields.password_input.value(cx).to_string();
    self.current_data.phone = self.fields.phone_input.value(cx).to_string();
    self.current_data.website = self.fields.website_input.value(cx).to_string();
}
```

Call this right before `validate()`. The submit handler becomes:

```rust
this.sync_from_fields(cx);
this.touched = true;
let valid = this.current_data.validate().is_ok();
```

## Composing multiple validators on one field

A phone number might need to be both non-empty and valid. Stack the validators:

```rust
#[koruma(
    NonEmptyValidation::<_>::builder(),
    PhoneNumberValidation::<_>::builder()
)]
pub phone: String,
```

`validate()` runs both rules and collects every failure. The user sees all the problems, not just the first one the code happened to hit.

## Writing a custom validator

The four built-in rules (`NonEmptyValidation`, `EmailValidation`, `PhoneNumberValidation`, `UrlValidation`) cover common cases. For anything specific to your domain, implement the `Validation` trait:

```rust
use koruma::Validation;

struct UsernameValidation;

impl Validation for UsernameValidation {
    type Value = String;

    fn validate(&self, value: &Self::Value) -> Result<(), String> {
        let v = value.trim();
        if v.len() < 3 {
            return Err("Username must be at least 3 characters.".into());
        }
        if !v.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err("Username can only contain letters, numbers, and underscores.".into());
        }
        Ok(())
    }
}
```

Then use it the same way:

```rust
#[koruma(UsernameValidation)]
pub username: String,
```

Custom validators work with Fluent too. Add the key `{struct}_koruma_variants-{field}-username` to your `.ftl` files, and the `KorumaAllFluent` derive picks it up.

For the full validator API and edge cases like async validation, see [the forms documentation](/docs/forms/).

## The tradeoffs

I want to be honest about what this approach gives up.

The derive macros generate a lot of code. If something goes wrong inside the generated impl, the compiler error points at the struct definition, not at the generated code. This can be confusing. `cargo expand` helps here: run it to see what the macros actually produce, then read the generated code to find the real error source.

The coupling between struct fields and validation rules is tight. If you need different validation depending on context (for example, a phone field that's optional during signup but required during checkout), you need separate structs. You can't conditionally apply rules at runtime. Validation logic in attributes is easy to audit and hard to forget, which is the point of this design.

Fluent key generation follows a strict naming convention. If you rename a field, you must update the `.ftl` files to match. The compiler catches this, but it's one more thing to track during refactoring.

## Where to go from here

The complete working form lives in `src/views/form_page.rs` in the [gpui-starter repo](https://github.com/freeoxide/gpui-starter). You can clone it, run `cargo run`, and see the validation in action immediately. The form page is accessible from the sidebar navigation.

For setup instructions and the full project structure, see [the getting started guide](/docs/getting-started/). The [earlier post on form validation with koruma](/blog/form-validation-koruma-rust/) covers the architecture in more depth if you want to understand the macro internals before building your own forms.
