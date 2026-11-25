use careerbrew_live::{cli, root, synthetic, FeedError};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let result = match args.as_slice() {
        [flag] if flag == "--all" => synthetic::run_all(None),
        [flag, case] if flag == "--case" && synthetic::CASES.contains(&case.as_str()) => {
            synthetic::run_all(Some(case))
        }
        [command, sub] if command == "notify" && sub == "open" => {
            root().map(|root| cli::notify_open(&root, "").to_string())
        }
        [command, sub, id] if command == "notify" && sub == "open" => {
            root().map(|root| cli::notify_open(&root, id).to_string())
        }
        [command, id] if command == "present" => careerbrew_live::root()
            .and_then(|root| careerbrew_live::notify::present(&root, id))
            .map(|_| String::new()),
        _ => Err(FeedError::value(
            "usage: live-component --all | --case <id> | present <popup-id>",
        )),
    };
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("live-component: {} ({})", error.message, error.class);
            ExitCode::from(2)
        }
    }
}
