//! Retrieval and conversion of metadata from external identifier services.
//!
//! External formats stop at this module boundary. Callers receive the canonical
//! [`Reference`](crate::model::Reference), which can be persisted normally.

pub mod csl;
pub mod doi;

pub use doi::{Doi, DoiMetadataClient, LookupError};
