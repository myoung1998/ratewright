use std::env;
use std::process::ExitCode;

use ratewright::{
    equivalent, format_shorthand, gcra_delay_variation_tolerance, gcra_emission_interval,
    parse_rate, parse_token_bucket, shorthand_to_window, window_to_shorthand,
};

fn print_usage() {
    eprintln!("usage: ratewright <to-window|to-shorthand|gcra|check> <spec> [spec]");
    eprintln!("  ratewright to-window 10r/s");
    eprintln!("  ratewright to-shorthand 600/60s");
    eprintln!("  ratewright gcra \"10r/s;burst=20\"");
    eprintln!("  ratewright check 10r/s 600/60s");
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some((mode, rest)) = args.split_first() else {
        print_usage();
        return ExitCode::FAILURE;
    };

    match (mode.as_str(), rest) {
        ("to-window", [spec]) => print_result(shorthand_to_window(spec)),
        ("to-shorthand", [spec]) => print_result(window_to_shorthand(spec)),
        ("gcra", [spec]) => match parse_token_bucket(spec) {
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
        ("check", [a, b]) => run_check(a, b),
        ("to-window" | "to-shorthand" | "gcra" | "check", _) => {
            print_usage();
            ExitCode::FAILURE
        }
        (other, _) => {
            eprintln!("unknown mode '{other}', expected to-window, to-shorthand, gcra, or check");
            ExitCode::FAILURE
        }
    }
}

fn run_check(a: &str, b: &str) -> ExitCode {
    let rate_a = match parse_rate(a) {
        Ok(rate) => rate,
        Err(err) => {
            eprintln!("{a}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let rate_b = match parse_rate(b) {
        Ok(rate) => rate,
        Err(err) => {
            eprintln!("{b}: {err}");
            return ExitCode::FAILURE;
        }
    };

    if equivalent(&rate_a, &rate_b) {
        println!("equivalent ({})", format_shorthand(&rate_a));
        ExitCode::SUCCESS
    } else {
        println!(
            "not equivalent ({} vs {})",
            format_shorthand(&rate_a),
            format_shorthand(&rate_b)
        );
        ExitCode::FAILURE
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
