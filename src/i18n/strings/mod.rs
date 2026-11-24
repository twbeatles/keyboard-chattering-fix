//! Localizable UI strings (Korean / English).
mod api;
mod en;
mod ja;
mod ko;
mod schema;
#[cfg(test)]
mod tests;
pub use api::{fill, strings};
pub use schema::Strings;
