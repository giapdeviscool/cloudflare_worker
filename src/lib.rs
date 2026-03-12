use std::sync::Arc;
use scc::{HashMap, hash_map::Entry};

use serde::Deserialize;
use serde_json::json;
use worker::{ok::Ok, *};

mod stream_viewer_counter;

fn extract_stream_id(req : &Request) -> String {
    let viewer_id = req.headers().get("x-viewer-id").ok().flatten().unwrap_or("unknown".to_string());
    viewer_id
}

#[derive(Deserialize, Debug)]
pub struct CountResponse {
    count: usize,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx : Context) -> Result<Response> {

    
    let url = req.url()?;
    let path = url.path();
    if req.method() == Method::Options {
        let mut response = Response::ok("")?;
        response.headers_mut().set("Access-Control-Allow-Origin", "https://stream-gate-i9.ermis.network")?;
        response.headers_mut().set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
        response.headers_mut().set("Access-Control-Allow-Headers", "x-viewer-id")?;
        return Ok(response);
    }

    match path {
        p if p.starts_with("/stream-gate/") => {
            let paths_segments: Vec<&str> = p.trim_start_matches("/stream-gate/").split('/').collect();
            if paths_segments.len() < 4 {
                return Response::error("Invalid path", 400);
            }
            let _hls = paths_segments[0];
            let _app_name = paths_segments[1];
            let stream_id = paths_segments[2];
            let stream_hls_method = paths_segments[3];
            let viewer_id = extract_stream_id(&req);
            let stream_counter_do = env.durable_object("STREAM_VIEWER_COUNT")?;
            let stream_counter_do_stub = stream_counter_do.id_from_name(&stream_id)?.get_stub()?;
            if stream_hls_method == "counter" {
                let mut response = stream_counter_do_stub.fetch_with_str(&format!(
                "https://stream_viewer_counter.worker/totalviewer?stream_id={}", 
                &stream_id
                )).await?;
                let body: CountResponse = response.json().await?;
                console_log!("Increment response: {:#?}", body);
                return Response::ok(json!({"count":body.count}).to_string());
            }
            if stream_hls_method == "disconnect" {
                let mut response = stream_counter_do_stub.fetch_with_str(&format!(
                "https://stream_viewer_counter.worker/disconnect?stream_id={}&viewer_id={}", 
                &stream_id, &viewer_id
                )).await?;
                let body: CountResponse = response.json().await?;
                console_log!("Increment response: {:#?}", body);
                return Response::ok(json!({"count":body.count}).to_string());
            }
            stream_counter_do_stub.fetch_with_str(&format!(
                "https://stream_viewer_counter.worker/connect?stream_id={}&viewer_id={}", 
                &stream_id, &viewer_id
            )).await?;
            Response::ok("New Viewer Connected")
        }
        _ => Response::error("Not found", 404)
    }
}
