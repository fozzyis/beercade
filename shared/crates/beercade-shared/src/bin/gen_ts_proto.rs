use beercade_shared::proto_ts::{generate_typescript, proto_ts_path};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str);

    let target = proto_ts_path();
    let expected = generate_typescript();

    match mode {
        Some("--write") => {
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("gen_ts_proto: failed to create {}: {e}", parent.display());
                    return ExitCode::from(1);
                }
            }
            if let Err(e) = std::fs::write(&target, expected.as_bytes()) {
                eprintln!("gen_ts_proto: failed to write {}: {e}", target.display());
                return ExitCode::from(1);
            }
            println!("Wrote {}", target.display());
            ExitCode::SUCCESS
        }
        Some("--check") => match std::fs::read_to_string(&target) {
            Ok(on_disk) if on_disk == expected => {
                println!("OK: {} is up to date.", target.display());
                ExitCode::SUCCESS
            }
            Ok(_) => {
                eprintln!(
                    "DRIFT: {} does not match beercade-shared wire models. Run `cargo run -p beercade-shared --bin gen_ts_proto -- --write`.",
                    target.display()
                );
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("MISSING: failed to read {}: {e}", target.display());
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("Usage: gen_ts_proto <--write|--check>");
            ExitCode::from(2)
        }
    }
}
