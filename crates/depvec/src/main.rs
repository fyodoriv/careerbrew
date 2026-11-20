//! `careerbrew-depvec <set> <vectors-file>`: prints the set's results as one
//! JSON line. Exit 2 on a usage error or an unknown set.
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [set, file] = args.as_slice() else {
        eprintln!("usage: careerbrew-depvec <set> <vectors-file>");
        return ExitCode::from(2);
    };
    let fixture = match std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(fixture) => fixture,
        None => {
            eprintln!("careerbrew-depvec: cannot read the vectors file");
            return ExitCode::from(2);
        }
    };
    match careerbrew_depvec::run(set, &fixture) {
        Ok(results) => {
            println!("{results}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("careerbrew-depvec: {reason}");
            ExitCode::from(2)
        }
    }
}
