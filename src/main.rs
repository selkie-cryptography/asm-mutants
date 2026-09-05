//! `cargo asm-mutants`: mutation testing for inline assembly.
//!
//! cargo-mutants rewrites Rust syntax and never looks inside an `asm!`
//! template string, so a hand-scheduled kernel gets no mutants at all. This
//! tool mutates the template lines themselves, one instruction at a time,
//! rebuilds, and runs the tests per mutant. `global_asm!(include_str!("x.s"))`
//! files are mutated the same way. The command line, configuration file,
//! output directory, and exit codes follow cargo-mutants.

#![deny(unsafe_code)]
#![warn(unused_qualifications)]

mod arch;
mod build_dir;
mod cargo;
mod cli;
mod config;
mod console;
mod in_diff;
mod instruction;
mod lab;
mod mutant;
mod operators;
mod options;
mod outcome;
mod output;
mod source;
mod timestamp;

use std::process::exit;

use crate::{cli::Cli, lab::Lab, options::Options};

fn main() {
    let cli = Cli::from_env();
    let result = Options::new(cli).and_then(|options| Lab::new(options)?.run());
    match result {
        Ok(code) => exit(code.into()),
        Err(error) => {
            eprintln!("error: {error:#}");
            exit(1);
        }
    }
}
