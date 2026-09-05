use std::env;
use std::process::ExitCode;

use ratewright::{
    gcra_delay_variation_tolerance, gcra_emission_interval, parse_token_bucket,
    shorthand_to_window, window_to_shorthand,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (mode, spec) = match args.as_slice() {
        [mode, spec] => (mode.as_str(), spec.as_str()),
        _ => {
            eprintln!("usage: ratewright <to-window|to-shorthand|gcra> <spec>");
            eprintln!("  ratewright to-window 10r/s");
            eprintln!("  ratewright to-shorthand 600/60s");
            eprintln!("  ratewright gcra \"10r/s;burst=20\"");
            return ExitCode::FAILURE;
        }
    };

    match mode {
        "to-window" => print_result(shorthand_to_window(spec)),
        "to-shorthand" => print_result(window_to_shorthand(spec)),
        "gcra" => match parse_token_bucket(spec) {
            Ok(bucket) => {
                let emission_interval = gcra_emission_interval(&bucket.rate);
                let delay_variation_tolerance = gcra_delay_variation_tolerance(&bucket);
                println!("emission_interval={emission_interval}s delay_variation_tolerance={delay_variation_tolerance}s");
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("{err}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("unknown mode '{other}', expected to-window, to-shorthand, or gcra");
            ExitCode::FAILURE
        }
    }
}

fn print_result(result: Result<String, ratewright::ParseError>) -> ExitCode {
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
