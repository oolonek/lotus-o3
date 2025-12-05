use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Path to the input CSV file.
    #[arg(short, long, value_name = "FILE")]
    pub input_file: PathBuf,

    /// CSV column for the chemical entity name.
    #[arg(
        long = "column-chemical-name",
        value_name = "COLUMN",
        default_value = "chemical_entity_name",
        help = "Header name for chemical names; override if your CSV uses a different label."
    )]
    pub column_chemical_name: String,

    /// CSV column for the chemical structure/SMILES.
    #[arg(
        long = "column-structure",
        value_name = "COLUMN",
        default_value = "chemical_entity_smiles",
        help = "Header name for the chemical structure (SMILES)."
    )]
    pub column_structure: String,

    /// CSV column for the taxon.
    #[arg(
        long = "column-taxon",
        value_name = "COLUMN",
        default_value = "taxon_name",
        help = "Header name for the taxon."
    )]
    pub column_taxon: String,

    /// CSV column for the reference DOI.
    #[arg(
        long = "column-doi",
        value_name = "COLUMN",
        default_value = "reference_doi",
        help = "Header name for the reference DOI."
    )]
    pub column_doi: String,

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
        let args = vec!["lotus-o3", "-i", "input.csv", "-m", "qs", "-o", "output.qs"];
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
    fn test_cli_qs_mode_missing_output() {
        let args = vec!["lotus-o3", "-i", "input.csv", "-m", "qs"];
        let result = Cli::try_parse_from(args);
        assert!(result.is_err());
    }
}
