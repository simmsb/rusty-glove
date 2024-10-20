#![feature(concat_bytes)]

use chrono::Local;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let memory_x = include_bytes!("memory.x").as_slice();

    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    {
        let mut memory_x_f = File::create(out.join("memory.x")).unwrap();

        memory_x_f.write_all(memory_x).unwrap();
    }
    File::create(out.join("build_date.txt"))
        .unwrap()
        .write_all(Local::now().date_naive().to_string().as_bytes())
        .unwrap();
    File::create(out.join("build_attribute.txt"))
        .unwrap()
        .write_all(env::var("PROFILE").unwrap().as_bytes())
        .unwrap();

    println!("cargo:rustc-link-search={}", out.display());

    // By default, Cargo will re-run a build script whenever
    // any file in the project changes. By specifying `memory.x`
    // here, we ensure the build script is only re-run when
    // `memory.x` is changed.
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    if env::var("CARGO_FEATURE_PROBE").is_ok() {
        println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
    }
}
