//! M4 photo pipeline infrastructure: image processing and the SQL photo repo.

pub mod processor;
pub mod repository;

pub use processor::LocalImageProcessor;
pub use repository::SqlxPhotoRepository;
