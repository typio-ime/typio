//! typio — unified Wayland input-method daemon (Rust).
//!
//! This is the shipping daemon entry point. It delegates the full lifecycle
//! to [`typio_daemon::app::App`].

use std::process::ExitCode;

use typio_daemon::app::App;
use typio_daemon::diagnostics;

fn main() -> ExitCode {
    let mut app = match App::from_env() {
        Ok(a) => a,
        Err(e) => {
            // Logging is not initialized yet (verbosity comes from the parsed
            // args), so this fatal CLI error goes straight to stderr.
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    diagnostics::init_logging(app.verbosity());

    if let Err(e) = app.init() {
        tracing::error!(target: "typio.startup", error = %e, "init failed");
        return ExitCode::from(1);
    }

    let exit_code = app.run();
    app.shutdown();
    let code = app.finish(exit_code);
    ExitCode::from(code as u8)
}
