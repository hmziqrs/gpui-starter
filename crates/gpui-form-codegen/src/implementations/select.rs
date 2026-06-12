use crate::components::*;
use proc_macro2::TokenStream;
use quote::quote;

impl super::ComponentLayout for SelectComponent {
    fn field_tokens(
        &self,
        field_structure_tokens: &mut TokenStream,
        field_base_declarations_tokens: &mut TokenStream,
    ) {
        let FieldInformation::<SelectOptions> {
            options,
            name,
            r#type,
        } = &self.0;

        let field_name_ident = crate::component_field_name!(name);

        let vec_type = if options.behaviour.searchable {
            quote! { ::gpui_component::select::SearchableVec }
        } else {
            quote! { Vec }
        };

        let state_type = quote! {
          ::gpui_component::select::SelectState<#vec_type<#r#type>>
        };

        let field_structure_definition = quote! {
            pub #field_name_ident: ::gpui::Entity<#state_type>,
        };

        let index = if let Some(default_expr) = options.field_default() {
            let default_expr = default_expr.clone();
            quote! {
                {
                    let __gpui_form_default = #default_expr;
                    #r#type::iter()
                        .position(|x| x == __gpui_form_default)
                        .map(::gpui_component::IndexPath::new)
                }
            }
        } else if options.use_enum_default() {
            quote! {
              #r#type::iter()
                .position(|x| x == #r#type::default())
                .map(::gpui_component::IndexPath::new)
            }
        } else {
            quote! { None }
        };

        let field_base_declaration = if !options.behaviour.partial {
            quote! {
                pub fn #field_name_ident(
                    window: &mut ::gpui::Window,
                    cx: &mut ::gpui::Context<'_, #state_type>,
                ) -> #state_type {
                  use strum::IntoEnumIterator as _;
                  ::gpui_component::select::SelectState::new(
                      #r#type::iter().collect::<Vec<#r#type>>().into(),
                      #index,
                      window,
                      cx,
                  )
                }
            }
        } else {
            quote! {}
        };

        field_structure_tokens.extend(field_structure_definition);
        field_base_declarations_tokens.extend(field_base_declaration);
    }
}
