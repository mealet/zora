use std::fmt::Display;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colored::Colorize;

/// Main CLI parser struct
#[derive(Debug, Parser)]
#[command(author, version, about = "The Zeen package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[arg(long = "no-color", help = "Disable output coloring")]
    pub no_color: bool,

    #[arg(
        long = "manifest-path",
        value_name = "PATH",
        global = true,
        help = "Path to `Zora.toml` (defaults to searching parents)"
    )]
    pub manifest_path: Option<PathBuf>,
}

/// Subcommand enum
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Initialize project in current directory
    Init(InitArgs),

    /// Create new project by name
    New(NewArgs),

    /// Build the project
    Build(BuildArgs),

    /// Build and run the project
    Run(RunArgs),

    /// Run project's test suite
    Test,

    /// Add new dependency to the project
    Add(AddArgs),

    /// Remove dependency from the project
    Remove(RemoveArgs),

    /// Clean build artifacts and caches
    Clean(CleanArgs),

    /// Check project config and source
    Check,

    /// Update lock dependencies from manifest
    Update,

    /// Print dependencies sources map
    DepsMap,
}

// ==== Specifications Below ====

/// `zora init` args
#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long = "lib", help = "Initialize project as library")]
    pub library: bool,
}

/// `zora new` args
#[derive(Debug, Args)]
pub struct NewArgs {
    pub name: String,

    #[arg(long = "lib", help = "Initialize project as library")]
    pub library: bool,
}

#[derive(Debug, Args)]
pub struct ModeArgs {
    /// `-m --mode`
    #[arg(short, long, value_enum, default_value_t = CompilationMode::Debug, conflicts_with_all = ["release", "debug"], help="Specify compilation mode")]
    pub mode: CompilationMode,

    #[arg(long, conflicts_with_all = ["mode", "debug"], help = "Compile the project in release mode")]
    pub release: bool,

    #[arg(long, conflicts_with_all = ["mode", "release"], help = "Compile the project in debug mode")]
    pub debug: bool,
}

impl ModeArgs {
    pub fn resolve(&self) -> CompilationMode {
        if self.release {
            CompilationMode::Release
        } else if self.debug {
            CompilationMode::Debug
        } else {
            self.mode
        }
    }
}

/// `zora build` args
#[derive(Debug, Args)]
pub struct BuildArgs {
    #[command(flatten)]
    pub mode: ModeArgs,

    /// `--target`
    #[arg(
        long = "target",
        value_name = "TRIPLE",
        help = "Compilation target triple (see `zeen --targets-list`)"
    )]
    pub target: Option<String>,

    /// `--emit`
    #[arg(long, help = "Emit options")]
    pub emit: Option<CompilationOutput>,
}

#[derive(Debug, Args)]
pub struct CleanArgs {
    /// Clean cached dependencies in global database (usually `~/.zora`)
    #[arg(long)]
    pub global: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompilationMode {
    #[value(name = "Debug")]
    #[default]
    Debug,

    #[value(name = "Release")]
    Release,
}

impl std::fmt::Display for CompilationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Debug => "Debug",
                Self::Release => "Release",
            }
        )
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CompilationOutput {
    #[value(name = "BIN")]
    #[default]
    Binary,
    #[value(name = "OBJ")]
    Object,
    #[value(name = "IR")]
    EmitIR,
    #[value(name = "MIR")]
    EmitMIR,
}

impl std::fmt::Display for CompilationOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Binary => "BIN",
                Self::Object => "OBJ",
                Self::EmitIR => "IR",
                Self::EmitMIR => "MIR",
            }
        )
    }
}

/// `zora run` args
#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub mode: ModeArgs,

    #[arg(last = true, value_name = "ARGS")]
    pub program_args: Vec<String>,
}

/// `zora add` args
#[derive(Debug, Args)]
pub struct AddArgs {
    #[arg(value_name = "git-url")]
    pub url: String,

    #[arg(long, value_name = "BRANCH")]
    pub branch: Option<String>,
}

/// `zora remove` args
#[derive(Debug, Args)]
pub struct RemoveArgs {
    #[arg(value_name = "name")]
    pub name: String,
}

// ==== Helpers Functions ====

pub fn exit_with_error(message: impl Display) -> ! {
    println_error(message);
    std::process::exit(1);
}

pub fn println_error(message: impl Display) {
    eprintln!("{} {}", "[Error]:".red(), message);
}

pub fn println_warn(message: impl Display) {
    eprintln!("{} {}", "[Warn]:".yellow(), message);
}

pub fn println_info(prefix: impl Display, message: impl Display) {
    eprintln!("{} {}", format!("{}", prefix).blue(), message);
}

pub fn println_basic(message: impl Display) {
    println!("{}", message);
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn resolved_mode(command: &Commands) -> CompilationMode {
        match command {
            Commands::Build(args) => args.mode.resolve(),
            Commands::Run(args) => args.mode.resolve(),
            _ => panic!("command carries no compilation mode"),
        }
    }

    #[test]
    fn build_defaults_to_debug_mode() {
        let cli = Cli::parse_from(["zora", "build"]);

        assert_eq!(resolved_mode(&cli.command), CompilationMode::Debug);
    }

    #[test]
    fn build_release_flag_selects_release_mode() {
        let cli = Cli::parse_from(["zora", "build", "--release"]);

        assert_eq!(resolved_mode(&cli.command), CompilationMode::Release);
    }

    #[test]
    fn build_debug_flag_selects_debug_mode() {
        let cli = Cli::parse_from(["zora", "build", "--debug"]);

        assert_eq!(resolved_mode(&cli.command), CompilationMode::Debug);
    }

    #[test]
    fn build_mode_flag_still_selects_mode() {
        let cli = Cli::parse_from(["zora", "build", "--mode", "Release"]);

        assert_eq!(resolved_mode(&cli.command), CompilationMode::Release);
    }

    #[test]
    fn run_release_flag_selects_release_mode() {
        let cli = Cli::parse_from(["zora", "run", "--release"]);

        assert_eq!(resolved_mode(&cli.command), CompilationMode::Release);
    }

    #[test]
    fn run_forwards_program_args_after_release_flag() {
        let cli = Cli::parse_from(["zora", "run", "--release", "--", "one", "two"]);

        let Commands::Run(args) = &cli.command else {
            panic!("expected run command");
        };

        assert_eq!(args.mode.resolve(), CompilationMode::Release);
        assert_eq!(args.program_args, vec!["one", "two"]);
    }

    #[test]
    fn release_conflicts_with_debug() {
        let result = Cli::try_parse_from(["zora", "build", "--release", "--debug"]);

        assert!(result.is_err());
    }

    #[test]
    fn release_conflicts_with_mode() {
        let result = Cli::try_parse_from(["zora", "build", "--release", "--mode", "Debug"]);

        assert!(result.is_err());
    }

    #[test]
    fn debug_conflicts_with_mode() {
        let result = Cli::try_parse_from(["zora", "build", "--debug", "--mode", "Release"]);

        assert!(result.is_err());
    }
}
