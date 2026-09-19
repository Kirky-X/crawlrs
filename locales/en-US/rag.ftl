# RAG configuration validation messages — en-US
# Covers RagSettings::validate_providers() (11 keys)

rag-config-embedding-provider-local-feature = rag.embedding_provider = "local" requires the rag-local feature (vecboost); rebuild with --features rag-local, or use "remote"
rag-config-embedding-provider-remote-feature = rag.embedding_provider = "remote" requires the rag-remote feature (rig-core); rebuild with --features rag-remote
rag-config-remote-embed-model-empty = rag.remote_embed_model cannot be empty
rag-config-embedding-provider-invalid = invalid rag.embedding_provider value "{ $value }" (supported: ""/local/remote)
rag-config-rerank-provider-local-feature = rag.rerank_provider = "local" requires the rag-local feature (vecboost); rebuild with --features rag-local, or use "http"
rag-config-rerank-endpoint-empty = rag.rerank_endpoint cannot be empty when rag.rerank_provider = "http"
rag-config-rerank-model-empty = rag.rerank_model cannot be empty when rag.rerank_format = "cohere"
rag-config-rerank-format-invalid = invalid rag.rerank_format value "{ $value }" (supported: cohere/tei)
rag-config-rerank-provider-invalid = invalid rag.rerank_provider value "{ $value }" (supported: ""/local/http)
rag-config-search-rerank-top-n = rag.search_rerank_top_n must be within 1..=100
rag-config-rag-top-k = rag.rag_top_k must be within 1..=50
