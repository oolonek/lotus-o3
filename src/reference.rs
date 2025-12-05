//! Namespace for reference-related helpers.

pub mod crossref;

pub use crossref::{
    CROSSREF_QID, ReferenceAuthor, ReferenceDate, ReferenceMetadata, fetch_reference_metadata,
    format_retrieved_date,
};
