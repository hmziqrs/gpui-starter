use gpui_form_derive::GpuiForm;

#[derive(GpuiForm)]
struct Demo {
    #[gpui_form(component(custom(component = crate::widgets::TagsInput)))]
    field: String,
}

fn main() {}
