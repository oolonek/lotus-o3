//! Chemoinformatics enrichment utilities.
use crate::chemical_entity::structure::{ChemicalStructureData, enrich_structure};
use crate::csv_handler::InputRecord;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Holds the input data plus descriptors fetched from external services.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnrichedData {
    pub chemical_entity_name: String,
    pub input_smiles: String,
    pub sanitized_smiles: String,
    pub smiles_were_sanitized: bool,
    pub taxon_name: String,
    pub reference_doi: String,
    pub canonical_smiles: Option<String>,
    pub isomeric_smiles: Option<String>,
    pub inchi: Option<String>,
    pub inchikey: Option<String>,
    pub molecular_formula: Option<String>,
    pub exact_mass: Option<f64>,
    pub other_descriptors: Option<HashMap<String, Value>>,
}

/// Calls the underlying chemical-entity enrichment helpers for a single CSV row.
pub async fn enrich_record(record: InputRecord, client: &reqwest::Client) -> Result<EnrichedData> {
    let structure = enrich_structure(&record.chemical_entity_smiles, client).await?;
    let ChemicalStructureData {
        sanitized_smiles,
        smiles_were_sanitized,
        canonical_smiles,
        isomeric_smiles,
        inchi,
        inchikey,
        molecular_formula,
        exact_mass,
        other_descriptors,
    } = structure;

    Ok(EnrichedData {
        chemical_entity_name: record.chemical_entity_name,
        input_smiles: record.chemical_entity_smiles,
        sanitized_smiles,
        smiles_were_sanitized,
        taxon_name: record.taxon_name,
        reference_doi: record.reference_doi,
        canonical_smiles,
        isomeric_smiles,
        inchi,
        inchikey,
        molecular_formula,
        exact_mass,
        other_descriptors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CrateError;
    use tokio;

    #[tokio::test]
    #[ignore]
    async fn test_enrich_caffeine_live() {
        let record = InputRecord {
            chemical_entity_name: "Caffeine".to_string(),
            chemical_entity_smiles: "CN1C=NC2=C1C(=O)N(C(=O)N2C)C".to_string(),
            taxon_name: "Coffea arabica".to_string(),
            reference_doi: "10.1000/test".to_string(),
        };
        let client = reqwest::Client::new();
        let enriched_data = enrich_record(record, &client).await.unwrap();

        assert!(enriched_data.inchikey.is_some());
        assert_eq!(
            enriched_data.inchikey.unwrap(),
            "RYYVLZVUVIJVGH-UHFFFAOYSA-N"
        );
        assert!(enriched_data.molecular_formula.is_some());
        assert_eq!(enriched_data.molecular_formula.unwrap(), "C8H10N4O2");
        assert!(enriched_data.canonical_smiles.is_some());
        assert!(enriched_data.inchi.is_some());
        assert!(
            enriched_data
                .inchi
                .unwrap()
                .starts_with("InChI=1S/C8H10N4O2/")
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_enrich_invalid_smiles() {
        let record = InputRecord {
            chemical_entity_name: "InvalidCompound".to_string(),
            chemical_entity_smiles: "Cl/C=C/1\\C=C2[C@]3([C@H]1OC(=O)C(C)CCCCCCC(CC([C@]1([C@@H]4[C@H]([C@@]52OC(O4)(O[C@@H]1[C@@H]5[C@H]1[C@]([C@H]3O)(CO)O1)c1ccccc1)C)O)(O)COC(=O)c1ccccc1)C)O".to_string(),
            taxon_name: "Trigonostemon cherrieri".to_string(),
            reference_doi: "10.1016/J.PHYTOCHEM.2012.07.023".to_string(),
        };
        let client = reqwest::Client::new();
        let result = enrich_record(record, &client).await;
        assert!(result.is_err(), "Expected failure for invalid SMILES");
        if let Err(e) = result {
            assert!(matches!(e, CrateError::SmilesSanitizationFailed { .. }));
        }
    }
}
