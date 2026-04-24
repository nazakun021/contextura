use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
pub struct CliArgs {
    /// Headless mode, JSON to stdout
    #[arg(long)]
    pub debug_cli: bool,

    /// Pretty-printed JSON output in debug-cli mode
    #[arg(long, requires = "debug_cli")]
    pub pretty: bool,

    /// Trigger exactly one OCR cycle then exit
    #[arg(long, requires = "debug_cli")]
    pub once: bool,

    /// PNG input for debug-cli OCR/translation runs
    #[arg(long, value_name = "PNG", requires = "debug_cli")]
    pub input: Option<PathBuf>,

    /// Run E2E test suite against directory of PNGs + expected JSON
    #[arg(long, value_name = "DIR", requires = "debug_cli")]
    pub test_suite: Option<PathBuf>,

    /// Print manifest table and exit
    #[arg(long)]
    pub list_models: bool,

    /// Interactive model cleanup wizard
    #[arg(long)]
    pub prune_models: bool,
}

impl CliArgs {
    pub fn is_cli_mode(&self) -> bool {
        self.debug_cli || self.list_models || self.prune_models
    }
}
