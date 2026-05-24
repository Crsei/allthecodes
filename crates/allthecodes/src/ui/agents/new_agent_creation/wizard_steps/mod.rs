//! Individual create-agent wizard step renderers.
pub mod color_step;
pub mod confirm_step;
pub mod confirm_step_wrapper;
pub mod description_step;
pub mod generate_step;
pub mod location_step;
pub mod memory_step;
pub mod method_step;
pub mod model_step;
pub mod prompt_step;
pub mod tools_step;
pub mod type_step;

pub fn render_step_frame(title: &str, body: impl AsRef<str>, complete: bool) -> String {
    format!(
        "{}\nstatus: {}\n{}",
        title,
        if complete { "complete" } else { "needs input" },
        body.as_ref()
    )
}
