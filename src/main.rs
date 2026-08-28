//! Command-line entry point: the `uid` binary.
//!
//! ```text
//! uid            # print a new ULID
//! uid ulid       # print a new ULID
//! uid uuid7      # print a new UUID version 7
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let kind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ulid".to_string());
    let id = match kind.as_str() {
        "ulid" => uid::ulid(),
        "uuid7" => uid::uuid7().to_string(),
        other => {
            eprintln!("unknown id kind {other:?}; choose from: ulid, uuid7");
            return ExitCode::from(2);
        }
    };
    println!("{id}");
    ExitCode::SUCCESS
}
