//! Host-only helper: strip `-Z json-target-spec` and invoke the real rustc.
//! Build once: `rustc tools/rustc-filter.rs -o tools/rustc-filter.exe`

use std::env;
use std::process::Command;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-Z" && i + 1 < args.len() && args[i + 1] == "json-target-spec" {
            args.remove(i);
            args.remove(i);
            continue;
        }
        i += 1;
    }

    let rustc = env::var("KERNEL_RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let status = Command::new(&rustc)
        .args(&args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("rustc-filter: failed to run {rustc}: {e}");
            std::process::exit(1);
        });

    std::process::exit(status.code().unwrap_or(1));
}
