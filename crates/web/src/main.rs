mod api;
mod app;
mod components;
mod icons;
mod markdown;
mod state;
mod theme;
mod util;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
