#![allow(unused)]

use clap::{CommandFactory, Parser};

mod manifest;
mod cli;

fn main() {
    let args = cli::Cli::parse();
}
