# RAG 配置校验消息 — zh-CN
# 覆盖 RagSettings::validate_providers()（11 个 key）

rag-config-embedding-provider-local-feature = rag.embedding_provider = "local" 需要 rag-local feature（vecboost）；请以 --features rag-local 重新构建，或改用 "remote"
rag-config-embedding-provider-remote-feature = rag.embedding_provider = "remote" 需要 rag-remote feature（rig-core）；请以 --features rag-remote 重新构建
rag-config-remote-embed-model-empty = rag.remote_embed_model 不能为空
rag-config-embedding-provider-invalid = rag.embedding_provider 取值非法："{ $value }"（支持 ""/local/remote）
rag-config-rerank-provider-local-feature = rag.rerank_provider = "local" 需要 rag-local feature（vecboost）；请以 --features rag-local 重新构建，或改用 "http"
rag-config-rerank-endpoint-empty = rag.rerank_provider = "http" 时 rag.rerank_endpoint 不能为空
rag-config-rerank-model-empty = rag.rerank_format = "cohere" 时 rag.rerank_model 不能为空
rag-config-rerank-format-invalid = rag.rerank_format 取值非法："{ $value }"（支持 cohere/tei）
rag-config-rerank-provider-invalid = rag.rerank_provider 取值非法："{ $value }"（支持 ""/local/http）
rag-config-search-rerank-top-n = rag.search_rerank_top_n 须在 1..=100
rag-config-rag-top-k = rag.rag_top_k 须在 1..=50
