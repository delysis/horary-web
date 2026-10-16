use crate::llama::{active_llama_connection, LlamaError, LlamaState, LLAMA_SIDECAR_BACKEND};

#[derive(Debug, Clone, PartialEq)]
pub struct LocalInferenceConnection {
    pub backend: &'static str,
    pub model_id: String,
    pub chat_completions_endpoint: String,
}

pub fn active_inference_connection(
    state: &LlamaState,
) -> Result<Option<LocalInferenceConnection>, LlamaError> {
    Ok(
        active_llama_connection(state)?.map(|connection| LocalInferenceConnection {
            backend: LLAMA_SIDECAR_BACKEND,
            model_id: connection.model_id,
            chat_completions_endpoint: sidecar_chat_completions_endpoint(connection.port),
        }),
    )
}

pub fn sidecar_chat_completions_endpoint(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1/chat/completions")
}

pub fn local_inference_http_client() -> Result<reqwest::Client, LlamaError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .no_proxy()
        .build()
        .map_err(|error| LlamaError {
            message: format!("failed to initialize local model client: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_endpoint_is_loopback_chat_completions() {
        assert_eq!(
            sidecar_chat_completions_endpoint(32123),
            "http://127.0.0.1:32123/v1/chat/completions"
        );
    }
}
