use proc_macro2::TokenStream;

pub mod checkbox;
pub mod custom;
pub mod date_picker;
pub mod file_picker;
pub mod infinite_select;
pub mod input;
pub mod number_input;
pub mod select;
pub mod switch;

pub trait ComponentLayout {
    fn field_tokens(
        &self,
        field_structure_tokens: &mut TokenStream,
        field_base_declarations_tokens: &mut TokenStream,
    );
}
