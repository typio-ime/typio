//! `typioctl` — command-line client for the Typio daemon (ADR-0004).

use std::process;

use clap::{Parser, Subcommand, ValueEnum};

mod commands;

use commands::OutputFormat;

#[derive(Parser)]
#[command(name = "typioctl", version, about = "Typio command-line client")]
struct Cli {
    /// Output format for command results.
    #[arg(long, short = 'o', value_enum, global = true, default_value_t = OutputFmt::Plain)]
    output: OutputFmt,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFmt {
    Plain,
    Json,
}

impl From<OutputFmt> for OutputFormat {
    fn from(value: OutputFmt) -> Self {
        match value {
            OutputFmt::Plain => OutputFormat::Plain,
            OutputFmt::Json => OutputFormat::Json,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Manage engines (`list`, `show`, `use`, `next`, `get`, `set`, `do`, ...)
    Engine {
        #[command(subcommand)]
        verb: EngineVerb,
    },

    /// Manage languages, the user-facing switch unit (`list`, `use`, `next`, `prev`)
    Language {
        #[command(subcommand)]
        verb: LanguageVerb,
    },

    /// Manage daemon configuration keys
    Config {
        #[command(subcommand)]
        verb: ConfigVerb,
    },

    /// Inspect or control the daemon itself
    Daemon {
        #[command(subcommand)]
        verb: DaemonVerb,
    },
}

#[derive(Subcommand)]
enum EngineVerb {
    /// List all engines; * marks the active one
    List,
    /// Show one engine's properties and commands
    Show { name: String },
    /// Make NAME the active engine (kind is inferred)
    Use { name: String },
    /// Switch to the next engine of a given kind (default: keyboard)
    Next {
        #[arg(long, value_enum)]
        kind: Option<EngineKind>,
    },
    /// List the configurable properties for NAME
    Props { name: String },
    /// List the invokable commands for NAME
    Actions { name: String },
    /// Read a property: `engine get rime schema`
    Get { name: String, key: String },
    /// Write a property: `engine set rime schema luna_pinyin`
    Set {
        name: String,
        key: String,
        value: String,
    },
    /// Invoke a command: `engine do rime deploy`
    Do { name: String, command: String },
    /// One-click setup: `engine setup rime` or `engine setup` to list
    Setup { name: Option<String> },
    /// Load an engine from a specific path
    Load {
        /// Path to the engine manifest
        path: String,
    },
    /// Unload an engine by name
    Unload {
        /// Engine name to unload
        name: String,
    },
    /// Reload an engine (unload then reload from path or rescan dirs)
    Reload {
        /// Engine name to reload
        name: String,
        /// Optional: explicit path to load from (otherwise rescans engine_dirs)
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum EngineKind {
    Keyboard,
    Voice,
}

impl EngineKind {
    fn as_str(&self) -> &'static str {
        match self {
            EngineKind::Keyboard => "keyboard",
            EngineKind::Voice => "voice",
        }
    }
}

#[derive(Subcommand)]
enum LanguageVerb {
    /// List enabled languages; * marks the active one
    List,
    /// Make TAG the active language (BCP-47, e.g. `zh-Hans`)
    Use { tag: String },
    /// Switch to the next language in the enabled cycle
    Next,
    /// Switch to the previous language in the enabled cycle
    Prev,
}

#[derive(Subcommand)]
enum ConfigVerb {
    /// Read a value by dotted key
    Get { key: String },
    /// Write a value by dotted key
    Set { key: String, value: String },
    /// Revert a key to its schema default
    Unset { key: String },
    /// List schema entries (optionally filtered by prefix)
    List {
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Print the raw daemon config text (TOML)
    Show,
    /// Open $EDITOR with the current config (read-only preview in TIP v1)
    Edit,
    /// Re-read the config from disk
    Reload,
}

#[derive(Subcommand)]
enum DaemonVerb {
    /// Daemon status (version, uptime, active engines, runtime)
    Status,
    /// Ask the daemon to exit
    Stop,
    /// Daemon version
    Version,
}

fn run(cli: Cli) -> std::io::Result<()> {
    let out: OutputFormat = cli.output.into();
    match cli.command {
        Command::Engine { verb } => match verb {
            EngineVerb::List => commands::engine_list(out),
            EngineVerb::Show { name } => commands::engine_show(&name, out),
            EngineVerb::Use { name } => commands::engine_use(&name, out),
            EngineVerb::Next { kind } => {
                commands::engine_next(kind.as_ref().map(EngineKind::as_str), out)
            }
            EngineVerb::Props { name } => commands::engine_props(&name, out),
            EngineVerb::Actions { name } => commands::engine_actions(&name, out),
            EngineVerb::Get { name, key } => commands::engine_get(&name, &key, out),
            EngineVerb::Set { name, key, value } => commands::engine_set(&name, &key, &value, out),
            EngineVerb::Do { name, command } => commands::engine_do(&name, &command, out),
            EngineVerb::Setup { name } => commands::engine_setup(name.as_deref(), out),
            EngineVerb::Load { path } => commands::engine_load(&path, out),
            EngineVerb::Unload { name } => commands::engine_unload(&name, out),
            EngineVerb::Reload { name, path } => {
                commands::engine_reload(&name, path.as_deref(), out)
            }
        },
        Command::Language { verb } => match verb {
            LanguageVerb::List => commands::language_list(out),
            LanguageVerb::Use { tag } => commands::language_use(&tag, out),
            LanguageVerb::Next => commands::language_cycle(true, out),
            LanguageVerb::Prev => commands::language_cycle(false, out),
        },
        Command::Config { verb } => match verb {
            ConfigVerb::Get { key } => commands::config_get(&key, out),
            ConfigVerb::Set { key, value } => commands::config_set(&key, &value, out),
            ConfigVerb::Unset { key } => commands::config_unset(&key, out),
            ConfigVerb::List { prefix } => commands::config_list(prefix.as_deref(), out),
            ConfigVerb::Show => commands::config_show(out),
            ConfigVerb::Edit => commands::config_edit(out),
            ConfigVerb::Reload => commands::config_reload(out),
        },
        Command::Daemon { verb } => match verb {
            DaemonVerb::Status => commands::daemon_status(out),
            DaemonVerb::Stop => commands::daemon_stop(out),
            DaemonVerb::Version => commands::daemon_version(out),
        },
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("typioctl: {e}");
        process::exit(1);
    }
}
