use crate::error::CrateError;
use once_cell::sync::Lazy;
use regex::Regex;

static CANONICAL_SMILES_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^[A-Za-z0-9+\-\*=#$:().>/\\\[\]%]+$"#).expect("valid canonical SMILES regex")
});

static ISOMERIC_SMILES_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^[A-Za-z0-9+\-\*=#$:().>\[\]%]*([@\\/]|\\d)[A-Za-z0-9+\-\*=#$:().>@\\/\[\]%]+$"#)
        .expect("valid isomeric SMILES regex")
});

pub type SMILESValidationError = CrateError;

pub fn validate_smiles_pair(
    canonical: Option<String>,
    isomeric: Option<String>,
) -> Result<(Option<String>, Option<String>), SMILESValidationError> {
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
