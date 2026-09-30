//! Blockscape - an original voxel sandbox game and engine.
//!
//! Entry point: parses command line options, initializes logging and runs the
//! application loop.

mod engine;
mod render;
mod voxel;

use engine::app::{App, AppConfig};
use engine::logging::Level;

fn parse_args() -> Result<AppConfig, String> {
    let mut config = AppConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut take_value = |name: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("missing value for {name}"))
        };
        match arg.as_str() {
            "--port" => {
                let v = take_value("--port")?;
                config.port = v.parse().map_err(|_| format!("invalid port: {v}"))?;
            }
            "--width" => {
                let v = take_value("--width")?;
                config.width = v.parse().map_err(|_| format!("invalid width: {v}"))?;
            }
            "--height" => {
                let v = take_value("--height")?;
                config.height = v.parse().map_err(|_| format!("invalid height: {v}"))?;
            }
            "--seed" => {
                // Accepted now; used by world generation in milestone 6.
                let v = take_value("--seed")?;
                let _seed: u64 = v.parse().map_err(|_| format!("invalid seed: {v}"))?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(config)
}

fn print_help() {
    println!(
        "Blockscape - voxel sandbox engine\n\
         \n\
         Usage: voxel-engine [options]\n\
         \n\
         Options:\n\
         \x20 --port <n>       display server port (default 8080)\n\
         \x20 --width <n>      render width in pixels (default 480)\n\
         \x20 --height <n>     render height in pixels (default 270)\n\
         \x20 --seed <n>       world seed (used by terrain generation)\n\
         \x20 -h, --help       show this help\n\
         \n\
         Environment:\n\
         \x20 BLOCKSCAPE_LOG   log level: error|warn|info|debug|trace"
    );
}

fn main() {
    let log_level = std::env::var("BLOCKSCAPE_LOG")
        .map(|v| Level::from_env_str(&v))
        .unwrap_or(Level::Info);
    engine::logging::set_max_level(log_level);

    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("run with --help for usage");
            std::process::exit(2);
        }
    };

    log_info!("main", "blockscape starting (build: {})", if cfg!(debug_assertions) { "debug" } else { "release" });

    match App::new(config).and_then(|app| app.run()) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        }
    }
}
