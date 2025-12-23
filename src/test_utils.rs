use ethers::providers::{JsonRpcClient, ProviderError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct MockClient {
    responses: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl MockClient {
    pub fn new() -> Self {
        Self { responses: Arc::new(Mutex::new(Vec::new())) }
    }
    pub fn push<T: Serialize>(&self, res: T) {
        self.responses.lock().unwrap().push(serde_json::to_value(res).unwrap());
    }
}

#[async_trait::async_trait]
impl JsonRpcClient for MockClient {
    type Error = ProviderError;

    async fn request<T, R>(&self, method: &str, params: T) -> Result<R, Self::Error>
    where
        T: Debug + Serialize + Send + Sync,
        R: DeserializeOwned + Send,
    {
        println!("Request: {} {:?}", method, params);
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(ProviderError::CustomError(format!("No responses for {}", method)));
        }
        let res = responses.remove(0);
        serde_json::from_value(res).map_err(|e| ProviderError::SerdeJson(e))
    }
}
