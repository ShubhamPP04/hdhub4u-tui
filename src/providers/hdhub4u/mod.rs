pub mod client;
pub mod parser;

pub use client::{HdHub4uClient, HdHub4uError};
pub use parser::{details_to_moviebox_json, releases_to_moviebox_json, search_to_moviebox_json};
