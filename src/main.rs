use clap::Parser;

mod cli;
mod fsio;
mod git;
mod link;
mod lockfile;
mod manifest;
mod pipeline;
mod resolve;
mod target;
mod tests;

fn main() {
    let args = cli::Cli::parse();

    if args.no_color {
        colored::control::set_override(false);
        pipeline::disable_color();
    }

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

    let (manifest, root) = pipeline::load_manifest_or_exit(args.manifest_path.as_deref());

    match args.command {
        cli::Commands::Init(_) | cli::Commands::New(_) => unreachable!(),

        cli::Commands::Check => {
            let _ = pipeline::cmd_check(&manifest, &root, false);
        }

        cli::Commands::Update => pipeline::cmd_update(&manifest, &root),
        cli::Commands::DepsMap => pipeline::cmd_deps_map(&manifest, &root),

        cli::Commands::Build(args) => {
            let _ = pipeline::cmd_build(args, &manifest, &root);
        }
        cli::Commands::Run(args) => pipeline::cmd_run(args, &manifest, &root),
        cli::Commands::Add(args) => pipeline::cmd_add(args, &manifest, &root),
        cli::Commands::Remove(args) => pipeline::cmd_remove(args, &manifest, &root),
        cli::Commands::Clean(args) => pipeline::cmd_clean(args, &root),
        cli::Commands::Test => pipeline::cmd_test(&manifest, &root),
    }
}
