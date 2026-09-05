//! Internationalization — re-export of the shared catalog.
//!
//! [`Locale`], [`Translator`] and the message catalog live in the
//! `bikesnest-i18n` crate so the infrastructure layer can render transactional
//! emails in the recipient's locale without depending on the web crate. This
//! module keeps the `bikesnest_web::i18n::{Locale, Translator, msg}` paths every
//! handler and template already uses, and the web crate enables the `axum`
//! feature, which is what makes `Locale` a request extractor.

pub use bikesnest_i18n::*;
