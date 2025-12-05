use crate::error::{CrateError, Result};
use chrono::{Datelike, NaiveDate, Utc};
use log::{info, warn};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;

const CROSSREF_API_URL: &str = "https://api.crossref.org/works/doi";
pub const CROSSREF_QID: &str = "Q5188229";
static CROSSREF_CACHE: Lazy<Mutex<HashMap<String, Option<ReferenceMetadata>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
pub struct ReferenceMetadata {
    pub doi: String,
    pub title: String,
    pub title_language: Option<String>,
    pub language_qid: Option<String>,
    pub entity_type_qid: String,
    pub publication_date: Option<ReferenceDate>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub container_title: Option<String>,
    pub issn: Option<String>,
    pub journal_qid: Option<String>,
    pub authors: Vec<ReferenceAuthor>,
    pub retrieved_on: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct ReferenceAuthor {
    pub full_name: String,
    pub ordinal: usize,
}

#[derive(Debug, Clone)]
pub struct ReferenceDate {
    pub year: i32,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

impl ReferenceDate {
    pub fn from_parts(parts: &[i32]) -> Option<Self> {
        if parts.is_empty() {
            return None;
        }

        let year = parts[0];
        let month = parts.get(1).copied().map(|v| v as u32);
        let day = parts.get(2).copied().map(|v| v as u32);

        Some(Self { year, month, day })
    }

    pub fn precision(&self) -> u8 {
        if self.day.is_some() {
            11
        } else if self.month.is_some() {
            10
        } else {
            9
        }
    }

    pub fn to_quickstatements_time(&self) -> String {
        let month = self.month.unwrap_or(1);
        let day = self.day.unwrap_or(1);
        format!(
            "+{year:04}-{month:02}-{day:02}T00:00:00Z/{precision}",
            year = self.year,
            precision = self.precision()
        )
    }
}

#[derive(Debug, Deserialize)]
struct CrossrefResponse {
    message: Option<CrossrefMessage>,
}

#[derive(Debug, Deserialize)]
struct CrossrefMessage {
    title: Option<Vec<String>>,
    #[serde(rename = "type")]
    work_type: Option<String>,
    language: Option<String>,
    author: Option<Vec<CrossrefAuthor>>,
    issued: Option<CrossrefIssued>,
    #[serde(rename = "container-title")]
    container_title: Option<Vec<String>>,
    volume: Option<String>,
    issue: Option<String>,
    #[serde(rename = "ISSN")]
    issn_list: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CrossrefAuthor {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CrossrefIssued {
    #[serde(rename = "date-parts")]
    date_parts: Vec<Vec<i32>>,
}

pub async fn fetch_reference_metadata(
    doi: &str,
    client: &reqwest::Client,
) -> Result<Option<ReferenceMetadata>> {
    let trimmed = doi.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let key = trimmed.to_lowercase();
    if let Some(cached) = CROSSREF_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return Ok(cached);
    }

    let url = format!("{}/{}", CROSSREF_API_URL, trimmed);
    info!("Querying Crossref for DOI {}", trimmed);
    let response = match client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            warn!("Crossref lookup failed for DOI {}: {}", trimmed, err);
            return Err(CrateError::ApiRequestError(err));
        }
    };

    if response.status() == StatusCode::NOT_FOUND {
        warn!("Crossref returned 404 for DOI {}", trimmed);
        cache_crossref_result(&key, None);
        return Ok(None);
    }

    if !response.status().is_success() {
        warn!(
            "Crossref returned unexpected status {} for DOI {}",
            response.status(),
            trimmed
        );
        cache_crossref_result(&key, None);
        return Ok(None);
    }

    let payload = match response.json::<CrossrefResponse>().await {
        Ok(data) => data,
        Err(err) => {
            warn!(
                "Failed to decode Crossref payload for DOI {}: {}",
                trimmed, err
            );
            return Err(CrateError::ApiJsonDecodeError(err));
        }
    };

    let message = match payload.message {
        Some(msg) => msg,
        None => {
            cache_crossref_result(&key, None);
            return Ok(None);
        }
    };

    let title = message
        .title
        .and_then(|mut titles| titles.drain(..).find(|t| !t.trim().is_empty()))
        .unwrap_or_else(|| trimmed.to_string());

    let language_code = message
        .language
        .as_deref()
        .and_then(normalize_language_code);
    let language_qid = language_code
        .as_deref()
        .and_then(language_code_to_qid)
        .map(|qid| qid.to_string());

    let entity_type_qid = map_work_type_to_qid(message.work_type.as_deref());

    let publication_date = message
        .issued
        .as_ref()
        .and_then(|issued| issued.date_parts.get(0))
        .and_then(|parts| ReferenceDate::from_parts(parts));

    let authors: Vec<ReferenceAuthor> = message
        .author
        .unwrap_or_default()
        .into_iter()
        .filter_map(|author| {
            let full_name = author.name.or_else(|| {
                let mut pieces = Vec::new();
                if let Some(given) = author.given {
                    if !given.trim().is_empty() {
                        pieces.push(given);
                    }
                }
                if let Some(family) = author.family {
                    if !family.trim().is_empty() {
                        pieces.push(family);
                    }
                }
                if pieces.is_empty() {
                    None
                } else {
                    Some(pieces.join(" "))
                }
            })?;
            let clean = full_name.trim();
            if clean.is_empty() {
                None
            } else {
                Some(clean.to_string())
            }
        })
        .enumerate()
        .map(|(idx, name)| ReferenceAuthor {
            full_name: name,
            ordinal: idx + 1,
        })
        .collect();

    let container_title = message
        .container_title
        .and_then(|mut titles| titles.drain(..).find(|t| !t.trim().is_empty()));

    let primary_issn = message
        .issn_list
        .as_ref()
        .and_then(|issns| issns.first())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let metadata = ReferenceMetadata {
        doi: trimmed.to_uppercase(),
        title,
        title_language: language_code,
        language_qid,
        entity_type_qid: entity_type_qid.to_string(),
        publication_date,
        volume: message.volume,
        issue: message.issue,
        container_title,
        issn: primary_issn,
        journal_qid: None,
        authors,
        retrieved_on: Utc::now().date_naive(),
    };
    let result = Some(metadata);
    cache_crossref_result(&key, result.clone());
    Ok(result)
}

fn normalize_language_code(code: &str) -> Option<String> {
    let normalized = code.trim().to_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(
            normalized
                .split('-')
                .next()
                .unwrap_or(&normalized)
                .to_string(),
        )
    }
}

fn language_code_to_qid(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("Q1860"),
        "es" => Some("Q1321"),
        "fr" => Some("Q150"),
        "de" => Some("Q188"),
        "pt" => Some("Q5146"),
        "it" => Some("Q652"),
        "ru" => Some("Q7737"),
        "zh" => Some("Q7850"),
        "ja" => Some("Q5287"),
        "pl" => Some("Q809"),
        "ar" => Some("Q13955"),
        _ => None,
    }
}

fn map_work_type_to_qid(work_type: Option<&str>) -> &'static str {
    match work_type.unwrap_or("") {
        "journal-article" => "Q13442814",
        "book-chapter" | "chapter" => "Q1980247",
        "book" => "Q571",
        "reference-entry" => "Q17329259",
        "report" => "Q10870555",
        "dataset" => "Q1172284",
        "dissertation" | "thesis" => "Q1266946",
        "proceedings-article" => "Q23927052",
        _ => "Q13442814",
    }
}

pub fn format_retrieved_date(date: NaiveDate) -> String {
    format!(
        "+{year:04}-{month:02}-{day:02}T00:00:00Z/11",
        year = date.year(),
        month = date.month(),
        day = date.day()
    )
}

fn cache_crossref_result(doi_key: &str, value: Option<ReferenceMetadata>) {
    if let Ok(mut cache) = CROSSREF_CACHE.lock() {
        cache.insert(doi_key.to_string(), value);
    }
}
