//! Helpers for validating and enriching chemical structure data.
use crate::error::{CrateError, Result};
use log::{info, warn};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

const API_BASE_URL: &str = "https://api.naturalproducts.net/latest";

static CANONICAL_SMILES_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^[A-Za-z0-9+\-\*=#$:().>/\\\[\]%]+$"#).expect("valid canonical SMILES regex")
});

static ISOMERIC_SMILES_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^[A-Za-z0-9+\-\*=#$:().>\[\]%]*([@\\/]|\\d)[A-Za-z0-9+\-\*=#$:().>@\\/\[\]%]+$"#)
        .expect("valid isomeric SMILES regex")
});

/// Normalized structural data returned by the chemical enrichment API.
#[derive(Debug, Clone)]
pub struct ChemicalStructureData {
    pub sanitized_smiles: String,
    pub sanitized_differs: bool,
    pub canonical_smiles: Option<String>,
    pub isomeric_smiles: Option<String>,
    pub inchi: Option<String>,
    pub inchikey: Option<String>,
    pub molecular_formula: Option<String>,
    pub other_descriptors: Option<HashMap<String, Value>>,
}

/// Validates canonical and isomeric SMILES against Wikidata's format constraints.
pub fn validate_smiles_pair(
    canonical: Option<String>,
    isomeric: Option<String>,
) -> Result<(Option<String>, Option<String>)> {
    if let Some(ref value) = canonical {
        if !CANONICAL_SMILES_REGEX.is_match(value) {
            return Err(CrateError::InvalidFormat {
                column: "canonical_smiles".to_string(),
                value: value.clone(),
                message: "Canonical SMILES must match Wikidata's SMILES regex.".to_string(),
            });
        }
    }
    if let Some(ref value) = isomeric {
        if !ISOMERIC_SMILES_REGEX.is_match(value) {
            return Err(CrateError::InvalidFormat {
                column: "isomeric_smiles".to_string(),
                value: value.clone(),
                message: "Isomeric SMILES must match Wikidata's SMILES regex and cannot contain escaped slashes."
                    .to_string(),
            });
        }
    }
    Ok((canonical, isomeric))
}

/// Fetches sanitized SMILES plus descriptors (InChI, InChIKey, etc.) for a structure.
pub async fn enrich_structure(
    smiles: &str,
    client: &reqwest::Client,
) -> Result<ChemicalStructureData> {
    let response = fetch_preprocessing(smiles, client).await?;

    let standardized_smiles = response
        .standardized
        .representations
        .canonical_smiles
        .clone()
        .ok_or_else(|| CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.to_string(),
            reason: "Sanitization service returned no SMILES".to_string(),
        })?;
    let sanitized_smiles = standardized_smiles.clone();
    let sanitized_differs = sanitized_smiles != smiles;

    if sanitized_smiles.is_empty() {
        return Err(CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.to_string(),
            reason: "Sanitized SMILES is empty".to_string(),
        });
    }

    if sanitized_smiles != smiles {
        info!(
            "Sanitized SMILES differs from original: {} -> {}",
            smiles, sanitized_smiles
        );
    }

    let parental_canonical = response
        .parent
        .as_ref()
        .and_then(|entry| entry.representations.canonical_smiles.clone());
    let isomeric_smiles = if response.standardized.has_stereo_defined {
        Some(standardized_smiles.clone())
    } else {
        None
    };
    let canonical_smiles = parental_canonical
        .clone()
        .unwrap_or_else(|| standardized_smiles.clone());
    let canonical_smiles = Some(canonical_smiles);

    let inchi = response.standardized.representations.standard_inchi.clone();
    let inchikey = response
        .standardized
        .representations
        .standard_inchikey
        .clone();

    if inchikey.as_deref().map(str::is_empty).unwrap_or(true) {
        return Err(CrateError::MissingDescriptor {
            descriptor: "inchikey".to_string(),
            smiles: sanitized_smiles.clone(),
        });
    }

    let molecular_formula = response
        .standardized
        .descriptors
        .as_ref()
        .and_then(|map| map.get("molecular_formula"))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string());
    let other_descriptors = response.standardized.descriptors.clone();

    let (canonical_smiles, isomeric_smiles) =
        validate_smiles_pair(canonical_smiles, isomeric_smiles)?;

    Ok(ChemicalStructureData {
        sanitized_smiles,
        sanitized_differs,
        canonical_smiles,
        isomeric_smiles,
        inchi,
        inchikey,
        molecular_formula,
        other_descriptors,
    })
}

#[derive(Debug, Deserialize)]
struct PreprocessingResponse {
    original: PreprocessingEntry,
    standardized: PreprocessingEntry,
    parent: Option<PreprocessingEntry>,
}

#[derive(Debug, Deserialize)]
struct PreprocessingEntry {
    representations: PreprocessingRepresentations,
    #[serde(default)]
    descriptors: Option<HashMap<String, Value>>,
    #[serde(default)]
    has_stereo_defined: bool,
}

#[derive(Debug, Deserialize)]
struct PreprocessingRepresentations {
    #[serde(rename = "canonical_smiles")]
    canonical_smiles: Option<String>,
    #[serde(rename = "standard_inchi")]
    standard_inchi: Option<String>,
    #[serde(rename = "standard_inchikey")]
    standard_inchikey: Option<String>,
}

async fn fetch_preprocessing(
    smiles: &str,
    client: &reqwest::Client,
) -> Result<PreprocessingResponse> {
    let url = format!("{}/chem/coconut/pre-processing", API_BASE_URL);
    info!("Running coconut pre-processing for SMILES: {}", smiles);

    let response = client
        .get(&url)
        .query(&[("smiles", smiles)])
        .send()
        .await
        .map_err(CrateError::ApiRequestError)?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        warn!(
            "Pre-processing API call failed for {}: Status {} - {}",
            smiles, status, body
        );
        return Err(CrateError::SmilesSanitizationFailed {
            input_smiles: smiles.to_string(),
            reason: format!("API returned status {}", status),
        });
    }

    response
        .json::<PreprocessingResponse>()
        .await
        .map_err(CrateError::ApiJsonDecodeError)
}
