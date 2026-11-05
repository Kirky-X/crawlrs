# fluxweir gateway integration config validation (FluxweirSettings::validate)
fluxweir-config-mode-invalid = invalid fluxweir.mode value "{ $value }" (supported: embedded/external)
fluxweir-config-listen-addr-empty = fluxweir.listen_addr cannot be empty when fluxweir.mode = "embedded"
fluxweir-config-external-base-url-empty = fluxweir.external_base_url cannot be empty when fluxweir.mode = "external"
fluxweir-config-api-key-missing = fluxweir requires an API key: set fluxweir.api_key_env to the environment variable holding the key, or configure the key directly
fluxweir-config-timeout-invalid = fluxweir.request_timeout_secs must be within 1..=600
