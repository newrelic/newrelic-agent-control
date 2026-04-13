//! Test binary for integration tests.
//!
//! This example binary is used by integration tests to verify self-replacement behavior.
//! It has two modes:
//!
//! 1. **Default mode**: Reports its own content hash (prints `HASH:xxxxxxxxxxxxxxxx`)
//!    - Used to verify that the binary changed after replacement
//!    - Each modified copy will have a different hash
//!
//! 2. **Replace mode**: Performs self-replacement when invoked with `--replace <new_binary>`
//!    - Replaces itself with the binary at the given path
//!    - Prints `REPLACEMENT_SUCCESS` on success
//!    - Prints `REPLACEMENT_FAILED: <error>` on failure
//!
//! ```

#[cfg(unix)]
use self_replacer::{SelfReplacer, UnixSelfReplacer};

#[cfg(windows)]
use self_replacer::{SelfReplacer, WindowsSelfReplacer};

use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};

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
        // Report hash of binary content as identifier
        // This allows us to verify the binary changed after replacement
        let exe_path = env::current_exe().unwrap();
        let content = fs::read(&exe_path).unwrap();

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        let hash = hasher.finish();

        println!("HASH:{:016x}", hash);
    }
}
