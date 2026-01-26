//! Network Request Capture
//!
//! Provides streaming capture of HTTP requests and responses.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::cdp::types::{
    NetworkRequestWillBeSentEvent,
    NetworkResponseReceivedEvent, NetworkLoadingFinishedEvent, NetworkLoadingFailedEvent,
};
use crate::cdp::transport::CdpMessage;
use crate::page::CapturedRequest;

/// Network event types
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new request was initiated
    RequestStarted(CapturedRequest),
    /// Response headers received
    ResponseReceived {
        request_id: String,
        status: i32,
        status_text: String,
        headers: HashMap<String, String>,
        mime_type: Option<String>,
    },
    /// Request completed successfully
    RequestCompleted {
        request_id: String,
        encoded_data_length: i64,
    },
    /// Request failed
    RequestFailed {
        request_id: String,
        error_text: String,
        canceled: bool,
    },
}

/// Watches network events and provides a stream of captured requests
pub struct NetworkWatcher {
    /// In-flight requests (request_id -> CapturedRequest)
    requests: Arc<Mutex<HashMap<String, CapturedRequest>>>,
    /// Channel to send events to consumers
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Channel to receive events
    event_rx: Mutex<mpsc::Receiver<NetworkEvent>>,
}

impl NetworkWatcher {
    /// Create a new NetworkWatcher
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            event_rx: Mutex::new(event_rx),
        }
    }

    /// Process a CDP event
    /// Returns true if the event was a network event that was processed
    pub async fn process_event(&self, event: &CdpMessage) -> bool {
        if let CdpMessage::Event { method, params, .. } = event {
            match method.as_str() {
                "Network.requestWillBeSent" => {
                    if let Ok(e) = serde_json::from_value::<NetworkRequestWillBeSentEvent>(params.clone()) {
                        self.on_request_will_be_sent(e).await;
                        return true;
                    }
                }
                "Network.responseReceived" => {
                    if let Ok(e) = serde_json::from_value::<NetworkResponseReceivedEvent>(params.clone()) {
                        self.on_response_received(e).await;
                        return true;
                    }
                }
                "Network.loadingFinished" => {
                    if let Ok(e) = serde_json::from_value::<NetworkLoadingFinishedEvent>(params.clone()) {
                        self.on_loading_finished(e).await;
                        return true;
                    }
                }
                "Network.loadingFailed" => {
                    if let Ok(e) = serde_json::from_value::<NetworkLoadingFailedEvent>(params.clone()) {
                        self.on_loading_failed(e).await;
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    async fn on_request_will_be_sent(&self, event: NetworkRequestWillBeSentEvent) {
        let request = CapturedRequest::from_request(
            event.request_id.clone(),
            &event.request,
            event.r#type.clone(),
            event.timestamp,
        );

        // Store the request
        {
            let mut requests = self.requests.lock().await;
            requests.insert(event.request_id.clone(), request.clone());
        }

        // Send event
        let _ = self.event_tx.send(NetworkEvent::RequestStarted(request)).await;
    }

    async fn on_response_received(&self, event: NetworkResponseReceivedEvent) {
        // Update the stored request
        {
            let mut requests = self.requests.lock().await;
            if let Some(request) = requests.get_mut(&event.request_id) {
                request.set_response(&event.response);
            }
        }

        // Send event
        let _ = self.event_tx.send(NetworkEvent::ResponseReceived {
            request_id: event.request_id,
            status: event.response.status,
            status_text: event.response.status_text,
            headers: event.response.headers,
            mime_type: event.response.mime_type,
        }).await;
    }

    async fn on_loading_finished(&self, event: NetworkLoadingFinishedEvent) {
        // Mark request as complete
        {
            let mut requests = self.requests.lock().await;
            if let Some(request) = requests.get_mut(&event.request_id) {
                request.mark_complete();
            }
        }

        // Send event
        let _ = self.event_tx.send(NetworkEvent::RequestCompleted {
            request_id: event.request_id,
            encoded_data_length: event.encoded_data_length,
        }).await;
    }

    async fn on_loading_failed(&self, event: NetworkLoadingFailedEvent) {
        // Remove failed request
        {
            let mut requests = self.requests.lock().await;
            requests.remove(&event.request_id);
        }

        // Send event
        let _ = self.event_tx.send(NetworkEvent::RequestFailed {
            request_id: event.request_id,
            error_text: event.error_text,
            canceled: event.canceled.unwrap_or(false),
        }).await;
    }

    /// Receive the next network event
    pub async fn recv(&self) -> Option<NetworkEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Try to receive a network event without blocking
    pub async fn try_recv(&self) -> Option<NetworkEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.try_recv().ok()
    }

    /// Get a captured request by ID
    pub async fn get_request(&self, request_id: &str) -> Option<CapturedRequest> {
        let requests = self.requests.lock().await;
        requests.get(request_id).cloned()
    }

    /// Get all captured requests
    pub async fn get_all_requests(&self) -> Vec<CapturedRequest> {
        let requests = self.requests.lock().await;
        requests.values().cloned().collect()
    }

    /// Get all completed requests
    pub async fn get_completed_requests(&self) -> Vec<CapturedRequest> {
        let requests = self.requests.lock().await;
        requests.values().filter(|r| r.complete).cloned().collect()
    }

    /// Clear all captured requests
    pub async fn clear(&self) {
        let mut requests = self.requests.lock().await;
        requests.clear();
    }

    /// Get requests matching a URL pattern
    pub async fn get_requests_matching(&self, pattern: &str) -> Vec<CapturedRequest> {
        let requests = self.requests.lock().await;
        requests.values()
            .filter(|r| r.url.contains(pattern))
            .cloned()
            .collect()
    }

    /// Wait for a request matching a URL pattern
    pub async fn wait_for_request(&self, pattern: &str, timeout_ms: u64) -> Option<CapturedRequest> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            // Check existing requests
            {
                let requests = self.requests.lock().await;
                if let Some(request) = requests.values().find(|r| r.url.contains(pattern)) {
                    return Some(request.clone());
                }
            }

            if start.elapsed() > timeout {
                return None;
            }

            // Wait a bit and check again
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Wait for a request to complete
    pub async fn wait_for_completion(&self, request_id: &str, timeout_ms: u64) -> Option<CapturedRequest> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        loop {
            {
                let requests = self.requests.lock().await;
                if let Some(request) = requests.get(request_id) {
                    if request.complete {
                        return Some(request.clone());
                    }
                }
            }

            if start.elapsed() > timeout {
                return None;
            }

            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

impl Default for NetworkWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_network_watcher_creation() {
        let watcher = NetworkWatcher::new();
        assert!(watcher.get_all_requests().await.is_empty());
    }

    #[tokio::test]
    async fn test_get_requests_matching() {
        let watcher = NetworkWatcher::new();

        // Manually insert a test request
        {
            let mut requests = watcher.requests.lock().await;
            let request = CapturedRequest {
                request_id: "1".to_string(),
                url: "https://api.example.com/users".to_string(),
                method: "GET".to_string(),
                headers: HashMap::new(),
                post_data: None,
                resource_type: Some("XHR".to_string()),
                status: None,
                status_text: None,
                response_headers: None,
                mime_type: None,
                timestamp: 0.0,
                complete: false,
            };
            requests.insert("1".to_string(), request);
        }

        let matches = watcher.get_requests_matching("api.example.com").await;
        assert_eq!(matches.len(), 1);
        assert!(matches[0].url.contains("api.example.com"));

        let no_matches = watcher.get_requests_matching("other.com").await;
        assert!(no_matches.is_empty());
    }
}
