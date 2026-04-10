#[cfg(unix)]
use self_replacer::{SelfReplacer, UnixSelfReplacer};

#[cfg(windows)]
use self_replacer::{SelfReplacer, WindowsSelfReplacer};

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && args[1] == "--replace" {
        // Perform self-replacement
        let new_binary_path = &args[2];

        #[cfg(unix)]
        let result = UnixSelfReplacer::self_replace(new_binary_path);

        #[cfg(windows)]
        let result = WindowsSelfReplacer::self_replace(new_binary_path);

        match result {
            Ok(()) => {
                println!("REPLACEMENT_SUCCESS");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("REPLACEMENT_FAILED: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        // Just print version
        println!("VERSION:{}", env!("CARGO_PKG_VERSION"));
    }
}
