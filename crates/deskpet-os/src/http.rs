//! 通过 Rust 侧发起 HTTP 请求，绕开 WebView 的跨域限制，并回传实际使用的请求头与完整响应头。

use std::time::{Duration, Instant};

use reqwest::header::{HeaderName, HeaderValue, ACCEPT_ENCODING, CONTENT_TYPE, USER_AGENT};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};

/// 请求头 / 查询参数键值对。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

/// 前端发起的请求描述。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequestSpec {
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub params: Vec<HeaderPair>,
    #[serde(default)]
    pub headers: Vec<HeaderPair>,
    #[serde(default)]
    pub body: Option<String>,
    /// json / text；json 时在缺少 Content-Type 的情况下自动补 application/json
    #[serde(default)]
    pub body_type: Option<String>,
}

/// 请求结果：包含实际发送的请求头与完整响应头。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseData {
    pub status: u16,
    pub status_text: String,
    pub duration_ms: u64,
    /// 响应体字节数（截断前）
    pub size: usize,
    pub request_headers: Vec<HeaderPair>,
    pub response_headers: Vec<HeaderPair>,
    pub body: String,
    pub truncated: bool,
    /// 非文本类型（如二进制下载），前端不展示内容
    pub binary: bool,
    /// 跟随重定向后的最终地址
    pub final_url: String,
}

const TIMEOUT: Duration = Duration::from_secs(30);
/// 单次响应最多回传 2 MB，避免撑爆 IPC
const MAX_BODY: usize = 2 * 1024 * 1024;
const USER_AGENT_VALUE: &str = concat!("DeskPet/", env!("CARGO_PKG_VERSION"));

fn parse_method(method: &str) -> Result<Method, String> {
    let m = method.trim().to_ascii_uppercase();
    match m.as_str() {
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" => {
            Method::from_bytes(m.as_bytes()).map_err(|_| format!("不支持的请求方法: {m}"))
        }
        _ => Err(format!("不支持的请求方法: {m}")),
    }
}

/// 解析并校验 URL，追加查询参数（自动百分号编码，保留 URL 中原有的查询串）。
fn build_url(raw: &str, params: &[HeaderPair]) -> Result<Url, String> {
    let mut url = Url::parse(raw.trim()).map_err(|e| format!("URL 无效: {e}"))?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => return Err(format!("仅支持 http/https 请求，当前为 {scheme}")),
    }
    {
        let mut query = url.query_pairs_mut();
        for param in params {
            let name = param.name.trim();
            if !name.is_empty() {
                query.append_pair(name, &param.value);
            }
        }
    }
    Ok(url)
}

fn host_header(url: &Url) -> String {
    match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        _ => String::new(),
    }
}

/// 按 Content-Type 判断能否作为文本展示；空类型交给 UTF-8 有损解码。
fn is_textual(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.starts_with("text/")
        || ct.contains("json")
        || ct.contains("xml")
        || ct.contains("javascript")
        || ct.contains("x-www-form-urlencoded")
        || ct.contains("svg")
}

/// 发送请求；错误以可读中文文案返回给前端。
pub async fn send(spec: &HttpRequestSpec) -> Result<HttpResponseData, String> {
    let url = build_url(&spec.url, &spec.params)?;
    let method = parse_method(&spec.method)?;
    let client = Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let mut request = client.request(method, url.clone());
    let mut request_headers: Vec<HeaderPair> = Vec::new();

    let host = host_header(&url);
    if !host.is_empty() {
        request_headers.push(HeaderPair {
            name: "Host".into(),
            value: host,
        });
    }

    let mut has_content_type = false;
    let mut has_user_agent = false;
    let mut has_accept_encoding = false;
    for header in &spec.headers {
        let name = header.name.trim();
        if name.is_empty() {
            continue;
        }
        let header_name =
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| format!("请求头名称无效: {name}"))?;
        let header_value =
            HeaderValue::from_str(&header.value).map_err(|_| format!("请求头 {name} 的值无效"))?;
        has_content_type |= header_name == CONTENT_TYPE;
        has_user_agent |= header_name == USER_AGENT;
        has_accept_encoding |= header_name == ACCEPT_ENCODING;
        request_headers.push(HeaderPair {
            name: name.to_string(),
            value: header.value.clone(),
        });
        request = request.header(header_name, header_value);
    }

    if !has_user_agent {
        request = request.header(USER_AGENT, USER_AGENT_VALUE);
        request_headers.push(HeaderPair {
            name: "User-Agent".into(),
            value: USER_AGENT_VALUE.into(),
        });
    }

    // reqwest 在开启压缩特性时会自动补该请求头，展示时保持一致
    if !has_accept_encoding {
        const ACCEPT_ENCODING_VALUE: &str = "gzip, deflate, br";
        request = request.header(ACCEPT_ENCODING, ACCEPT_ENCODING_VALUE);
        request_headers.push(HeaderPair {
            name: "Accept-Encoding".into(),
            value: ACCEPT_ENCODING_VALUE.into(),
        });
    }

    if let Some(body) = spec.body.as_deref().filter(|b| !b.is_empty()) {
        if !has_content_type {
            let (name, value) = if spec.body_type.as_deref() == Some("json") {
                ("Content-Type", "application/json")
            } else {
                ("Content-Type", "text/plain; charset=utf-8")
            };
            request = request.header(name, value);
            request_headers.push(HeaderPair {
                name: name.into(),
                value: value.into(),
            });
        }
        request_headers.push(HeaderPair {
            name: "Content-Length".into(),
            value: body.len().to_string(),
        });
        request = request.body(body.to_string());
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let duration_ms = started.elapsed().as_millis() as u64;

    let status = response.status();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| HeaderPair {
            name: name.as_str().to_string(),
            value: value.to_str().unwrap_or("<二进制值>").to_string(),
        })
        .collect::<Vec<_>>();

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取响应失败: {e}"))?;
    let size = bytes.len();
    let truncated = size > MAX_BODY;
    let mut body = String::from_utf8_lossy(&bytes[..size.min(MAX_BODY)]).to_string();
    let binary = !body.is_empty() && !content_type.is_empty() && !is_textual(&content_type);
    if binary {
        // 二进制内容回传给前端没有意义，只保留字节数
        body.clear();
    }

    Ok(HttpResponseData {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        duration_ms,
        size,
        request_headers,
        response_headers,
        body,
        truncated,
        binary,
        final_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_appends_encoded_params() {
        let url = build_url(
            "https://example.com/api?x=1",
            &[
                HeaderPair {
                    name: "q".into(),
                    value: "a b".into(),
                },
                HeaderPair {
                    name: " ".into(),
                    value: "skip".into(),
                },
            ],
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://example.com/api?x=1&q=a+b");
    }

    #[test]
    fn build_url_rejects_non_http_scheme() {
        assert!(build_url("file:///etc/passwd", &[]).is_err());
        assert!(build_url("not a url", &[]).is_err());
    }

    #[test]
    fn parse_method_is_case_insensitive_and_limited() {
        assert_eq!(parse_method("post").unwrap(), Method::POST);
        assert!(parse_method("BREW").is_err());
    }

    #[test]
    fn host_header_includes_non_default_port() {
        let url = Url::parse("http://127.0.0.1:8080/api").unwrap();
        assert_eq!(host_header(&url), "127.0.0.1:8080");
        let url = Url::parse("https://example.com").unwrap();
        assert_eq!(host_header(&url), "example.com");
    }

    #[test]
    fn textual_detection_covers_common_types() {
        assert!(is_textual("application/json; charset=utf-8"));
        assert!(is_textual("text/html"));
        assert!(is_textual("application/xml"));
        assert!(!is_textual("image/png"));
        assert!(!is_textual("application/octet-stream"));
    }
}
