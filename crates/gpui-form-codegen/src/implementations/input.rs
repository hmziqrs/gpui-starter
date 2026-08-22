use crate::components::*;
use proc_macro2::TokenStream;
use quote::{ToTokens as _, quote};

impl super::ComponentLayout for InputComponent {
    fn field_tokens(
        &self,
        field_structure_tokens: &mut TokenStream,
        field_base_declarations_tokens: &mut TokenStream,
    ) {
        let FieldInformation::<InputOptions> {
            options: _,
            name,
            r#type,
        } = &self.0;

        let field_name_ident = crate::component_field_name!(name);

        let field_structure_definition = quote! {
            pub #field_name_ident: ::gpui::Entity<::gpui_component::input::InputState>,
        };

        // Skip validation for String types since they always parse successfully
        let type_str = r#type.to_token_stream().to_string();
        let validation_logic = if type_str == "String" {
            quote! {}
        } else {
            quote! {
                .validate(|value, _| value.parse::<#r#type>().is_ok())
            }
        };

        let field_base_declaration = quote! {
            pub fn #field_name_ident(
                window: &mut ::gpui::Window,
                cx: &mut ::gpui::Context<'_, ::gpui_component::input::InputState>
            ) -> ::gpui_component::input::InputState {
                ::gpui_component::input::InputState::new(window, cx)#validation_logic
            }
        };

        field_structure_tokens.extend(field_structure_definition);
        field_base_declarations_tokens.extend(field_base_declaration);
    }
}
