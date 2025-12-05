//! Namespace for reference-related helpers.

pub mod crossref;

pub use crossref::{
    fetch_reference_metadata, format_retrieved_date, ReferenceAuthor, ReferenceDate,
    ReferenceMetadata, CROSSREF_QID,
};
