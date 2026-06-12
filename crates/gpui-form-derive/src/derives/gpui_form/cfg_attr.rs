use syn::{Attribute, DeriveInput};
use syn_cfg_attr::{AttributeHelpers as _, ExpandedAttr};

/// Flattens `cfg_attr` attributes in a DeriveInput.
pub fn flatten_cfg_attr_in_derive_input(mut input: DeriveInput) -> DeriveInput {
    input.attrs = flatten_attrs(input.attrs);

    if let syn::Data::Struct(ref mut data_struct) = input.data {
        for field in data_struct.fields.iter_mut() {
            field.attrs = flatten_attrs(field.attrs.clone());
        }
    }

    input
}

pub fn flatten_attrs(attrs: Vec<Attribute>) -> Vec<Attribute> {
    attrs
        .flattened_attributes()
        .into_iter()
        .map(|expanded| match expanded {
            ExpandedAttr::Direct(attr) => attr,
            ExpandedAttr::Nested { attr, .. } => syn::Attribute {
                pound_token: Default::default(),
                style: syn::AttrStyle::Outer,
                bracket_token: Default::default(),
                meta: attr,
            },
        })
        .collect()
}
