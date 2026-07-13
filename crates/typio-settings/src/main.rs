mod events;
mod model;
mod platform_config;
mod ui;

use std::error::Error;

use iris::{Application, Config, PaintHost};
use ui::AppState;

fn main() -> Result<(), Box<dyn Error>> {
    let mut verbose = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!("Usage: typio-settings [--verbose]");
                println!();
                println!("Graphical settings application for Typio.");
                return Ok(());
            }
            unknown => {
                return Err(format!("unknown option: {unknown}").into());
            }
        }
    }

    let mut config = Config::new("Typio Settings")?.size(960, 720);
    if verbose {
        config = config.log_raw();
    }
    let mut state = AppState::new();
    Application::run(
        config,
        move |frame, _input| state.frame(frame),
        None::<fn(PaintHost)>,
    )?;
    Ok(())
}
