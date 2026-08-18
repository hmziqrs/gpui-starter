use koruma_derive_core::ValidatorAttr;
use proc_macro2::TokenStream;
use quote::quote;

/// Convert a ValidatorAttr to a TokenStream for generating koruma attribute.
/// This produces normalized builder-chain tokens like
/// `ValidatorPath::<Type>::builder().arg1(val1).arg2(val2)`.
pub fn validator_attr_to_tokens(validator: &ValidatorAttr) -> TokenStream {
    let path = &validator.validator;

    let type_params = if validator.infer_type {
        quote! { ::<_> }
    } else if let Some(explicit_ty) = &validator.explicit_type {
        quote! { ::<#explicit_ty> }
    } else {
        quote! {}
    };

    let setter_calls = validator.setter_calls();
    let builder_calls = setter_calls.iter().map(|method| {
        let method_name = &method.method;
        let args = &method.args;
        quote! { .#method_name(#(#args),*) }
    });

    quote! { #path #type_params ::builder() #(#builder_calls)* }
}
