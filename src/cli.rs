use clap::{Args, Parser, Subcommand, ValueEnum};

/// Main CLI parser struct
#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "The Zeen package manager",
    long_about = None,
    help_template = "{options}"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Subcommand enum
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// `zora init`
    Init(InitArgs),

    /// `zora new`
    New(NewArgs),

    /// `zora build`
    Build(BuildArgs),

    /// `zora run`
    Run(RunArgs),

    /// `zora test`
    Test,

    /// `zora test`
    Add(AddArgs),

    /// `zora remove`
    Remove(RemoveArgs),

    /// `zora update`
    Update,

    /// `zora check`
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
