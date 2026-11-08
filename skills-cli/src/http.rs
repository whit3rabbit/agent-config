//! Blocking HTTP client over `ureq` with the upstream timeouts.

use std::time::Duration;

use skills_store::error::{diag, SkillsError};

/// A fetched HTTP response.
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Response body bytes.
    pub body: Vec<u8>,
    /// Selected response headers, lowercased keys.
    pub headers: Vec<(String, String)>,
}

impl Response {
    /// Header value by (case-insensitive) name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Body as UTF-8 text.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// Body parsed as JSON.
    pub fn json(self) -> Result<serde_json::Value, SkillsError> {
        serde_json::from_slice(&self.body)
            .map_err(|err| SkillsError::new("SKILLS_IO", format!("invalid JSON: {err}")))
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .redirects(5)
        .build()
}

/// GET a URL with the given timeout and optional extra headers.
pub fn get(
    url: &str,
    timeout: Duration,
    headers: &[(&str, &str)],
) -> Result<Response, SkillsError> {
    let request = agent(timeout).get(url);
    let mut request = request;
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let mut headers = Vec::new();
            for name in response.headers_names() {
                if let Some(value) = response.header(&name) {
                    headers.push((name, value.to_string()));
                }
            }
            return Ok(Response {
                status: code,
                body: Vec::new(),
                headers,
            });
        }
        Err(other) => return Err(SkillsError::new("SKILLS_HTTP", other.to_string())),
    };
    let status = response.status();
    let mut out_headers = Vec::new();
    for name in response.headers_names() {
        if let Some(value) = response.header(&name) {
            out_headers.push((name, value.to_string()));
        }
    }
    let mut body = Vec::new();
    use std::io::Read;
    response
        .into_reader()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut body)
        .map_err(|err| SkillsError::new("SKILLS_HTTP", err.to_string()))?;
    Ok(Response {
        status,
        body,
        headers: out_headers,
    })
}

/// GET expecting success; non-2xx becomes `SKILLS_DOWNLOAD_HTTP_ERROR`.
pub fn get_ok(url: &str, timeout: Duration) -> Result<Response, SkillsError> {
    let response = get(url, timeout, &[])?;
    if response.status < 200 || response.status >= 300 {
        return Err(diag::download_http_error(response.status));
    }
    Ok(response)
}

/// Download up to `max_bytes`, enforcing the limit mid-stream.
pub fn download_capped(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Vec<u8>, SkillsError> {
    let request = agent(timeout).get(url);
    let response = request
        .call()
        .map_err(|err| SkillsError::new("SKILLS_HTTP", err.to_string()))?;
    if response.status() < 200 || response.status() >= 300 {
        return Err(diag::download_http_error(response.status()));
    }
    if let Some(length) = response
        .header("content-length")
        .and_then(|value| value.parse::<usize>().ok())
    {
        if length > max_bytes {
            return Err(diag::download_too_large(max_bytes));
        }
    }
    let mut body = Vec::new();
    use std::io::Read;
    let mut reader = response.into_reader();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|err| SkillsError::new("SKILLS_HTTP", err.to_string()))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if body.len() > max_bytes {
            return Err(diag::download_too_large(max_bytes));
        }
    }
    Ok(body)
}

/// The GitHub API token from the environment (upstream `getGitHubToken`).
#[must_use]
pub fn github_token() -> Option<String> {
    for name in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}
