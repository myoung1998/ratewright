use std::env;
use std::process::ExitCode;

use ratewright::{shorthand_to_window, window_to_shorthand};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (mode, spec) = match args.as_slice() {
        [mode, spec] => (mode.as_str(), spec.as_str()),
        _ => {
            eprintln!("usage: ratewright <to-window|to-shorthand> <spec>");
            eprintln!("  ratewright to-window 10r/s");
            eprintln!("  ratewright to-shorthand 600/60s");
            return ExitCode::FAILURE;
        }
    };

    let result = match mode {
        "to-window" => shorthand_to_window(spec),
        "to-shorthand" => window_to_shorthand(spec),
        other => {
            eprintln!("unknown mode '{other}', expected to-window or to-shorthand");
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
