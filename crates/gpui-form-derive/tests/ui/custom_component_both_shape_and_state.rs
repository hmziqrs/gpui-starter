use gpui_form_derive::GpuiForm;

#[derive(GpuiForm)]
struct Demo {
    #[gpui_form(component(custom(shape = crate::shape::DemoShape, state = crate::state::DemoState)))]
    field: String,
}

fn main() {}
