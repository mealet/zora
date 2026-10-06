#![allow(unused)]

use clap::{CommandFactory, Parser};

mod cli;
mod manifest;
mod pipeline;

fn main() {
    let _ = pipeline::load_manifest_or_exit();
    let args = cli::Cli::parse();
}
