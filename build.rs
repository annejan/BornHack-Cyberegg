//! This build script copies the `memory.x` file from the crate root into
//! a directory where the linker can always find it at build time.
//! For many projects this is optional, as the linker always searches the
//! project root directory -- wherever `Cargo.toml` is. However, if you
//! are using a workspace or have a more complicated build setup, this
//! build script becomes required. Additionally, by requesting that
//! Cargo re-run the build script whenever `memory.x` is changed,
//! updating `memory.x` ensures a rebuild of the application with the
//! new memory settings.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let embassy = std::env::var_os("CARGO_FEATURE_EMBASSY_BASE").is_some();
    let simulator = std::env::var_os("CARGO_FEATURE_SIMULATOR").is_some();
    let hwtest = std::env::var_os("CARGO_FEATURE_HWTEST").is_some();

    let embedded_features = [("embassy-base", embassy), ("hwtest", hwtest)]
        .iter()
        .filter(|(_, on)| *on)
        .count();
    if simulator && embedded_features > 0 {
        panic!("Feature `simulator` is mutually exclusive with embedded firmware features.");
    }
    if embedded_features > 1 {
        panic!("Features `embassy-base` and `hwtest` are mutually exclusive.");
    }

    let memory_script: Option<&[u8]> = if hwtest {
        Some(include_bytes!("memory-hwtest.x"))
    } else if embassy {
        Some(include_bytes!("memory-fw.x"))
    } else {
        None
    };

    if let Some(script) = memory_script {
        // Put `memory.x` in our output directory and ensure it's
        // on the linker search path.  The source file is named
        // differently from `memory.x` on purpose: GNU ld's `INCLUDE`
        // searches the current working directory (the project root)
        // before `-L` paths, so a `memory.x` in the project root
        // would shadow the OUT_DIR copy and every variant would link
        // with the same memory map.
        let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
        File::create(out.join("memory.x"))
            .unwrap()
            .write_all(script)
            .unwrap();
        println!("cargo:rustc-link-search={}", out.display());

        // Re-run when either memory layout changes.
        println!("cargo:rerun-if-changed=memory-fw.x");
        println!("cargo:rerun-if-changed=memory-hwtest.x");

        println!("cargo:rustc-link-arg-bins=--nmagic");
        println!("cargo:rustc-link-arg-bins=-Tlink.x");
        println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    }

    if simulator {
        // Placeholder for simulator-specific build steps.
    }
}
