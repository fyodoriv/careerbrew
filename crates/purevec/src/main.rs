//! `careerbrew-purevec <set> <vectors-file>`: prints the set's results as one
//! JSON line. `careerbrew-purevec --all` prints the registered set names.
//! Exit 2 on a usage error or an unknown set.
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--all"] {
        println!("{}", careerbrew_purevec::all());
        return ExitCode::SUCCESS;
    }
    let [set, file] = args.as_slice() else {
        eprintln!("usage: careerbrew-purevec <set> <vectors-file> | --all");
        return ExitCode::from(2);
    };
    let fixture = match std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(fixture) => fixture,
        None => {
            eprintln!("careerbrew-purevec: cannot read the vectors file");
            return ExitCode::from(2);
        }
    };
    match careerbrew_purevec::run(set, &fixture) {
        Ok(results) => {
            println!("{results}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("careerbrew-purevec: {reason}");
            ExitCode::from(2)
        }
    }
}
