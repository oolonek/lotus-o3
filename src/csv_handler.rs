use crate::error::{CrateError, Result};
use serde::Deserialize;
use std::path::Path;

// Represents a row from the input CSV file.
#[derive(Debug, Deserialize)]
pub struct InputRecord {
    pub chemical_entity_name: String,
    pub chemical_entity_smiles: String,
    pub taxon_name: String,
    pub reference_doi: String,
    // Add other potential columns if needed, maybe using Option<String>
}

const REQUIRED_HEADERS: [&str; 4] = [
    "chemical_entity_name",
    "chemical_entity_smiles",
    "taxon_name",
    "reference_doi",
];

// Loads and validates the input CSV file.
pub fn load_and_validate_csv(file_path: &Path) -> Result<Vec<InputRecord>> {
    let mut reader = csv::Reader::from_path(file_path)?;
    let headers = reader.headers()?.clone();

    // 1. Validate Headers
    for required_header in REQUIRED_HEADERS.iter() {
        if !headers.iter().any(|h| h == *required_header) {
            return Err(CrateError::MissingHeader(required_header.to_string()));
        }
    }

    let mut valid_records = Vec::new();
    for (i, result) in reader.deserialize().enumerate() {
        let record: InputRecord = result?;
        let row_num = i + 2; // +1 for header, +1 for 0-based index

        // 2. Validate Required Values are not empty
        if record.chemical_entity_name.trim().is_empty() {
            return Err(CrateError::MissingValue {
                column: "chemical_entity_name".to_string(),
                row: row_num,
            });
        }
        if record.chemical_entity_smiles.trim().is_empty() {
            return Err(CrateError::MissingValue {
                column: "chemical_entity_smiles".to_string(),
                row: row_num,
            });
        }
        if record.taxon_name.trim().is_empty() {
            return Err(CrateError::MissingValue {
                column: "taxon_name".to_string(),
                row: row_num,
            });
        }
        if record.reference_doi.trim().is_empty() {
            return Err(CrateError::MissingValue {
                column: "reference_doi".to_string(),
                row: row_num,
            });
        }

        valid_records.push(record);
    }

    Ok(valid_records)
}

// Basic tests for the CSV handler
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_csv(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "{}", content).unwrap();
        file
    }

    #[test]
    fn test_load_valid_csv() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,C1=CC=CC=C1,TaxonX,10.1000/test1\nCompoundB,C,TaxonY,10.1000/test2";
        let file = create_test_csv(content);
        let records = load_and_validate_csv(file.path()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].chemical_entity_name, "CompoundA");
        assert_eq!(records[1].taxon_name, "TaxonY");
    }

    #[test]
    fn test_missing_header() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name\nCompoundA,C1=CC=CC=C1,TaxonX";
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path());
        assert!(matches!(result, Err(CrateError::MissingHeader(h)) if h == "reference_doi"));
    }

    #[test]
    fn test_missing_value() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,,TaxonX,10.1000/test1";
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path());
        assert!(matches!(result, Err(CrateError::MissingValue{ column, row }) if column == "chemical_entity_smiles" && row == 2));
    }

     #[test]
    fn test_empty_csv() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi";
        let file = create_test_csv(content);
        let records = load_and_validate_csv(file.path()).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_malformed_csv() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,C1,TaxonX"; // Missing DOI
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path());
        assert!(matches!(result, Err(CrateError::CsvError(_))));
    }
}

