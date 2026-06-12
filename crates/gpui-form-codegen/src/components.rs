use darling::{Error as DarlingError, FromMeta};
use gpui_form_schema::components::{ComponentKind, NumberInputKind};
use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote};

use crate::implementations::ComponentLayout as _;

pub trait ComponentOption {}

pub trait ComponentDefinition {
    fn component_kind() -> ComponentKind;

    fn component_name() -> &'static str {
        Self::component_kind().component_name()
    }
}

pub struct FieldInformation<T: ComponentOption> {
    pub options: T,
    pub name: String,
    pub r#type: syn::Type,
}

impl<T: ComponentOption> FieldInformation<T> {
    pub fn new(options: T, name: String, r#type: syn::Type) -> Self {
        Self {
            options,
            name,
            r#type,
        }
    }
}

pub struct GeneratedFieldLayout {
    pub field_structure_tokens: TokenStream,
    pub field_base_declarations_tokens: TokenStream,
    pub wrap_in_option: bool,
}

macro_rules! impl_component_option {
    ($($option:ty),+ $(,)?) => {
        $(impl ComponentOption for $option {})+
    };
}

macro_rules! define_component_definition {
    ($component:ident, $options:ty, $kind:ident) => {
        pub struct $component(pub FieldInformation<$options>);

        impl ComponentDefinition for $component {
            fn component_kind() -> ComponentKind {
                ComponentKind::$kind
            }
        }
    };
}

#[derive(Clone, Debug, Default, Eq, FromMeta, PartialEq)]
pub struct BehaviourSelectOptions {
    #[darling(default)]
    pub partial: bool,
    #[darling(default)]
    pub searchable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SelectOptions {
    pub behaviour: BehaviourSelectOptions,
    /// Field-level default value expression (e.g., `EnumCountry::France` or
    /// `preferred_country()`).
    /// This is set by the derive macro when the field has a `default = ...` attribute.
    field_default: Option<syn::Expr>,
}

impl FromMeta for SelectOptions {
    fn from_word() -> darling::Result<Self> {
        Ok(Self::default())
    }

    fn from_list(items: &[darling::ast::NestedMeta]) -> darling::Result<Self> {
        let behaviour = BehaviourSelectOptions::from_list(items)?;
        Ok(Self {
            behaviour,
            field_default: None,
        })
    }
}

impl SelectOptions {
    pub fn with_field_default(mut self, default: Option<syn::Expr>) -> Self {
        self.field_default = default;
        self
    }

    pub fn field_default(&self) -> Option<&syn::Expr> {
        self.field_default.as_ref()
    }

    pub fn use_enum_default(&self) -> bool {
        self.field_default.is_none()
    }
}

#[derive(Clone, Debug, Default, FromMeta)]
pub struct InputOptions;

#[derive(Clone, Debug, Default)]
pub struct NumberInputOptions {
    /// For custom numeric types (like Decimal), specify a standard numeric type
    /// for validation purposes (e.g., f64, i32, u64)
    pub r#as: Option<syn::Ident>,
}

impl FromMeta for NumberInputOptions {
    fn from_word() -> darling::Result<Self> {
        Ok(Self::default())
    }

    fn from_list(items: &[darling::ast::NestedMeta]) -> darling::Result<Self> {
        let mut r#as = None;

        for item in items {
            if let darling::ast::NestedMeta::Meta(syn::Meta::NameValue(nv)) = item
                && nv.path.is_ident("as")
                && let syn::Expr::Path(expr_path) = &nv.value
                && let Some(ident) = expr_path.path.get_ident()
            {
                r#as = Some(ident.clone());
            }
        }

        Ok(Self { r#as })
    }
}

#[derive(Clone, Debug, FromMeta)]
pub struct CheckboxOptions;

#[derive(Clone, Debug, FromMeta)]
pub struct SwitchOptions;

#[derive(Clone, Debug, FromMeta)]
pub struct DatePickerOptions;

#[derive(Clone, Debug, FromMeta)]
pub struct FilePickerOptions;

fn default_custom_wraps_in_option() -> bool {
    true
}

#[derive(Clone, Debug)]
pub struct CustomOptions {
    /// Path to a type implementing `gpui_form::custom::CustomComponentShape`.
    pub shape: syn::Path,
    /// UI component type path (e.g. `TagsInput`).
    /// When provided, the prototyping code generator emits `Component::new(&entity)`.
    pub component: Option<syn::Path>,
    /// Whether the value holder should store this field as `Option<T>`.
    /// Defaults to `true`.
    pub wraps_in_option: bool,
    /// Whether prototyping code should wire this custom component through
    /// `CustomComponentValueAdapter`.
    pub value_binding: bool,
}

#[derive(Debug, Default, FromMeta)]
struct CustomOptionsMeta {
    #[darling(default)]
    shape: Option<syn::Path>,
    #[darling(default)]
    state: Option<syn::Path>,
    #[darling(default)]
    component: Option<syn::Path>,
    #[darling(default = "default_custom_wraps_in_option")]
    wraps_in_option: bool,
    #[darling(default)]
    value_binding: bool,
}

impl CustomOptions {
    fn from_meta(meta: CustomOptionsMeta) -> darling::Result<Self> {
        let CustomOptionsMeta {
            shape,
            state,
            component,
            wraps_in_option,
            value_binding,
        } = meta;

        let shape = match (shape, state) {
            (Some(shape), None) | (None, Some(shape)) => shape,
            (Some(_), Some(_)) => {
                return Err(DarlingError::custom(
                    "custom component may specify only one of `shape` or `state`",
                ));
            },
            (None, None) => {
                return Err(DarlingError::custom(
                    "custom component requires `shape = ...` or `state = ...`",
                ));
            },
        };

        Ok(Self {
            shape,
            component,
            wraps_in_option,
            value_binding,
        })
    }
}

impl FromMeta for CustomOptions {
    fn from_word() -> darling::Result<Self> {
        Err(DarlingError::custom(
            "custom component requires `shape = ...` or `state = ...`",
        ))
    }

    fn from_list(items: &[darling::ast::NestedMeta]) -> darling::Result<Self> {
        let meta = CustomOptionsMeta::from_list(items)?;
        Self::from_meta(meta)
    }
}

#[derive(Clone, Debug, Default, Eq, FromMeta, PartialEq)]
pub struct BehaviourInfiniteSelectOptions {
    #[darling(default)]
    pub searchable: bool,
    #[darling(default)]
    pub max_depth: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct InfiniteSelectOptions {
    pub behaviour: BehaviourInfiniteSelectOptions,
    /// Field-level default value expression (e.g., `Country::France(...)` or
    /// `default_country()`).
    /// This is set by the derive macro when the field has a `default = ...` attribute.
    field_default: Option<syn::Expr>,
}

impl FromMeta for InfiniteSelectOptions {
    fn from_word() -> darling::Result<Self> {
        Ok(Self::default())
    }

    fn from_list(items: &[darling::ast::NestedMeta]) -> darling::Result<Self> {
        let behaviour = BehaviourInfiniteSelectOptions::from_list(items)?;
        Ok(Self {
            behaviour,
            field_default: None,
        })
    }
}

impl InfiniteSelectOptions {
    pub fn with_field_default(mut self, default: Option<syn::Expr>) -> Self {
        self.field_default = default;
        self
    }

    pub fn field_default(&self) -> Option<&syn::Expr> {
        self.field_default.as_ref()
    }

    pub fn use_enum_default(&self) -> bool {
        self.field_default.is_none()
    }
}

impl_component_option!(
    BehaviourSelectOptions,
    SelectOptions,
    InputOptions,
    NumberInputOptions,
    CheckboxOptions,
    SwitchOptions,
    DatePickerOptions,
    FilePickerOptions,
    CustomOptions,
    BehaviourInfiniteSelectOptions,
    InfiniteSelectOptions,
);

#[derive(Clone, Debug, FromMeta)]
#[darling(rename_all = "snake_case")]
pub enum Components {
    Input,
    NumberInput(NumberInputOptions),
    Checkbox,
    Switch,
    Select(SelectOptions),
    InfiniteSelect(InfiniteSelectOptions),
    Custom(CustomOptions),
    DatePicker,
    FilePicker,
}

define_component_definition!(InputComponent, InputOptions, Input);
define_component_definition!(NumberInputComponent, NumberInputOptions, NumberInput);
define_component_definition!(CheckboxComponent, CheckboxOptions, Checkbox);
define_component_definition!(SwitchComponent, SwitchOptions, Switch);
define_component_definition!(SelectComponent, SelectOptions, Select);
define_component_definition!(
    InfiniteSelectComponent,
    InfiniteSelectOptions,
    InfiniteSelect
);
define_component_definition!(CustomComponent, CustomOptions, Custom);
define_component_definition!(DatePickerComponent, DatePickerOptions, DatePicker);
define_component_definition!(FilePickerComponent, FilePickerOptions, FilePicker);

fn number_input_kind(type_str: &str) -> NumberInputKind {
    if type_str.starts_with('f') {
        NumberInputKind::Float
    } else if type_str.starts_with('u') {
        NumberInputKind::UnsignedInteger
    } else if type_str.starts_with('i') {
        NumberInputKind::SignedInteger
    } else {
        NumberInputKind::Custom
    }
}

fn number_input_behaviour_tokens(
    options: &NumberInputOptions,
    field_type: &syn::Type,
) -> TokenStream {
    let type_str = options
        .r#as
        .as_ref()
        .map(|ty| ty.to_string())
        .unwrap_or_else(|| field_type.to_token_stream().to_string());

    let kind_tokens = match number_input_kind(&type_str) {
        NumberInputKind::Float => {
            quote! { ::gpui_form::schema::components::NumberInputKind::Float }
        },
        NumberInputKind::SignedInteger => {
            quote! { ::gpui_form::schema::components::NumberInputKind::SignedInteger }
        },
        NumberInputKind::UnsignedInteger => {
            quote! { ::gpui_form::schema::components::NumberInputKind::UnsignedInteger }
        },
        NumberInputKind::Custom => {
            quote! { ::gpui_form::schema::components::NumberInputKind::Custom }
        },
    };
    let validation_type = options.r#as.as_ref().map(|value| value.to_string());
    let validation_type = match validation_type {
        Some(value) => quote! { Some(#value) },
        None => quote! { None },
    };

    quote! {
        ::gpui_form::schema::components::NumberInputBehaviour {
            validation_type: #validation_type,
            kind: #kind_tokens,
        }
    }
}

impl Components {
    pub const fn kind(&self) -> ComponentKind {
        match self {
            Self::Input => ComponentKind::Input,
            Self::NumberInput(_) => ComponentKind::NumberInput,
            Self::Checkbox => ComponentKind::Checkbox,
            Self::Switch => ComponentKind::Switch,
            Self::Select(_) => ComponentKind::Select,
            Self::InfiniteSelect(_) => ComponentKind::InfiniteSelect,
            Self::Custom(_) => ComponentKind::Custom,
            Self::DatePicker => ComponentKind::DatePicker,
            Self::FilePicker => ComponentKind::FilePicker,
        }
    }

    pub fn wraps_in_option(&self) -> bool {
        match self {
            Self::Custom(options) => options.wraps_in_option,
            _ => self.kind().default_wraps_in_option(),
        }
    }

    pub fn generate_field_layout(
        &self,
        field_name: String,
        field_type: syn::Type,
        field_default: Option<syn::Expr>,
    ) -> GeneratedFieldLayout {
        let mut field_structure_tokens = TokenStream::new();
        let mut field_base_declarations_tokens = TokenStream::new();

        match self {
            Self::Input => {
                let component =
                    InputComponent(FieldInformation::new(InputOptions, field_name, field_type));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::NumberInput(options) => {
                let component = NumberInputComponent(FieldInformation::new(
                    options.clone(),
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::Checkbox => {
                let component = CheckboxComponent(FieldInformation::new(
                    CheckboxOptions,
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::Switch => {
                let component =
                    SwitchComponent(FieldInformation::new(SwitchOptions, field_name, field_type));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::Select(options) => {
                let component = SelectComponent(FieldInformation::new(
                    options.clone().with_field_default(field_default),
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::InfiniteSelect(options) => {
                let component = InfiniteSelectComponent(FieldInformation::new(
                    options.clone().with_field_default(field_default),
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::Custom(options) => {
                let component = CustomComponent(FieldInformation::new(
                    options.clone(),
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::DatePicker => {
                let component = DatePickerComponent(FieldInformation::new(
                    DatePickerOptions,
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
            Self::FilePicker => {
                let component = FilePickerComponent(FieldInformation::new(
                    FilePickerOptions,
                    field_name,
                    field_type,
                ));
                component.field_tokens(
                    &mut field_structure_tokens,
                    &mut field_base_declarations_tokens,
                );
            },
        }

        GeneratedFieldLayout {
            field_structure_tokens,
            field_base_declarations_tokens,
            wrap_in_option: self.wraps_in_option(),
        }
    }

    pub fn behaviour_tokens(&self, field_type: &syn::Type) -> TokenStream {
        match self {
            Self::Input => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::Input }
            },
            Self::NumberInput(options) => {
                let behaviour = number_input_behaviour_tokens(options, field_type);

                quote! {
                    ::gpui_form::schema::components::ComponentsBehaviour::NumberInput(
                        #behaviour
                    )
                }
            },
            Self::Checkbox => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::Checkbox }
            },
            Self::Switch => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::Switch }
            },
            Self::Select(options) => {
                let searchable = options.behaviour.searchable;
                let partial = options.behaviour.partial;
                quote! {
                    ::gpui_form::schema::components::ComponentsBehaviour::Select(
                        ::gpui_form::schema::components::SelectBehaviour {
                            searchable: #searchable,
                            partial: #partial,
                        }
                    )
                }
            },
            Self::InfiniteSelect(options) => {
                let searchable = options.behaviour.searchable;
                let max_depth = match options.behaviour.max_depth {
                    Some(depth) => quote! { Some(#depth) },
                    None => quote! { None },
                };
                quote! {
                    ::gpui_form::schema::components::ComponentsBehaviour::InfiniteSelect(
                        ::gpui_form::schema::components::InfiniteSelectBehaviour {
                            searchable: #searchable,
                            max_depth: #max_depth,
                        }
                    )
                }
            },
            Self::Custom(_) => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::Custom }
            },
            Self::DatePicker => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::DatePicker }
            },
            Self::FilePicker => {
                quote! { ::gpui_form::schema::components::ComponentsBehaviour::FilePicker }
            },
        }
    }

    pub fn custom_component_tokens(&self) -> Option<TokenStream> {
        let Self::Custom(options) = self else {
            return None;
        };

        let shape = &options.shape;
        if let Some(component) = options.component.as_ref() {
            let component_str = component.to_token_stream().to_string();
            Some(quote! { .with_custom_component(#component_str) })
        } else {
            Some(quote! {
                .with_custom_component_opt(
                    <#shape as ::gpui_form::custom::CustomComponentShape>::COMPONENT_PATH
                )
            })
        }
    }

    pub fn custom_shape_tokens(&self) -> Option<TokenStream> {
        let Self::Custom(options) = self else {
            return None;
        };

        let shape = options.shape.to_token_stream().to_string();
        Some(quote! { .with_custom_shape(#shape) })
    }

    pub fn custom_value_binding_tokens(&self) -> Option<TokenStream> {
        let Self::Custom(options) = self else {
            return None;
        };

        options
            .value_binding
            .then(|| quote! { .with_custom_value_binding(true) })
    }
}
