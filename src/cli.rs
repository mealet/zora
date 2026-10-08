use std::fmt::Display;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Serialize;

/// Main CLI parser struct
#[derive(Debug, Parser)]
#[command(author, version, about = "The Zeen package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
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

    /// Check project config and source
    Check,
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

/// `zora build` args
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// `-m --mode`
    #[arg(short, long, value_enum, default_value_t = CompilationMode::Debug, help="Specify compilation mode")]
    pub mode: CompilationMode,

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

#[derive(Debug, Default, Clone, Copy, ValueEnum)]
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
    /// `-m --mode`
    #[arg(short, long, value_enum, default_value_t = CompilationMode::Debug, help="Specify compilation mode")]
    pub mode: CompilationMode,
}

/// `zora add` args
#[derive(Debug, Args)]
pub struct AddArgs {
    #[arg(value_name = "git-url")]
    pub url: String,
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
    eprintln!("{} {}", "[Error]:".red().bold(), message);
}

pub fn println_warn(message: impl Display) {
    eprintln!("{} {}", "[Warn]:".yellow().bold(), message);
}

pub fn println_info(prefix: impl Display, message: impl Display) {
    eprintln!("{} {}", format!("{}", prefix).blue().bold(), message);
}

pub fn println_primary(message: impl Display) {
    println!("{}", message.to_string().bold().blue());
}

pub fn println_basic(message: impl Display) {
    println!("{}", message);
}
