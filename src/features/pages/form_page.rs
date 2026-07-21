use es_fluent::EsFluentVariants;
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    form::{field, v_form},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_form::GpuiForm;
use koruma::{Koruma, KorumaAllFluent};
use koruma_collection::{
    collection::NonEmptyValidation,
    format::{EmailValidation, PhoneNumberValidation, UrlValidation},
};

// ---------------------------------------------------------------------------
// Form model with derive macros
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Form page
// ---------------------------------------------------------------------------

/// The five input fields of [`RegistrationForm`].
///
/// Centralizing field identity here means adding a new field is one enum
/// variant plus one `subscribe_field` / `render_field` / `on_reset_field` call,
/// instead of in-sync edits scattered across `new`, `render`, `on_reset`, and
/// `error_for`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FormField {
    Name,
    Email,
    Password,
    Phone,
    Website,
}

impl FormField {
    const ALL: [FormField; 5] = [
        FormField::Name,
        FormField::Email,
        FormField::Password,
        FormField::Phone,
        FormField::Website,
    ];

    /// Writes the latest input value into the form value holder for this field.
    fn set_on(self, holder: &mut RegistrationFormFormValueHolder, value: Option<String>) {
        match self {
            FormField::Name => holder.name = value,
            FormField::Email => holder.email = value,
            FormField::Password => holder.password = value,
            FormField::Phone => holder.phone = value,
            FormField::Website => holder.website = value,
        }
    }
}

pub struct FormPage {
    current_data: RegistrationFormFormValueHolder,
    fields: RegistrationFormFormFields,
    agree_terms: bool,
    submitted: bool,
    touched: bool,
    /// True when `current_data` has changed since the cached validation was
    /// (re)computed. Gates `validate()` so it runs at most once per edit.
    dirty: bool,
    /// Per-field localized error strings, recomputed only when `dirty && touched`.
    cached_errors: [Option<String>; 5],
    _subscriptions: Vec<Subscription>,
}

impl FormPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current_data = RegistrationFormFormValueHolder::default();

        let name_input = cx.new(|cx| RegistrationFormFormComponents::name_input(window, cx));
        let email_input = cx.new(|cx| RegistrationFormFormComponents::email_input(window, cx));
        let password_input =
            cx.new(|cx| RegistrationFormFormComponents::password_input(window, cx));
        let phone_input = cx.new(|cx| RegistrationFormFormComponents::phone_input(window, cx));
        let website_input = cx.new(|cx| RegistrationFormFormComponents::website_input(window, cx));

        // One parametrized closure replaces five near-identical per-field blocks.
        let _subscriptions = vec![
            Self::subscribe_field(FormField::Name, &name_input, cx),
            Self::subscribe_field(FormField::Email, &email_input, cx),
            Self::subscribe_field(FormField::Password, &password_input, cx),
            Self::subscribe_field(FormField::Phone, &phone_input, cx),
            Self::subscribe_field(FormField::Website, &website_input, cx),
        ];

        Self {
            current_data,
            fields: RegistrationFormFormFields {
                name_input,
                email_input,
                password_input,
                phone_input,
                website_input,
            },
            agree_terms: false,
            submitted: false,
            touched: false,
            dirty: false,
            cached_errors: Default::default(),
            _subscriptions,
        }
    }

    /// Subscribes to an input's `InputEvent::Change` and mirrors its value into
    /// `current_data` for `field`, marking the page dirty so the next render
    /// re-validates. One closure parametrized by the field enum replaces five
    /// near-identical per-field closures.
    fn subscribe_field(
        field: FormField,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe(
            input,
            move |this: &mut FormPage, state: Entity<InputState>, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = state.read(cx).value();
                    let value = if text.is_empty() {
                        None
                    } else {
                        Some(text.to_string())
                    };
                    field.set_on(&mut this.current_data, value);
                    this.dirty = true;
                }
            },
        )
    }

    fn field_input(&self, field: FormField) -> &Entity<InputState> {
        match field {
            FormField::Name => &self.fields.name_input,
            FormField::Email => &self.fields.email_input,
            FormField::Password => &self.fields.password_input,
            FormField::Phone => &self.fields.phone_input,
            FormField::Website => &self.fields.website_input,
        }
    }

    fn on_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.current_data = RegistrationFormFormValueHolder::default();
        for field in FormField::ALL {
            self.on_reset_field(field, window, cx);
        }
        self.agree_terms = false;
        self.submitted = false;
        self.touched = false;
        self.dirty = false;
        self.cached_errors = Default::default();
        cx.notify();
    }

    fn on_reset_field(&self, field: FormField, window: &mut Window, cx: &mut Context<Self>) {
        self.field_input(field)
            .update(cx, |s, cx| s.set_value("", window, cx));
    }

    /// Returns the cached localized error string for `field` (rebuilt only when
    /// the page is dirty and touched — see `recompute_validation`).
    fn error_for_field(&self, field: FormField) -> Option<String> {
        self.cached_errors[field as usize].clone()
    }

    /// Recomputes `cached_errors` from `current_data.validate()` and clears the
    /// dirty flag. Called lazily from `render` (when dirty && touched) and from
    /// the submit handler, so validation no longer runs on every render.
    fn recompute_validation(&mut self) {
        if let Some(e) = self.current_data.validate().err() {
            let to_err = |msgs: Vec<String>| {
                if msgs.is_empty() {
                    None
                } else {
                    Some(msgs.join("\n"))
                }
            };
            self.cached_errors[FormField::Name as usize] = to_err(
                e.name()
                    .all()
                    .iter()
                    .map(crate::i18n::localize_message)
                    .collect::<Vec<String>>(),
            );
            self.cached_errors[FormField::Email as usize] = to_err(
                e.email()
                    .all()
                    .iter()
                    .map(crate::i18n::localize_message)
                    .collect::<Vec<String>>(),
            );
            self.cached_errors[FormField::Password as usize] = to_err(
                e.password()
                    .all()
                    .iter()
                    .map(crate::i18n::localize_message)
                    .collect::<Vec<String>>(),
            );
            self.cached_errors[FormField::Phone as usize] = to_err(
                e.phone()
                    .all()
                    .iter()
                    .map(crate::i18n::localize_message)
                    .collect::<Vec<String>>(),
            );
            self.cached_errors[FormField::Website as usize] = to_err(
                e.website()
                    .all()
                    .iter()
                    .map(crate::i18n::localize_message)
                    .collect::<Vec<String>>(),
            );
        } else {
            self.cached_errors = Default::default();
        }
        self.dirty = false;
    }

    /// Renders a single form `field()` row, keyed on the enum so the label,
    /// description, required flag, input, and cached error all stay in sync.
    fn render_field(&self, field: FormField, danger: Hsla) -> impl IntoElement + '_ {
        let label = match field {
            FormField::Name => RegistrationFormLabelVariants::Name,
            FormField::Email => RegistrationFormLabelVariants::Email,
            FormField::Password => RegistrationFormLabelVariants::Password,
            FormField::Phone => RegistrationFormLabelVariants::Phone,
            FormField::Website => RegistrationFormLabelVariants::Website,
        };
        let description = match field {
            FormField::Name => RegistrationFormDescriptionVariants::Name,
            FormField::Email => RegistrationFormDescriptionVariants::Email,
            FormField::Password => RegistrationFormDescriptionVariants::Password,
            FormField::Phone => RegistrationFormDescriptionVariants::Phone,
            FormField::Website => RegistrationFormDescriptionVariants::Website,
        };
        let required = !matches!(field, FormField::Website);
        let error = self.error_for_field(field);
        let description_text = crate::i18n::localize_message(&description);
        let input = self.field_input(field);

        field()
            .label(crate::i18n::localize_message(&label))
            .required(required)
            .description_fn(move |_, _| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().child(description_text.clone()))
                    .when_some(error.clone(), |el, err| {
                        el.child(div().text_color(danger).text_xs().child(err))
                    })
            })
            .child(Input::new(input))
    }
}

impl Render for FormPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Re-run validation only when inputs have changed since the last compute.
        if self.touched && self.dirty {
            self.recompute_validation();
        }

        let danger = cx.theme().danger;

        v_flex()
            .min_h_full()
            .p_6()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child(crate::i18n::localize("form_page_title", None)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::i18n::localize("form_page_subtitle", None)),
            )
            .when(self.submitted, |this| {
                this.child(
                    div()
                        .p_3()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().success.opacity(0.1))
                        .border_1()
                        .border_color(cx.theme().success)
                        .text_color(cx.theme().success)
                        .child(crate::i18n::localize("form_page_success", None)),
                )
            })
            .child(
                v_form()
                    .label_width(px(160.))
                    .child(self.render_field(FormField::Name, danger))
                    .child(self.render_field(FormField::Email, danger))
                    .child(self.render_field(FormField::Password, danger))
                    .child(self.render_field(FormField::Phone, danger))
                    .child(self.render_field(FormField::Website, danger))
                    // Terms
                    .child(
                        field().label_indent(false).child(
                            Checkbox::new("agree-terms")
                                .label(crate::i18n::localize("form_agree_terms", None))
                                .checked(self.agree_terms)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.agree_terms = *checked;
                                    cx.notify();
                                })),
                        ),
                    )
                    // Actions
                    .child(
                        field().label_indent(false).child(
                            h_flex()
                                .gap_3()
                                .pt_2()
                                .child(
                                    Button::new("submit")
                                        .primary()
                                        .label(crate::i18n::localize("form_submit", None))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.touched = true;
                                            this.recompute_validation();
                                            let valid =
                                                this.cached_errors.iter().all(|e| e.is_none());
                                            if valid && this.agree_terms {
                                                this.submitted = true;
                                                window.push_notification(
                                                    crate::i18n::localize(
                                                        "form_notification_submitted",
                                                        None,
                                                    ),
                                                    cx,
                                                );
                                            } else if valid && !this.agree_terms {
                                                window.push_notification(
                                                    crate::i18n::localize(
                                                        "form_notification_agree_terms",
                                                        None,
                                                    ),
                                                    cx,
                                                );
                                            } else {
                                                window.push_notification(
                                                    crate::i18n::localize(
                                                        "form_notification_fix_errors",
                                                        None,
                                                    ),
                                                    cx,
                                                );
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("reset")
                                        .ghost()
                                        .label(crate::i18n::localize("form_reset", None))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.on_reset(window, cx);
                                        })),
                                ),
                        ),
                    ),
            )
    }
}
