use reqwest::header::{HeaderName, HeaderValue};
use reqwest::{Client, Method};
use std::env;
use std::str::FromStr;
use vercel_runtime::{run, Body, Error, Request, Response, StatusCode};

#[tokio::main]
async fn main() -> Result<(), Error> {
    run(handler).await
}

pub async fn handler(req: Request) -> Result<Response<Body>, Error> {
    let target_base = env::var("TARGET_DOMAIN")
        .unwrap_or_else(|_| "http://docker.io".to_string());

    let path_query = req
        .uri()
        .path_and_query()
        .map(|x| x.as_str())
        .unwrap_or("");
    let target_url = format!("{}{}", target_base, path_query);

    let method = Method::from_str(req.method().as_str()).unwrap_or(Method::GET);

    let client = Client::builder()
        .http2_prior_knowledge()
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|e| Error::from(format!("client build: {}", e)))?;

    let mut proxy_req = client.request(method, &target_url);

    let client_ip = req
        .headers()
        .get("x-real-ip")
        .or_else(|| req.headers().get("x-forwarded-for"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    for (name, value) in req.headers().iter() {
        let n = name.as_str();
        if n == "host" || n == "x-forwarded-for" || n == "x-real-ip" {
            continue;
        }
        proxy_req = proxy_req.header(n, value.as_bytes());
    }

    if let Some(ip) = client_ip {
        proxy_req = proxy_req.header("x-forwarded-for", ip);
    }

    let body_bytes: Vec<u8> = match req.into_body() {
        Body::Empty => Vec::new(),
        Body::Text(s) => s.into_bytes(),
        Body::Binary(b) => b,
    };
    proxy_req = proxy_req.body(body_bytes);

    let upstream = match proxy_req.send().await {
        Ok(r) => r,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::Text("Tunnel Link Broken".into()))?);
        }
    };

    let status = upstream.status().as_u16();
    let upstream_headers = upstream.headers().clone();

    let mut response_builder = Response::builder().status(status);
    if let Some(headers) = response_builder.headers_mut() {
        for (name, value) in upstream_headers.iter() {
            let n = name.as_str();
            if n == "transfer-encoding" || n == "connection" || n == "content-length" {
                continue;
            }
            if let (Ok(hn), Ok(hv)) = (
                HeaderName::from_bytes(n.as_bytes()),
                HeaderValue::from_bytes(value.as_bytes()),
            ) {
                headers.insert(hn, hv);
            }
        }
    }

    let body = upstream
        .bytes()
        .await
        .map_err(|e| Error::from(format!("upstream body read: {}", e)))?;

    Ok(response_builder.body(Body::Binary(body.to_vec()))?)
}
