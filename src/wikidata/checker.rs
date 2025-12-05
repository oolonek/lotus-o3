//! SPARQL helpers that check Wikidata for chemicals, taxa, references, and occurrences.
use crate::enrichment::EnrichedData;
use crate::error::{CrateError, Result};
use crate::reference::{ReferenceMetadata, fetch_reference_metadata};
use log::{info, warn};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

/// Stores results from Wikidata checks for a single row.
#[derive(Debug, Clone, Default)]
pub struct WikidataInfo {
    pub chemical_qid: Option<String>,
    pub taxon_qid: Option<String>,
    pub reference_qid: Option<String>,
    pub occurrence_exists: bool, // Added field for occurrence check
    pub reference_metadata: Option<ReferenceMetadata>,
}

// Structure to deserialize SPARQL JSON results (both SELECT and ASK)
// Made fields optional to handle variations in response structure
#[derive(Deserialize, Debug)]
struct SparqlResponse {
    head: Option<SparqlHead>,
    results: Option<SparqlResults>,
    boolean: Option<bool>, // For ASK queries
}

#[derive(Deserialize, Debug)]
struct SparqlHead {
    // Made vars optional as it might be missing
    #[serde(default)] // Use default (empty vec) if missing
    vars: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct SparqlResults {
    // Made bindings optional or default
    #[serde(default)] // Use default (empty vec) if missing
    bindings: Vec<HashMap<String, SparqlBinding>>,
}

#[derive(Deserialize, Debug)]
struct SparqlBinding {
    #[serde(rename = "type")]
    datatype: String,
    value: String,
}

const WIKIDATA_SPARQL_URL: &str = "https://query.wikidata.org/sparql";
pub const USER_AGENT: &str =
    "lotus-o3/0.1 (https://github.com/your_repo; your_email@example.com) reqwest/0.11"; // Replace with actual info

static JOURNAL_LABEL_CACHE: Lazy<Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static JOURNAL_ISSN_CACHE: Lazy<Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static REFERENCE_QID_CACHE: Lazy<Mutex<HashMap<String, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Helper function to execute a SPARQL query and parse the result
async fn execute_sparql_query(query: &str, client: &reqwest::Client) -> Result<SparqlResponse> {
    let response = client
        .get(WIKIDATA_SPARQL_URL)
        .query(&[("query", query), ("format", "json")])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/sparql-results+json")
        .send()
        .await
        .map_err(CrateError::SparqlQueryError)?;

    if !response.status().is_success() {
        // Use SparqlQueryError for non-2xx status codes from the SPARQL endpoint
        return Err(CrateError::SparqlQueryError(
            response.error_for_status().unwrap_err(),
        ));
    }

    // Use SparqlJsonDecodeError for errors during JSON decoding from response body
    let sparql_response: SparqlResponse = response
        .json()
        .await
        .map_err(CrateError::SparqlJsonDecodeError)?;

    Ok(sparql_response)
}

// Helper to extract QID from SPARQL bindings (for SELECT queries)
// Now handles potentially missing results or bindings
fn extract_qid(response: &SparqlResponse, var_name: &str) -> Option<String> {
    response.results.as_ref().and_then(|results| {
        results.bindings.get(0).and_then(|binding| {
            binding.get(var_name).and_then(|item_binding| {
                if item_binding.datatype == "uri" {
                    item_binding.value.split("/").last().map(String::from)
                } else {
                    None
                }
            })
        })
    })
}

// Check for chemical entity by InChIKey (P235)
async fn check_chemical(inchikey: &str, client: &reqwest::Client) -> Result<Option<String>> {
    let query = format!("SELECT ?item WHERE {{ ?item wdt:P235 \"{inchikey}\". }}");
    let response = execute_sparql_query(&query, client).await?;
    Ok(extract_qid(&response, "item"))
}

// Check for taxon by name
async fn check_taxon(taxon_name: &str, client: &reqwest::Client) -> Result<Option<String>> {
    let query = format!("SELECT ?item WHERE {{ ?item wdt:P225 \"{taxon_name}\". }}");
    let response = execute_sparql_query(&query, client).await?;
    Ok(extract_qid(&response, "item"))
}

// Check for reference (publication) by DOI (P356)
async fn check_reference(doi: &str, client: &reqwest::Client) -> Result<Option<String>> {
    let trimmed = doi.trim();
    let key = trimmed.to_lowercase();
    if let Some(cached) = REFERENCE_QID_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return Ok(cached);
    }
    info!("Searching Wikidata for DOI {}", trimmed);
    let mut candidates = Vec::new();
    candidates.push(trimmed.to_string());

    let upper = trimmed.to_uppercase();
    if upper != trimmed {
        candidates.push(upper);
    }

    let lower = trimmed.to_lowercase();
    if lower != trimmed && lower != trimmed.to_uppercase() {
        candidates.push(lower);
    }

    let mut found = None;
    for candidate in candidates {
        let escaped = candidate.replace('\"', "\\\"");
        let query = format!(
            r#"SELECT ?item WHERE {{
                {{
                    ?item wdt:P356 "{escaped}".
                }} UNION {{
                    SERVICE wdsubgraph:scholarly_articles {{
                        ?item wdt:P356 "{escaped}".
                    }}
                }}
            }}"#
        );
        let response = execute_sparql_query(&query, client).await?;
        if let Some(qid) = extract_qid(&response, "item") {
            found = Some(qid);
            break;
        }
    }

    if let Ok(mut cache) = REFERENCE_QID_CACHE.lock() {
        cache.insert(key, found.clone());
    }

    Ok(found)
}

async fn lookup_journal_qid(title: &str, client: &reqwest::Client) -> Result<Option<String>> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Some(cached) = JOURNAL_LABEL_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(trimmed).cloned())
    {
        return Ok(cached);
    }

    let escaped = trimmed.replace('"', "\"");
    let query = format!(
        r#"SELECT ?item WHERE {{
            VALUES ?class {{ wd:Q5633421 wd:Q1002697 wd:Q737498 }}
            ?item wdt:P31/wdt:P279* ?class ;
                  rdfs:label ?label .
            FILTER (lcase(str(?label)) = lcase("{escaped}"))
        }} LIMIT 1"#
    );

    let response = execute_sparql_query(&query, client).await?;
    let qid = extract_qid(&response, "item");
    if let Ok(mut cache) = JOURNAL_LABEL_CACHE.lock() {
        cache.insert(trimmed.to_string(), qid.clone());
    }
    Ok(qid)
}

async fn lookup_journal_qid_by_issn(
    issn: &str,
    client: &reqwest::Client,
) -> Result<Option<String>> {
    let trimmed = issn.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Some(cached) = JOURNAL_ISSN_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(trimmed).cloned())
    {
        return Ok(cached);
    }

    let escaped = trimmed.replace('"', "\"");
    let query = format!(
        r#"SELECT ?item WHERE {{
            ?item wdt:P236 "{escaped}" .
        }} LIMIT 1"#
    );

    let response = execute_sparql_query(&query, client).await?;
    let qid = extract_qid(&response, "item");
    if let Ok(mut cache) = JOURNAL_ISSN_CACHE.lock() {
        cache.insert(trimmed.to_string(), qid.clone());
    }
    Ok(qid)
}

// Check if the specific occurrence (chemical P703 taxon, ref DOI) exists
async fn check_occurrence(
    chemical_qid: &str,
    taxon_qid: &str,
    reference_qid: &str,
    client: &reqwest::Client,
) -> Result<bool> {
    let query = format!(
        // I need smt like
        // ASK WHERE {
        //     wd:Q213511 p:P703 ?statement.
        //     ?statement ps:P703 wd:Q2355919;
        //       wikibase:rank wikibase:NormalRank;
        //       (prov:wasDerivedFrom/pr:P248) wd:Q105275116.
        //   }
        "ASK WHERE {{
            wd:{chemical_qid} p:P703 ?statement.
            ?statement ps:P703 wd:{taxon_qid};
                wikibase:rank wikibase:NormalRank;
                (prov:wasDerivedFrom/pr:P248) wd:{reference_qid}.
        }}"
    );
    let response = execute_sparql_query(&query, client).await?;
    response.boolean.ok_or_else(|| {
        CrateError::SparqlResponseFormatError(
            "Missing or invalid \'boolean\' field in ASK WHERE response".to_string(),
        )
    })
}

/// Resolves existing Wikidata entities and detects missing references for an enriched record.
pub async fn check_wikidata(
    record: &EnrichedData,
    client: &reqwest::Client,
) -> Result<WikidataInfo> {
    let inchikey = record
        .inchikey
        .as_deref()
        .ok_or_else(|| CrateError::MissingDescriptor {
            descriptor: "inchikey".to_string(),
            smiles: record.input_smiles.clone(),
        })?;

    let chemical_qid_fut = check_chemical(inchikey, client);
    let taxon_qid_fut = check_taxon(&record.taxon_name, client);
    let reference_qid_fut = check_reference(&record.reference_doi, client);

    // Execute entity checks concurrently
    let (chemical_result, taxon_result, reference_result) =
        tokio::join!(chemical_qid_fut, taxon_qid_fut, reference_qid_fut);

    // Collect entity results, propagating the first error encountered
    let chemical_qid = chemical_result?;
    let taxon_qid = taxon_result?;
    let reference_qid = reference_result?;

    let mut occurrence_exists = false;
    let mut reference_metadata = None;
    // Only check occurrence if all three entities were found
    if let (Some(chem_q), Some(tax_q), Some(ref_q)) = (&chemical_qid, &taxon_qid, &reference_qid) {
        occurrence_exists = check_occurrence(chem_q, tax_q, ref_q, client).await?;
    } else if reference_qid.is_none() {
        info!(
            "DOI {} not found on Wikidata. Falling back to Crossref metadata lookup.",
            record.reference_doi
        );
        match fetch_reference_metadata(&record.reference_doi, client).await {
            Ok(Some(mut metadata)) => {
                if let Some(issn) = metadata.issn.clone() {
                    match lookup_journal_qid_by_issn(&issn, client).await {
                        Ok(Some(journal_qid)) => metadata.journal_qid = Some(journal_qid),
                        Ok(None) => {}
                        Err(err) => {
                            warn!("Failed to match journal ISSN {} on Wikidata: {}", issn, err)
                        }
                    }
                }

                if metadata.journal_qid.is_none() {
                    if let Some(title) = metadata.container_title.clone() {
                        match lookup_journal_qid(&title, client).await {
                            Ok(Some(journal_qid)) => metadata.journal_qid = Some(journal_qid),
                            Ok(None) => {}
                            Err(err) => {
                                warn!("Failed to match journal '{}' on Wikidata: {}", title, err)
                            }
                        }
                    }
                }
                reference_metadata = Some(metadata);
            }
            Ok(None) => reference_metadata = None,
            Err(err) => warn!(
                "Failed to fetch Crossref metadata for DOI {}: {}",
                record.reference_doi, err
            ),
        }
    }

    Ok(WikidataInfo {
        chemical_qid,
        taxon_qid,
        reference_qid,
        occurrence_exists,
        reference_metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::EnrichedData;
    use tokio;

    // Helper to create a basic EnrichedData for testing
    fn create_test_enriched_data() -> EnrichedData {
        EnrichedData {
            chemical_entity_name: "Test Compound".to_string(),
            input_smiles: "C".to_string(),
            sanitized_smiles: "C".to_string(),
            taxon_name: "Test Taxon".to_string(),
            reference_doi: "10.1234/test".to_string(),
            canonical_smiles: Some("C".to_string()),
            isomeric_smiles: Some("C".to_string()),
            inchi: Some("InChI=1S/CH4/h1H4".to_string()), // Example InChI for Methane
            inchikey: Some("VNWKTOKETHGBQD-UHFFFAOYSA-N".to_string()), // Methane InChIKey
            molecular_formula: Some("CH4".to_string()),
            other_descriptors: None,
        }
    }

    #[tokio::test]
    #[ignore] // Ignored by default to avoid hitting live Wikidata
    async fn test_check_methane_live() {
        let mut record = create_test_enriched_data();
        record.inchikey = Some("VNWKTOKETHGBQD-UHFFFAOYSA-N".to_string()); // Methane
        record.taxon_name = "Homo sapiens".to_string(); // Use a known taxon
        record.reference_doi = "10.1038/nature02403".to_string(); // Example DOI

        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .unwrap();
        let info = check_wikidata(&record, &client).await.unwrap();

        assert!(info.chemical_qid.is_some());
        // Note: QID might change, this is illustrative
        // Example QID for Methane is Q21 methane, but SPARQL returns just Q21
        assert_eq!(info.chemical_qid.unwrap(), "Q21");
        assert!(info.taxon_qid.is_some());
        assert_eq!(info.taxon_qid.unwrap(), "Q15978631"); // Homo sapiens
        assert!(info.reference_qid.is_some());
        // Occurrence check depends on whether this specific triple exists
        // println!("Occurrence exists: {}", info.occurrence_exists);
    }

    #[tokio::test]
    #[ignore]
    async fn test_check_nonexistent_chemical_live() {
        let mut record = create_test_enriched_data();
        record.inchikey = Some("AAAAAAAAAAAAAAAAAAAAAAAAAA-UHFFFAOYSA-N".to_string()); // Fake InChIKey
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .unwrap();
        let info = check_wikidata(&record, &client).await.unwrap();
        assert!(info.chemical_qid.is_none());
        // Occurrence check should be false as chemical_qid is None
        assert!(!info.occurrence_exists);
    }

    // Added test case provided by user
    #[tokio::test]
    #[ignore] // Ignored by default to avoid hitting live Wikidata
    async fn test_check_erythromycin_live() {
        let mut record = create_test_enriched_data();
        record.chemical_entity_name = "Erythromycin".to_string();
        record.input_smiles =
            "CCC(C)C(C1C(C(C(C(=O)O1)C)OC2CC(C(C(O2)C)O)(C)OC)OC3C(C(C(C(O3)C)O)N(C)C)O)O"
                .to_string(); // Example SMILES
        record.inchikey = Some("ULGZDMOVFRHVEP-RWJQBGPGSA-N".to_string()); // Erythromycin InChIKey
        record.taxon_name = "Streptomyces coelicolor".to_string(); // Corrected Taxon Name
        record.reference_doi = "10.1021/BI965010K".to_string(); // Corrected DOI

        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .unwrap();
        let info = check_wikidata(&record, &client).await.unwrap();

        // We display info for debugging
        println!("Chemical QID: {:?}", info.chemical_qid);
        println!("Taxon QID: {:?}", info.taxon_qid);
        println!("Reference QID: {:?}", info.reference_qid);
        println!("Occurrence exists: {:?}", info.occurrence_exists);
        // Assertions

        assert!(info.chemical_qid.is_some());
        assert_eq!(info.chemical_qid.unwrap(), "Q213511"); // Corrected Chemical QID
        assert!(info.taxon_qid.is_some());
        assert_eq!(info.taxon_qid.unwrap(), "Q2355919"); // Corrected Taxon QID
        assert!(info.reference_qid.is_some());
        assert_eq!(info.reference_qid.unwrap(), "Q105275116"); // Corrected Reference QID
        // Occurrence check depends on whether this specific triple exists
        // println!("Occurrence exists: {}", info.occurrence_exists);
    }

    // Add more tests for taxon, reference, occurrence, and error cases
    // Consider using a mock SPARQL server (e.g., using wiremock-rs)
}
