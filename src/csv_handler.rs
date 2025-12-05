use crate::error::{CrateError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// Represents a row from the input CSV file.
#[derive(Debug, Deserialize)]
pub struct InputRecord {
    pub chemical_entity_name: String,
    pub chemical_entity_smiles: String,
    pub taxon_name: String,
    pub reference_doi: String,
}

#[derive(Debug, Clone)]
pub struct ColumnConfig {
    pub chemical_name: String,
    pub structure: String,
    pub taxon: String,
    pub doi: String,
}

impl ColumnConfig {
    pub fn default() -> Self {
        Self {
            chemical_name: "chemical_entity_name".to_string(),
            structure: "chemical_entity_smiles".to_string(),
            taxon: "taxon_name".to_string(),
            doi: "reference_doi".to_string(),
        }
    }

    fn name_for(&self, role: ColumnRole) -> &str {
        match role {
            ColumnRole::ChemicalName => &self.chemical_name,
            ColumnRole::Structure => &self.structure,
            ColumnRole::Taxon => &self.taxon,
            ColumnRole::Doi => &self.doi,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ColumnRole {
    ChemicalName,
    Structure,
    Taxon,
    Doi,
}

struct ColumnRequirement {
    role: ColumnRole,
    default_header: &'static str,
    cli_flag: &'static str,
    description: &'static str,
}

const COLUMN_REQUIREMENTS: [ColumnRequirement; 4] = [
    ColumnRequirement {
        role: ColumnRole::ChemicalName,
        default_header: "chemical_entity_name",
        cli_flag: "--column-chemical-name",
        description: "Chemical entity label (used for item creation)",
    },
    ColumnRequirement {
        role: ColumnRole::Structure,
        default_header: "chemical_entity_smiles",
        cli_flag: "--column-structure",
        description: "Chemical structure expressed as SMILES",
    },
    ColumnRequirement {
        role: ColumnRole::Taxon,
        default_header: "taxon_name",
        cli_flag: "--column-taxon",
        description: "Taxon label used for occurrence statements",
    },
    ColumnRequirement {
        role: ColumnRole::Doi,
        default_header: "reference_doi",
        cli_flag: "--column-doi",
        description: "Reference DOI backing the occurrence",
    },
];

fn normalize_taxon_name(taxon_name: &str) -> String {
    taxon_name
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

// Loads and validates the input CSV file.
pub fn load_and_validate_csv(file_path: &Path, columns: &ColumnConfig) -> Result<Vec<InputRecord>> {
    let mut reader = csv::Reader::from_path(file_path)?;
    let headers = reader.headers()?.clone();

    let header_map: HashMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(idx, name)| (name, idx))
        .collect();

    let chemical_idx = lookup_column_index(&header_map, columns, ColumnRole::ChemicalName)?;
    let structure_idx = lookup_column_index(&header_map, columns, ColumnRole::Structure)?;
    let taxon_idx = lookup_column_index(&header_map, columns, ColumnRole::Taxon)?;
    let doi_idx = lookup_column_index(&header_map, columns, ColumnRole::Doi)?;

    let mut valid_records = Vec::new();
    for (i, result) in reader.records().enumerate() {
        let record = result?;
        let row_num = i + 2; // header + 1-based index

        let mut normalized = InputRecord {
            chemical_entity_name: record.get(chemical_idx).unwrap_or("").trim().to_string(),
            chemical_entity_smiles: record.get(structure_idx).unwrap_or("").trim().to_string(),
            taxon_name: record.get(taxon_idx).unwrap_or("").trim().to_string(),
            reference_doi: record.get(doi_idx).unwrap_or("").trim().to_string(),
        };

        if normalized.chemical_entity_name.is_empty() {
            return Err(CrateError::MissingValue {
                column: columns.name_for(ColumnRole::ChemicalName).to_string(),
                row: row_num,
            });
        }
        if normalized.chemical_entity_smiles.is_empty() {
            return Err(CrateError::MissingValue {
                column: columns.name_for(ColumnRole::Structure).to_string(),
                row: row_num,
            });
        }
        if normalized.taxon_name.is_empty() {
            return Err(CrateError::MissingValue {
                column: columns.name_for(ColumnRole::Taxon).to_string(),
                row: row_num,
            });
        }
        if normalized.reference_doi.is_empty() {
            return Err(CrateError::MissingValue {
                column: columns.name_for(ColumnRole::Doi).to_string(),
                row: row_num,
            });
        }

        normalized.taxon_name = normalize_taxon_name(&normalized.taxon_name);
        valid_records.push(normalized);
    }

    Ok(valid_records)
}

fn lookup_column_index<'a>(
    header_map: &HashMap<&'a str, usize>,
    columns: &ColumnConfig,
    role: ColumnRole,
) -> Result<usize> {
    let expected = columns.name_for(role);
    header_map
        .get(expected)
        .copied()
        .ok_or_else(|| missing_header_error(expected, role, columns))
}

fn missing_header_error(missing: &str, role: ColumnRole, columns: &ColumnConfig) -> CrateError {
    let requirement = COLUMN_REQUIREMENTS
        .iter()
        .find(|req| req.role == role)
        .expect("column requirement must exist");
    let mut message = format!(
        "Missing required CSV column '{}' ({}).\n",
        missing, requirement.description
    );
    message.push_str("\nThe tool currently expects the following columns:\n");
    for req in COLUMN_REQUIREMENTS.iter() {
        let current = columns.name_for(req.role);
        message.push_str(&format!(
            "  - {} (default: {}) – {} [override with {} <COLUMN>]\n",
            current, req.default_header, req.description, req.cli_flag
        ));
    }
    message.push_str(
        "\nRename your CSV headers or rerun lotus-o3 with the override flags above to match your column names.",
    );
    CrateError::MissingHeader(message)
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
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,C1=CC=CC=C1,TaxonX species extra , 10.1000/test1 \nCompoundB,C,TaxonY,10.1000/test2";
        let file = create_test_csv(content);
        let records = load_and_validate_csv(file.path(), &ColumnConfig::default()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].chemical_entity_name, "CompoundA");
        assert_eq!(records[0].taxon_name, "TaxonX species");
        assert_eq!(records[0].reference_doi, "10.1000/test1");
        assert_eq!(records[1].taxon_name, "TaxonY");
    }

    #[test]
    fn test_missing_header() {
        let content =
            "chemical_entity_name,chemical_entity_smiles,taxon_name\nCompoundA,C1=CC=CC=C1,TaxonX";
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path(), &ColumnConfig::default());
        assert!(matches!(result, Err(CrateError::MissingHeader(h)) if h.contains("reference_doi")));
    }

    #[test]
    fn test_missing_value() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,,TaxonX,10.1000/test1";
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path(), &ColumnConfig::default());
        assert!(
            matches!(result, Err(CrateError::MissingValue{ column, row }) if column == "chemical_entity_smiles" && row == 2)
        );
    }

    #[test]
    fn test_empty_csv() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi";
        let file = create_test_csv(content);
        let records = load_and_validate_csv(file.path(), &ColumnConfig::default()).unwrap();
        assert_eq!(records.len(), 0);
    }

    #[test]
    fn test_malformed_csv() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\nCompoundA,C1,TaxonX"; // Missing DOI
        let file = create_test_csv(content);
        let result = load_and_validate_csv(file.path(), &ColumnConfig::default());
        assert!(matches!(result, Err(CrateError::CsvError(_))));
    }

    #[test]
    fn test_normalize_taxon_name() {
        assert_eq!(
            normalize_taxon_name("Vernonanthura patens (Kunth) H.Rob."),
            "Vernonanthura patens"
        );
        assert_eq!(normalize_taxon_name("Single"), "Single");
        assert_eq!(
            normalize_taxon_name("  Leading  and trailing  "),
            "Leading and"
        );
    }

    #[test]
    fn test_trim_fields() {
        let content = "chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi\n CompoundA , C1=CC=CC=C1 , TaxonX extra info , 10.5772/28961 \r";
        let file = create_test_csv(content);
        let records = load_and_validate_csv(file.path(), &ColumnConfig::default()).unwrap();
        assert_eq!(records[0].chemical_entity_name, "CompoundA");
        assert_eq!(records[0].chemical_entity_smiles, "C1=CC=CC=C1");
        assert_eq!(records[0].taxon_name, "TaxonX extra");
        assert_eq!(records[0].reference_doi, "10.5772/28961");
    }

    #[test]
    fn test_custom_column_mapping() {
        let content =
            "name,structure,taxa,doi\nCompoundA,C1=CC=CC=C1,Vernonanthura patens ,10.1000/test1";
        let file = create_test_csv(content);
        let config = ColumnConfig {
            chemical_name: "name".to_string(),
            structure: "structure".to_string(),
            taxon: "taxa".to_string(),
            doi: "doi".to_string(),
        };
        let records = load_and_validate_csv(file.path(), &config).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].chemical_entity_name, "CompoundA");
        assert_eq!(records[0].taxon_name, "Vernonanthura patens");
        assert_eq!(records[0].reference_doi, "10.1000/test1");
    }
}
