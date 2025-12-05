//! Chemoinformatics enrichment utilities.
use crate::csv_handler::InputRecord;
use crate::error::{CrateError, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Holds the input data plus descriptors fetched from external services.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EnrichedData {
    pub chemical_entity_name: String,
    pub input_smiles: String,
    pub sanitized_smiles: String,
    pub taxon_name: String,
    pub reference_doi: String,
    pub canonical_smiles: Option<String>,
    pub isomeric_smiles: Option<String>, // Note: API doesn't seem to have a specific endpoint for this, might be same as canonical or require different handling
    pub inchi: Option<String>,
    pub inchikey: Option<String>,
    pub molecular_formula: Option<String>,
    // Store other descriptors for potential future use or debugging
    pub other_descriptors: Option<HashMap<String, Value>>,
}

// --- Structs for /chem/errors?fix=true response ---

#[derive(Deserialize, Debug)]
struct SmilesMessage {
    smi: String,
    messages: Vec<Vec<Value>>,
}

#[derive(Deserialize, Debug)]
struct SanitizationResponseFixed {
    original: SmilesMessage,
    standardized: SmilesMessage,
}

#[derive(Deserialize, Debug)]
struct SanitizationResponseNoErrors {
    smi: String,
    messages: Vec<String>,
}

// Enum to handle the two possible successful JSON structures
#[derive(Deserialize, Debug)]
#[serde(untagged)] // Allows deserializing into the first matching variant
enum SanitizationResult {
    Fixed(SanitizationResponseFixed),
    NoErrors(SanitizationResponseNoErrors),
}

// Structure to deserialize the /chem/descriptors response for molecular formula
#[derive(Deserialize, Debug)]
struct DescriptorsResponse {
    molecular_formula: Option<String>,
    // Include other fields if needed, using #[serde(flatten)] for flexibility
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

// Structure to deserialize the /convert/* responses (assuming simple string value)
#[derive(Deserialize, Debug)]
struct ConvertResponse {
    value: String,
}

const API_BASE_URL: &str = "https://api.naturalproducts.net/latest";

// Step 1: Sanitize SMILES using /chem/errors?fix=true
async fn sanitize_smiles(smiles: &str, client: &reqwest::Client) -> Result<String> {
    let url = format!("{}/chem/errors", API_BASE_URL);
    info!("Sanitizing SMILES: {}", smiles);

    let response = client
        .get(&url)
        .query(&[("smiles", smiles), ("fix", "true")])
        .send()
        .await
        .map_err(CrateError::ApiRequestError)?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        warn!(
            "SMILES sanitization API call failed for {}: Status {} - {}",
            smiles, status, error_text
        );
        return Err(CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.to_string(),
            reason: format!("API returned status {}", status),
        });
    }

    // Need to read the body first to check if it's plain text error
    let response_text = response.text().await.map_err(CrateError::ApiRequestError)?;
    if response_text.contains("Error reading SMILES string") {
        warn!(
            "SMILES sanitization failed for {}: API reported error reading SMILES",
            smiles
        );
        return Err(CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.to_string(),
            reason: "API could not read SMILES string".to_string(),
        });
    }

    // Attempt to parse the JSON
    match serde_json::from_str::<SanitizationResult>(&response_text) {
        Ok(SanitizationResult::Fixed(res)) => {
            info!(
                "Sanitized SMILES (fixed): {} -> {}",
                res.original.smi, res.standardized.smi
            );
            Ok(res.standardized.smi)
        }
        Ok(SanitizationResult::NoErrors(res)) => {
            info!("Sanitized SMILES (no errors): {}", res.smi);
            Ok(res.smi)
        }
        Err(e) => {
            warn!(
                "Failed to decode SMILES sanitization JSON response for {}: {}. Response text: {}",
                smiles, e, response_text
            );
            Err(CrateError::SmilesSanitizationFailed {
                input_smiles: smiles.to_string(),
                reason: format!("Failed to parse API response: {}", e),
            })
        }
    }
}

// Helper to fetch a single value from a /convert/* endpoint
async fn fetch_converted_value(
    endpoint: &str,
    smiles: &str,
    client: &reqwest::Client,
) -> Result<Option<String>> {
    let url = format!("{}/convert/{}", API_BASE_URL, endpoint);

    let response = client
        .get(&url)
        .query(&[("smiles", smiles)])
        .send()
        .await
        .map_err(CrateError::ApiRequestError)?;

    if !response.status().is_success() {
        // Log warning but don't fail the whole enrichment if one conversion fails
        warn!(
            "API call to /convert/{} failed for SMILES {}: Status {}",
            endpoint,
            smiles,
            response.status()
        );
        // Optionally read body for more details if needed
        // let error_body = response.text().await.unwrap_or_else(|_| "<failed to read body>".to_string());
        // warn!("Error body: {}", error_body);
        return Ok(None); // Return None instead of erroring out
    }

    // Attempt to parse as JSON first, fallback to plain text
    let response_bytes = response
        .bytes()
        .await
        .map_err(CrateError::ApiRequestError)?;

    if let Ok(json_response) = serde_json::from_slice::<ConvertResponse>(&response_bytes) {
        Ok(Some(json_response.value))
    } else if let Ok(text_response) = String::from_utf8(response_bytes.to_vec()) {
        // Trim potential quotes or whitespace from plain text response
        Ok(Some(text_response.trim().trim_matches('"').to_string()))
    } else {
        warn!(
            "Failed to decode response from /convert/{} for SMILES {} as JSON or Text",
            endpoint, smiles
        );
        Ok(None)
    }
}

// Helper to fetch molecular formula (and other descriptors) from /chem/descriptors
async fn fetch_descriptors(
    smiles: &str,
    client: &reqwest::Client,
) -> Result<Option<DescriptorsResponse>> {
    let url = format!("{}/chem/descriptors", API_BASE_URL);

    let response = client
        .get(&url)
        .query(&[("smiles", smiles)])
        .send()
        .await
        .map_err(CrateError::ApiRequestError)?;

    if !response.status().is_success() {
        warn!(
            "API call to /chem/descriptors failed for SMILES {}: Status {}",
            smiles,
            response.status()
        );
        return Ok(None);
    }

    // Use ApiJsonDecodeError for errors during JSON decoding from response body
    match response.json::<DescriptorsResponse>().await {
        Ok(data) => Ok(Some(data)),
        Err(e) => {
            warn!(
                "Failed to decode JSON response from /chem/descriptors for SMILES {}: {}",
                smiles, e
            );
            Err(CrateError::ApiJsonDecodeError(e))
        }
    }
}

// Helper to fetch error details from /chem/errors

// Enriches a single InputRecord with data from the API using specific endpoints
/// Calls the sanitization and descriptor APIs to enrich a CSV row.
pub async fn enrich_record(record: InputRecord, client: &reqwest::Client) -> Result<EnrichedData> {
    let smiles = &record.chemical_entity_smiles;

    let sanitized_smiles = sanitize_smiles(smiles, client).await?;
    info!("Sanitized SMILES: {}", sanitized_smiles);
    // Check if the sanitized SMILES is empty or invalid
    if sanitized_smiles.is_empty() {
        return Err(CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.clone(),
            reason: "Sanitized SMILES is empty".to_string(),
        });
    }
    // Check if the sanitized SMILES is different from the original
    if sanitized_smiles != *smiles {
        info!(
            "Sanitized SMILES differs from original: {} -> {}",
            smiles, sanitized_smiles
        );
    }
    // Proceed with the sanitized SMILES for further API calls
    let smiles = &sanitized_smiles;
    info!("Using sanitized SMILES for further API calls: {}", smiles);

    // Fetch data concurrently
    let (canon_smiles_res, inchi_res, inchikey_res, descriptors_res) = tokio::join!(
        fetch_converted_value("canonicalsmiles", smiles, client),
        fetch_converted_value("inchi", smiles, client),
        fetch_converted_value("inchikey", smiles, client),
        fetch_descriptors(smiles, client)
    );

    // Propagate critical errors (e.g., descriptor fetch failure if needed), handle optional ones
    let canonical_smiles = canon_smiles_res?;
    let inchi = inchi_res?;
    let inchikey = inchikey_res?;
    let descriptors_opt = descriptors_res?; // This returns Result<Option<DescriptorsResponse>>
    let molecular_formula = descriptors_opt
        .as_ref()
        .and_then(|d| d.molecular_formula.clone());
    let other_descriptors = descriptors_opt.map(|d| d.other);

    // Basic check: InChIKey is crucial for Wikidata lookup
    if inchikey.is_none() || inchikey.as_deref() == Some("") {
        return Err(CrateError::MissingDescriptor {
            descriptor: "inchikey".to_string(),
            smiles: smiles.clone(),
        });
    }

    // Note: Isomeric SMILES endpoint wasn't specified, using canonical for now.
    // If the API provides it via /convert/isomericSMILES or similar, add another call.
    let isomeric_smiles = canonical_smiles.clone();

    Ok(EnrichedData {
        chemical_entity_name: record.chemical_entity_name,
        input_smiles: smiles.clone(),
        sanitized_smiles,
        taxon_name: record.taxon_name,
        reference_doi: record.reference_doi,
        canonical_smiles,
        isomeric_smiles, // Using canonical as placeholder
        inchi,
        inchikey,
        molecular_formula,
        other_descriptors, // Store the rest from /descriptors
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    // Basic test hitting the actual API (use with caution, might be rate-limited or change)
    #[tokio::test]
    #[ignore] // Ignored by default to avoid hitting external API during normal tests
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
        // Canonical SMILES can sometimes vary slightly depending on the algorithm
        assert_eq!(
            enriched_data.canonical_smiles.unwrap(),
            "CN1C=NC2=C1C(=O)N(C)C(=O)N2C"
        );
        assert!(enriched_data.inchi.is_some());
        assert!(
            enriched_data
                .inchi
                .unwrap()
                .starts_with("InChI=1S/C8H10N4O2/")
        );
    }

    // Test case for a known problematic SMILES or one that might lack certain descriptors
    // Add more tests, including error cases and potentially mocking
    #[tokio::test]
    #[ignore] // Ignored by default to avoid hitting external API during normal tests
    async fn test_enrich_invalid_smiles() {
        let record = InputRecord {
            chemical_entity_name: "InvalidCompound".to_string(),
            chemical_entity_smiles: "Cl/C=C/1\\C=C2[C@]3([C@H]1OC(=O)C(C)CCCCCCC(CC([C@]1([C@@H]4[C@H]([C@@]52OC(O4)(O[C@@H]1[C@@H]5[C@H]1[C@]([C@H]3O)(CO)O1)c1ccccc1)C)O)(O)COC(=O)c1ccccc1)C)O".to_string(),
            taxon_name: "Trigonostemon cherrieri".to_string(),
            reference_doi: "10.1016/J.PHYTOCHEM.2012.07.023".to_string(),
        };
        let client = reqwest::Client::new();
        let result = enrich_record(record, &client).await;
        assert!(
            result.is_err(),
            "Expected enrichment to fail for invalid SMILES"
        );
        if let Err(e) = result {
            assert!(matches!(e, CrateError::SmilesSanitizationFailed { .. })) // Check if the error is specifically about SMILES sanitization
        } else {
            panic!("Expected an error but got Ok result");
        }
    }
}
