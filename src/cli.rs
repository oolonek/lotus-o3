use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the input CSV file.
    #[arg(short, long, value_name = "FILE")]
    pub input_file: PathBuf,

    /// Output mode: generate QuickStatements or attempt direct push (not implemented).
    #[arg(short, long, value_enum, default_value = "qs")]
    pub mode: OutputMode,

    /// Path to the output QuickStatements file (required if mode is "qs").
    #[arg(short, long, value_name = "FILE", required_if_eq("mode", "qs"))]
    pub output_file: Option<PathBuf>,

    // TODO: Add options for verbosity/logging level
    // TODO: Add options for direct push credentials (if implemented)
}

#[derive(clap::ValueEnum, Debug, Clone, PartialEq, Eq)]
pub enum OutputMode {
    /// Generate a QuickStatements V1 file.
    #[value(name = "qs")]
    QuickStatements,
    /// Push data directly to Wikidata (requires authentication, not implemented).
    #[value(name = "direct")]
    DirectPush,
}

// Basic tests for CLI parsing
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_qs_mode() {
        let args = vec![
            "lotus-o3",
            "-i",
            "input.csv",
            "-m",
            "qs",
            "-o",
            "output.qs",
        ];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.input_file, PathBuf::from("input.csv"));
        assert_eq!(cli.mode, OutputMode::QuickStatements);
        assert_eq!(cli.output_file, Some(PathBuf::from("output.qs")));
    }

    #[test]
    fn test_cli_qs_mode_default() {
        let args = vec!["lotus-o3", "-i", "input.csv", "-o", "output.qs"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.input_file, PathBuf::from("input.csv"));
        assert_eq!(cli.mode, OutputMode::QuickStatements);
        assert_eq!(cli.output_file, Some(PathBuf::from("output.qs")));
    }

    #[test]
    fn test_cli_direct_mode() {
        let args = vec!["lotus-o3", "-i", "input.csv", "-m", "direct"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.input_file, PathBuf::from("input.csv"));
        assert_eq!(cli.mode, OutputMode::DirectPush);
        assert!(cli.output_file.is_none());
    }

    #[test]
    #[should_panic] // Expect panic because output_file is required for qs mode
    fn test_cli_qs_mode_missing_output() {
        let args = vec!["lotus-o3", "-i", "input.csv", "-m", "qs"];
        Cli::parse_from(args);
    }
}

