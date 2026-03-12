use std::sync::Arc;

use serde_json::json;
use worker::*;
use scc::{HashMap, HashSet, hash_map::Entry};

#[durable_object]
#[allow(dead_code)]
pub struct StreamViewerCounter {
    state: State,
    env: Env,
    viewer_counter: Arc<HashMap<String, HashSet<String>>>,
}

impl DurableObject for StreamViewerCounter {
    fn new(state: State, env: Env) -> Self {
        Self {
            state,
            env,
            viewer_counter: Arc::new(HashMap::new()),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();
        match path {
            "/increment" => {
                let stream_id = url.query_pairs()
                    .find(|(k, _)| k == "stream_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());

                // You may want to extract viewer_id from query as well
                let viewer_id = url.query_pairs()
                    .find(|(k, _)| k == "viewer_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());
                self.increment_viewer_count(&stream_id, &viewer_id).await;
                return Response::ok("Viewer count incremented");
            }
            "/count" => {
                let stream_id = url.query_pairs()
                    .find(|(k, _)| k == "stream_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());
                let count = self.get_viewer_count(&stream_id).await;
                let body = json!({"count":count}).to_string();
                return Response::ok(&body)
            }
            _ => Response::error("Not found", 404)
        }
    }
}

impl StreamViewerCounter {
    pub async fn increment_viewer_count(&self, stream_id: &str, viewer_id: &str) {
        let viewer_counter = self.viewer_counter.clone();
        match viewer_counter.entry_async(stream_id.to_string()).await { 
            Entry::Occupied(entry_stream_id_occupied) => {
                let viewer_list = entry_stream_id_occupied.get();
                let _ = viewer_list.insert_async(viewer_id.to_string()).await;
            }
            Entry::Vacant(entry_stream_id_vacant) => {
                let new_viewers = HashSet::new();
                let _ = new_viewers.insert_async(viewer_id.to_string()).await;
                entry_stream_id_vacant.insert_entry(new_viewers);
            }
        }
    }

    pub async fn get_viewer_count(&self, stream_id: &str) -> usize{
        let viewer_counter = self.viewer_counter.clone();
        match viewer_counter.entry_async(stream_id.to_string()).await {
            Entry::Occupied(entry_stream_id_occupied) => {
                let viewer_session = entry_stream_id_occupied.get();
                viewer_session.len()
            }
            Entry::Vacant(_) => 0,
        }
    }
}