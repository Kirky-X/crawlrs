# fluxweir 网关集成配置校验（FluxweirSettings::validate）
fluxweir-config-mode-invalid = 非法的 fluxweir.mode 值 "{ $value }"（支持：embedded/external）
fluxweir-config-listen-addr-empty = fluxweir.mode = "embedded" 时 fluxweir.listen_addr 不能为空
fluxweir-config-external-base-url-empty = fluxweir.mode = "external" 时 fluxweir.external_base_url 不能为空
fluxweir-config-api-key-missing = fluxweir 需要 API key：将 fluxweir.api_key_env 指向存放密钥的环境变量，或直接配置密钥
fluxweir-config-timeout-invalid = fluxweir.request_timeout_secs 必须在 1..=600 范围内
