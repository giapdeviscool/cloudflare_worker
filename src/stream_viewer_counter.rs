use std::sync::Arc;

use serde_json::json;
use worker::*;
use scc::{HashMap, hash_map::Entry};

#[durable_object]
#[allow(dead_code)]
pub struct StreamViewerCounter {
    state: State,
    env: Env,
    // Thay HashSet<String> thành HashMap<String, u64> để lưu timestamp
    viewer_counter: Arc<HashMap<String, HashMap<String, u64>>>,
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
            "/connect" => {
                let stream_id = url.query_pairs()
                    .find(|(k, _)| k == "stream_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());

                let viewer_id = url.query_pairs()
                    .find(|(k, _)| k == "viewer_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());
                
                self.increment_viewer_count(&stream_id, &viewer_id).await;
                return Response::ok("Viewer count incremented");
            }
            "/active-viewers" => {
                let stream_id = url.query_pairs()
                    .find(|(k, _)| k == "stream_id")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or("guest".to_string());

                // Lấy viewers hiện tại (chưa disconnect)
                let active_count = self.get_active_viewer_count(&stream_id,30).await; // TTL 60s
                let body = json!({"active_viewer_count": active_count}).to_string();
                return Response::ok(&body);
            }
            _ => Response::error("Not found", 404)
        }
    }
}

impl StreamViewerCounter {
    pub async fn increment_viewer_count(&self, stream_id: &str, viewer_id: &str) {
        let viewer_counter = self.viewer_counter.clone();
        let now = js_sys::Date::now() as u64; // Lấy timestamp hiện tại (milliseconds)
        
        match viewer_counter.entry_async(stream_id.to_string()).await { 
            Entry::Occupied(entry_stream_id_occupied) => {
                let viewer_list = entry_stream_id_occupied.get();
                // Lưu viewer_id -> timestamp
                match viewer_list.entry_async(viewer_id.to_string()).await {                
                    Entry::Occupied(mut entry_viewer_id_occupied) => {
                        let viewer_timestamp = entry_viewer_id_occupied.get_mut();
                        *viewer_timestamp = now;
                    }
                    Entry::Vacant(entry_viewer_id_vacant) => {
                        let _ = entry_viewer_id_vacant.insert_entry(now);
                    }
                }
            }
            Entry::Vacant(entry_stream_id_vacant) => {
                let new_viewers = HashMap::new();
                let _ = new_viewers.insert_async(viewer_id.to_string(), now).await;
                entry_stream_id_vacant.insert_entry(new_viewers);
            }
        }
    }


    /// Get active viewers (chưa timeout)
    /// ttl_seconds: nếu viewer không ping trong thời gian này được coi là disconnect
    pub async fn get_active_viewer_count(&self, stream_id: &str, ttl_seconds: u64) -> u32 {
        let viewer_counter = self.viewer_counter.clone();
        let now = js_sys::Date::now() as u64;
        let ttl_ms = ttl_seconds * 1000;
        
        match viewer_counter.entry_async(stream_id.to_string()).await {
            Entry::Occupied(entry_stream_id_occupied) => {
                let viewer_session = entry_stream_id_occupied.get();
                let mut active_count = 0;
                let mut viewers_to_remove = Vec::new();
                console_log!("ses : {:#?}", viewer_session);
                // Duyệt qua tất cả viewers, đếm những cái chưa timeout
                viewer_session.iter_async(|viewer_id, timestamp| {
                    if now.saturating_sub(*timestamp) < ttl_ms {
                        active_count += 1;
                    } else {
                        // Collect timeout viewers để xóa sau
                        viewers_to_remove.push(viewer_id.clone());
                    }
                    true
                }).await;

                // Xóa các viewers timeout (sau khi duyệt xong)
                for viewer_id in viewers_to_remove {
                    let _ = viewer_session.remove_async(&viewer_id).await;
                }

                active_count
            }
            Entry::Vacant(_) => 0,
        }
    }

}