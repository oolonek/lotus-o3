use thiserror::Error;

#[derive(Error, Debug)]
pub enum CrateError {
    #[error("CSV processing error: {0}")]
    CsvError(#[from] csv::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Missing required CSV header: {0}")]
    MissingHeader(String),

    #[error("Missing required value in column '{column}' at row {row}")]
    MissingValue { column: String, row: usize },

    #[error("API request error: {0}")]
    ApiRequestError(reqwest::Error),

    #[error("API returned an error status: {status} for SMILES: {smiles}")]
    ApiStatusError {
        status: reqwest::StatusCode,
        smiles: String,
    },

    #[error("Failed to decode API JSON response: {0}")]
    ApiJsonDecodeError(reqwest::Error),

    #[error("Failed to parse API response content: {0}")] // Kept for potential direct serde errors
    ApiResponseParseError(serde_json::Error),

    #[error("Missing expected descriptor '{descriptor}' in API response for SMILES: {smiles}")]
    MissingDescriptor { descriptor: String, smiles: String },

    #[error("Failed to sanitize SMILES: {input_smiles}")]
    SmilesSanitizationFailed {
        input_smiles: String,
        reason: String,
    },

    #[error("Wikidata SPARQL query failed: {0}")]
    SparqlQueryError(reqwest::Error),

    #[error("Failed to decode SPARQL JSON response: {0}")]
    SparqlJsonDecodeError(reqwest::Error),

    #[error("Failed to parse SPARQL response content: {0}")]
    // Kept for potential direct serde errors
    SparqlResponseParseError(serde_json::Error),

    // Corrected: Added a field to hold the reason string
    #[error("Unexpected SPARQL response format: {0}")]
    SparqlResponseFormatError(String),

    #[error("Wikidata check failed for record: {record_smiles}")]
    WikidataCheckError {
        record_smiles: String,
        source: Box<CrateError>,
    },

    #[error("QuickStatements generation error: {0}")]
    QuickStatementError(String),

    #[error("Wikidata write error (direct API): {0}")] // Placeholder
    WikidataWriteError(String),

    #[error("Missing QID for {entity_type} needed for occurrence statement (InChIKey: {inchikey})")]
    MissingQidForOccurrence {
        entity_type: String,
        inchikey: String,
    },
}

pub type Result<T> = std::result::Result<T, CrateError>;
