
use serde::Deserialize;
use serde_json::json;
use worker::{ok::Ok, *};

mod stream_viewer_counter;

fn extract_stream_id(req : &Request) -> String {
    let viewer_id = req.headers().get("x-viewer-id").ok().flatten().unwrap_or("unknown".to_string());
    viewer_id
}

async fn set_header_response_hls(response : &mut Response) -> Result<()>{
    response.headers_mut().set("Content-Type", "application/vnd.apple.mpegurl")?;
    response.headers_mut().set("Access-Control-Allow-Origin", "*")?;
    response.headers_mut().set("Access-Control-Expose-Headers", "Content-Length,Content-Type,Content-Range,cf-cache-status,x-cache,cdn-cache")?;
    response.headers_mut().set("Cache-Control", "public, max-age=2")?;
    response.headers_mut().set("Vary", "Origin, Accept-Encoding")?;
    Ok(())
}

async fn send_hls_viewer_count_to_server(
    server_url : String,
    active_data: ActiveViewerResponse
) -> Result<()> {

    let payload = json!({
        "viewer_count": active_data.active_viewer_count
    });

    let mut request_init = RequestInit::new();
    request_init.with_method(Method::Put);
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    request_init.with_headers(headers);
    request_init.with_body(Some(payload.to_string().into()));
    
    let request = Request::new_with_init(&server_url, &request_init)?;
    let server_response = Fetch::Request(request).send().await?;
    let status = server_response.status_code();
    
    if (200..300).contains(&status) {
        console_log!("[Worker] HLS viewer count sent successfully to {}", server_url);
        Ok(())
    } else {
        console_log!("[Worker] Failed to send viewer count to {}: {}", server_url, status);
        Err("Failed to send viewer count".into())
    }
}

#[derive(Deserialize, Debug)]
pub struct CountResponse {
    viewer_count: u32,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ActiveViewerResponse {
    active_viewer_count: u32,
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx : Context) -> Result<Response> {
    
    let url =req.url()?;
    let path = url.path();
    let origin = req.headers().get("origin").ok().flatten().unwrap_or("".to_string());
    if req.method() == Method::Options {
        let mut response = Response::ok("")?;
        response.headers_mut().set("Access-Control-Allow-Origin", &origin)?;
        response.headers_mut().set("Access-Control-Allow-Methods", "GET, OPTIONS")?;
        response.headers_mut().set("Access-Control-Allow-Headers", "x-viewer-id")?;
        return Ok(response);
    }
    let quality_collections = 
        vec![
            "original",
            "720p",
            "480p",
            "360p"
        ];
    match path {
        //for stream-gate server
        p if p.starts_with("/stream-gate/") => {     
            let paths_segments: Vec<&str> = p.trim_start_matches("/stream-gate/").split('/').collect();
            if paths_segments.len() < 4 {
                return Response::error("Invalid path", 400);
            }
            let _hls = paths_segments[0];
            let _app_name = paths_segments[1];
            let stream_id = paths_segments[2];
            let mut stream_hls_method = paths_segments[3];
            let mut quality="";
            if quality_collections.contains(&paths_segments[3]) {
                stream_hls_method = paths_segments[4];
                quality = paths_segments[3];
            }
            let viewer_id = extract_stream_id(&req);
            let stream_counter_do = env.durable_object("STREAM_VIEWER_COUNT")?;
            let stream_counter_do_stub = stream_counter_do.id_from_name(&stream_id)?.get_stub()?;
           
            stream_counter_do_stub.fetch_with_str(&format!(
                "https://stream_viewer_counter.worker/connect?stream_id={}&viewer_id={}", 
                &stream_id, &viewer_id
            )).await?;
            
            let mut cdn_url = format!("https://stream-gate-i9.ermis.network/stream-gate/hls/Streaming-demo/{}/{}",stream_id, stream_hls_method);
            //handle response
            if quality.len() != 0 {
                cdn_url = format!("https://stream-gate-i9.ermis.network/stream-gate/hls/Streaming-demo/{}/{}/{}",stream_id,quality,stream_hls_method);
            } 
            
            // Chỉ gửi count khi request playlist (master hoặc media)
            if stream_hls_method.ends_with(".m3u8") || stream_hls_method == "playlist.m3u8" || stream_hls_method == "master.m3u8" {
                let update_counter_url = format!("https://stream-gate-i9.ermis.network/stream-gate/hls/Streaming-demo/{}/upsert/viewers/count",stream_id);
            
                
                // Get active viewer count
                let mut active_response = stream_counter_do_stub.fetch_with_str(&format!(
                    "https://stream_viewer_counter.worker/active-viewers?stream_id={}",
                    &stream_id
                    )).await?;
                let active_data: ActiveViewerResponse = active_response.json().await?;
                
                let _ = send_hls_viewer_count_to_server(update_counter_url, active_data).await;
            }
            
            let mut cdn_res = Fetch::Url(cdn_url.parse()?).send().await?;
            let cdn_m3u8_body = cdn_res.text().await?;
            let url_segment = format!("https://stream-gate-i9.ermis.network/stream-gate/hls/Streaming-demo/{}/segment/", stream_id);
            let url_session = format!("https://stream-gate-i9.ermis.network/stream-gate/hls/Streaming-demo/{}/session/", stream_id);
            let update_content = cdn_m3u8_body
            .replace("segment/", &url_segment)
            .replace("session/", &url_session);
            // Tạo response với headers
            let mut response = Response::ok(&update_content)?;
            let _ = set_header_response_hls(&mut response).await;
            Ok(response)
        }
        //for cdn streams server
        p if p.starts_with("/streams/") => {
            let paths_segments: Vec<&str> = p.trim_start_matches("/streams/").split('/').collect();
            if paths_segments.len() < 3 {
                return Response::error("Invalid path", 400);
            }
            let _app_name = paths_segments[0];
            let stream_id = paths_segments[1];
            let mut stream_hls_method = paths_segments[2];
            let mut quality="";
            if quality_collections.contains(&paths_segments[2]) {
                quality = paths_segments[2];
                stream_hls_method = paths_segments[3];               
            }
            let viewer_id = extract_stream_id(&req);
            let stream_counter_do = env.durable_object("STREAM_VIEWER_COUNT")?;
            let stream_counter_do_stub = stream_counter_do.id_from_name(&stream_id)?.get_stub()?;       
            
            stream_counter_do_stub.fetch_with_str(&format!(
                "https://stream_viewer_counter.worker/connect?stream_id={}&viewer_id={}", 
                &stream_id, &viewer_id
            )).await?;

            let mut cdn_url = format!("https://hls-r2-dev.ermis.network/streams/Streaming-demo/{}/{}",stream_id, stream_hls_method);
            //handle response
            if quality.len() != 0 {
                cdn_url = format!("https://hls-r2-dev.ermis.network/streams/Streaming-demo/{}/{}/{}",stream_id,quality,stream_hls_method);
            } 
            
            // ✅Chỉ gửi count khi request playlist (master hoặc media)
            if stream_hls_method.ends_with(".m3u8") || stream_hls_method == "playlist.m3u8" || stream_hls_method == "master.m3u8" {
                let cdn_server_url = format!("https://hls-r2-dev.ermis.network/streams/Streaming-demo/{}/upsert/viewers/count",stream_id);
                
                // Get active viewer count
                let mut active_response = stream_counter_do_stub.fetch_with_str(&format!(
                    "https://stream_viewer_counter.worker/active-viewers?stream_id={}",
                    &stream_id
                    )).await?;
                let active_data: ActiveViewerResponse = active_response.json().await?;
                
                let _ = send_hls_viewer_count_to_server(cdn_server_url,  active_data).await;
            }

            let mut cdn_res = Fetch::Url(cdn_url.parse()?).send().await?;
            let cdn_m3u8_body = cdn_res.text().await?;
            // Tạo response với headers
            let mut response = Response::ok(&cdn_m3u8_body)?;
            let _ = set_header_response_hls(&mut response).await;
            Ok(response)
        }
        _ => {
            Response::error("Not found", 404)
        }
    }
}
