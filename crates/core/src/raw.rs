//! Raw HTTP helpers shared by every binary — thin, typed passthroughs over
//! octocrab's low-level verbs. The token only ever travels inside the
//! octocrab client (which loads it from the encrypted vault).

use anyhow::Result;
use http_body_util::combinators::BoxBody;
use octocrab::Octocrab;
use std::sync::Arc;

pub type Raw = (u16, serde_json::Value);

type OctoBody = BoxBody<bytes::Bytes, octocrab::Error>;

async fn collect(resp: http::Response<OctoBody>) -> Result<Raw> {
    let status = resp.status().as_u16();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| anyhow::anyhow!("reading response body: {e}"))?
        .to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    let value = if text.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.to_string()))
    };
    Ok((status, value))
}

/// GET any API route → (status, body)
pub async fn raw_get(octo: &Arc<Octocrab>, route: &str) -> Result<Raw> {
    let route = normalize(route);
    let resp = octo._get(route.as_str()).await?;
    collect(resp).await
}

/// POST/PUT/PATCH/DELETE any API route → (status, body)
pub async fn raw_send(
    octo: &Arc<Octocrab>,
    method: &str,
    route: &str,
    body: Option<serde_json::Value>,
) -> Result<Raw> {
    let route = normalize(route);
    let resp = match method.to_ascii_uppercase().as_str() {
        "POST" => octo._post::<serde_json::Value>(route.as_str(), body.as_ref()).await?,
        "PUT" => octo._put::<serde_json::Value>(route.as_str(), body.as_ref()).await?,
        "PATCH" => octo._patch::<serde_json::Value>(route.as_str(), body.as_ref()).await?,
        "DELETE" => octo._delete::<serde_json::Value>(route.as_str(), body.as_ref()).await?,
        other => anyhow::bail!("unsupported method '{other}'"),
    };
    collect(resp).await
}

fn normalize(route: &str) -> String {
    if route.starts_with('/') {
        route.to_string()
    } else {
        format!("/{route}")
    }
}
