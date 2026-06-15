//! Local embedding backend.
//!
//! Compiled only with the `semantic` feature. Without it, this module exposes a
//! single `create` that reports the feature is absent, so the caller degrades to
//! lexical retrieval with a clear notice. With it, `create` initializes a
//! `fastembed` model (downloaded once into the data-dir model cache) and wraps
//! it as an [`Embedder`].

use std::path::Path;

use crate::search::index::Embedder;

/// Stable identifier of the embedding model, recorded next to each vector so a
/// model change invalidates the vector segment.
pub const MODEL_ID: &str = "bge-small-en-v1.5";

/// Try to construct the local embedder.
///
/// - Built without the `semantic` feature: returns `Err` with a notice that the
///   binary has no semantic support, so the caller falls back to lexical.
/// - Built with it: returns the initialized embedder, or `Err` if model
///   download/initialization failed.
///
/// # Errors
///
/// Returns a human-readable message describing why semantic retrieval is
/// unavailable.
#[cfg(feature = "semantic")]
pub fn create(model_cache_dir: &Path) -> Result<Box<dyn Embedder>, String> {
    imp::FastEmbedder::new(model_cache_dir).map(|e| Box::new(e) as Box<dyn Embedder>)
}

/// Stub when the `semantic` feature is disabled — always reports unavailability.
///
/// # Errors
///
/// Always returns a notice that the build lacks semantic support.
#[cfg(not(feature = "semantic"))]
pub fn create(_model_cache_dir: &Path) -> Result<Box<dyn Embedder>, String> {
    Err(
        "built without semantic support — rebuild with `--features semantic` for embeddings"
            .to_string(),
    )
}

#[cfg(feature = "semantic")]
mod imp {
    use std::path::Path;

    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

    use super::MODEL_ID;
    use crate::search::index::Embedder;

    pub struct FastEmbedder {
        model: TextEmbedding,
    }

    impl FastEmbedder {
        pub fn new(model_cache_dir: &Path) -> Result<Self, String> {
            let options = InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_cache_dir(model_cache_dir.to_path_buf())
                .with_show_download_progress(false);
            let model = TextEmbedding::try_new(options).map_err(|e| e.to_string())?;
            Ok(Self { model })
        }
    }

    impl Embedder for FastEmbedder {
        fn model_id(&self) -> &str {
            MODEL_ID
        }

        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            let docs: Vec<&str> = texts.iter().map(String::as_str).collect();
            self.model.embed(docs, None).map_err(|e| e.to_string())
        }
    }
}
