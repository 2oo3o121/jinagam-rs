#![windows_subsystem = "windows"]

mod app;
mod monitor_cache;
mod overlay_window;
mod tray;

use app::App;

fn main() {
    if let Err(err) = App::new().and_then(|mut app| app.run()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
