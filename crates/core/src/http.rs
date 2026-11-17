use crate::error::{Error, Result};
use serde_json::Value;
use std::time::Duration;

pub const TIMEOUT: u64 = 10;
pub const TIMEOUT_LONG: u64 = 20;

pub trait Http: Send + Sync {
    fn get_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        params: &[(&str, &str)],
        timeout_secs: u64,
    ) -> Result<Value>;
}

pub struct UreqHttp;

impl Http for UreqHttp {
    fn get_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        params: &[(&str, &str)],
        timeout_secs: u64,
    ) -> Result<Value> {
        let mut req = ureq::get(url).timeout(Duration::from_secs(timeout_secs));
        for (k, v) in headers {
            req = req.set(k, v);
        }
        for (k, v) in params {
            req = req.query(k, v);
        }
        let resp = req
            .call()
            .map_err(|e| Error::Network(format!("Request to {url} failed: {e}")))?;
        resp.into_json::<Value>()
            .map_err(|e| Error::Network(format!("Request to {url} returned invalid JSON: {e}")))
    }
}
