use crate::components::*;
use proc_macro2::TokenStream;
use quote::quote;

impl super::ComponentLayout for InfiniteSelectComponent {
    fn field_tokens(
        &self,
        field_structure_tokens: &mut TokenStream,
        field_base_declarations_tokens: &mut TokenStream,
    ) {
        let FieldInformation::<InfiniteSelectOptions> {
            options,
            name,
            r#type,
        } = &self.0;

        let field_name_ident = crate::component_field_name!(name);
        let searchable = options.behaviour.searchable;

        let state_type = if searchable {
            quote! { ::gpui_form::infinite_select::SearchableInfiniteSelectState<#r#type> }
        } else {
            quote! { ::gpui_form::infinite_select::InfiniteSelectState<#r#type> }
        };

        let (initial_value_binding, initial_value_expr) =
            if let Some(default_expr) = options.field_default() {
                let default_expr = default_expr.clone();
                (
                    quote! {
                        let __gpui_form_default = #default_expr;
                    },
                    quote! { __gpui_form_default },
                )
            } else {
                (
                    quote! {},
                    quote! { <#r#type as ::core::default::Default>::default() },
                )
            };

        let options_expr = if let Some(max_depth) = options.behaviour.max_depth {
            quote! {
                ::gpui_form::infinite_select::InfiniteSelectStateOptions::default()
                    .searchable(#searchable)
                    .max_depth(#max_depth)
            }
        } else {
            quote! {
                ::gpui_form::infinite_select::InfiniteSelectStateOptions::default()
                    .searchable(#searchable)
            }
        };

        let field_structure_definition = quote! {
            pub #field_name_ident: ::gpui::Entity<#state_type>,
        };

        let field_base_declaration = quote! {
            pub fn #field_name_ident(
                window: &mut ::gpui::Window,
                cx: &mut ::gpui::Context<'_, #state_type>,
            ) -> #state_type {
                #initial_value_binding
                ::gpui_form::infinite_select::InfiniteSelectState::new_with_options(
                    #initial_value_expr,
                    #options_expr,
                    window,
                    cx,
                )
            }
        };

        field_structure_tokens.extend(field_structure_definition);
        field_base_declarations_tokens.extend(field_base_declaration);
    }
}
