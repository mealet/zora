#![allow(unused)]

use clap::{CommandFactory, Parser};

mod cli;
mod link;
mod lockfile;
mod manifest;
mod pipeline;

fn main() {
    let args = cli::Cli::parse();

    // initialization commands
    match &args.command {
        cli::Commands::Init(args) => {
            pipeline::cmd_init(args);
            return;
        }
        cli::Commands::New(args) => {
            pipeline::cmd_new(args);
            return;
        }
        _ => {}
    }

    // commands with manifest required

    let (manifest, root) = pipeline::load_manifest_or_exit();

    match args.command {
        cli::Commands::Init(_) | cli::Commands::New(_) => unreachable!(),

        cli::Commands::Check => pipeline::cmd_check(&manifest, &root, false),
        cli::Commands::Build(args) => pipeline::cmd_build(args, &manifest, &root),
        cli::Commands::Run(args) => pipeline::cmd_run(args, &manifest, &root),
        cli::Commands::Add(args) => pipeline::cmd_add(args, &manifest, &root),
        cli::Commands::Remove(args) => pipeline::cmd_remove(args, &manifest, &root),
        cli::Commands::Test => pipeline::cmd_test(&manifest, &root),
    }
}
