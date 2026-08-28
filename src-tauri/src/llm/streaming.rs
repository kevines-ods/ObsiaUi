use crate::llm::provider::{TokenEvent, TokenStream, LlmError};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tracing::{instrument};

pub struct StreamingManager {
    app_handle: AppHandle,
}

impl StreamingManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }

    #[instrument(skip(self, rx))]
    pub async fn forward_stream(&self, mut rx: TokenStream) -> Result<(), LlmError> {
        while let Some(event) = rx.recv().await {
            match event {
                TokenEvent::Token(token) => {
                    self.app_handle.emit("llm:token", token)
                        .map_err(|e| LlmError::StreamError(e.to_string()))?;
                }
                TokenEvent::Done(response) => {
                    self.app_handle.emit("llm:done", response)
                        .map_err(|e| LlmError::StreamError(e.to_string()))?;
                    break;
                }
                TokenEvent::Error(err) => {
                    self.app_handle.emit("llm:error", &err)
                        .map_err(|e| LlmError::StreamError(e.to_string()))?;
                    return Err(LlmError::StreamError(err));
                }
            }
        }
        Ok(())
    }

    pub fn emit_token(&self, token: String) -> Result<(), LlmError> {
        self.app_handle.emit("llm:token", token)
            .map_err(|e| LlmError::StreamError(e.to_string()))
    }

    pub fn emit_done(&self, response: crate::llm::provider::ChatResponse) -> Result<(), LlmError> {
        self.app_handle.emit("llm:done", response)
            .map_err(|e| LlmError::StreamError(e.to_string()))
    }

    pub fn emit_error(&self, error: String) -> Result<(), LlmError> {
        self.app_handle.emit("llm:error", error)
            .map_err(|e| LlmError::StreamError(e.to_string()))
    }
}

pub fn create_token_channel() -> (mpsc::Sender<TokenEvent>, TokenStream) {
    mpsc::channel(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_channel() {
        let (tx, mut rx) = create_token_channel();
        tx.send(TokenEvent::Token("hello".into())).await.unwrap();
        tx.send(TokenEvent::Token(" world".into())).await.unwrap();
        tx.send(TokenEvent::Done(crate::llm::provider::ChatResponse {
            id: "test".into(),
            model: "test".into(),
            choices: vec![],
            usage: None,
        })).await.unwrap();

        let mut tokens = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                TokenEvent::Token(t) => tokens.push(t),
                TokenEvent::Done(_) => break,
                TokenEvent::Error(_) => panic!("unexpected error"),
            }
        }
        assert_eq!(tokens, vec!["hello", " world"]);
    }
}