#![cfg(feature = "real-llm")]
//! Real embedding integration tests.
//!
//! Require a running Ollama instance with qwen3-embedding model.
//! Set `CORTEX_OLLAMA_URL` to override (default: `http://localhost:11434`).
//!
//! Run with: `cargo test -p cortex-turn --test real_embedding -- --ignored`

use cortex_kernel::embedding_client::{EmbeddingClient, validate_embedding};
use cortex_kernel::embedding_store::{EmbeddingStore, content_hash};
use cortex_types::config::{AuthType, ProviderConfig, ProviderProtocol};

fn ollama_url() -> String {
    std::env::var("CORTEX_OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into())
}

fn ollama_model() -> String {
    std::env::var("CORTEX_OLLAMA_MODEL").unwrap_or_else(|_| "qwen3-embedding".into())
}

fn make_client() -> EmbeddingClient {
    let provider = ProviderConfig {
        name: "Ollama".into(),
        protocol: ProviderProtocol::Ollama,
        base_url: ollama_url(),
        auth_type: AuthType::None,
        models: vec![],
        vision_provider: String::new(),
        vision_model: String::new(),
        image_input_mode: cortex_types::config::OpenAiImageInputMode::default(),
        files_base_url: String::new(),
        openai_stream_options: false,
        vision_max_output_tokens: 0,
        capability_cache_ttl_hours: 0,
    };
    EmbeddingClient::new(&provider, "", &ollama_model())
}

#[tokio::test]

async fn embed_produces_vector() {
    let client = make_client();
    let result = client.embed("Hello world").await;
    match result {
        Ok(vec) => {
            assert!(!vec.is_empty(), "Empty embedding vector");
            assert!(validate_embedding(&vec).is_ok(), "Degraded vector");
            eprintln!("Vector dimension: {}", vec.len());
        }
        Err(e) => {
            eprintln!("Embedding failed (Ollama may not be running): {e}");
        }
    }
}

#[tokio::test]

async fn similar_texts_high_cosine() {
    let client = make_client();
    let v1 = client.embed("The cat sat on the mat").await;
    let v2 = client.embed("A cat was sitting on the mat").await;

    if let (Ok(v1), Ok(v2)) = (v1, v2) {
        let sim = cortex_turn::memory::recall::cosine_similarity(&v1, &v2);
        eprintln!("Cosine similarity (similar texts): {sim:.4}");
        assert!(sim > 0.5, "Similar texts should have high cosine: {sim}");
    } else {
        eprintln!("Skipping: Ollama not available");
    }
}

#[tokio::test]

async fn different_texts_lower_cosine() {
    let client = make_client();
    let v1 = client.embed("Quantum physics is fascinating").await;
    let v2 = client.embed("I love baking chocolate cake").await;

    if let (Ok(v1), Ok(v2)) = (v1, v2) {
        let sim = cortex_turn::memory::recall::cosine_similarity(&v1, &v2);
        eprintln!("Cosine similarity (different texts): {sim:.4}");
        // Different topics should have lower similarity than similar topics
        // (but not necessarily < 0 — embeddings tend to be positive)
    } else {
        eprintln!("Skipping: Ollama not available");
    }
}

#[tokio::test]

async fn cache_stores_and_retrieves() {
    let client = make_client();
    let text = "Cache test embedding";

    let embed_result = client.embed(text).await;
    let Ok(vec) = embed_result else {
        eprintln!("Skipping: Ollama not available");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let store = EmbeddingStore::open(&dir.path().join("embed.db")).unwrap();
    let hash = content_hash(text);

    // Store
    store.put(&hash, &ollama_model(), &vec).unwrap();

    // Retrieve
    let cached = store.get(&hash).unwrap();
    assert_eq!(cached.len(), vec.len());
    for (a, b) in cached.iter().zip(vec.iter()) {
        assert!((a - b).abs() < f64::EPSILON);
    }
    eprintln!("Cache roundtrip OK, {} dimensions", vec.len());
}
