//! 通过 Rust 侧发起 HTTP 请求，绕开 WebView 的跨域限制，并回传实际使用的请求头与完整响应头。

use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use cookie_store::{CookieDomain, CookieStore, RawCookie};
use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, ACCEPT_ENCODING, CONTENT_TYPE, USER_AGENT};
use reqwest::{Client, Method, RequestBuilder, Url};
use serde::{Deserialize, Serialize};

use crate::template::Vars;

/// 请求头 / 查询参数键值对。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

/// form-data 的文件字段：name 为字段名，path 为本地文件绝对路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFile {
    pub name: String,
    pub path: String,
}

/// 认证方式：bearer 填 token，basic 填 username/password，apikey 填 key/value
/// （place 为 header 或 query），其余字段可缺省。
/// TS 契约为 auth.type / auth.in（Rust 关键字冲突，借 serde rename 对齐），缺失时视为未配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpAuth {
    /// none / bearer / basic / apikey
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
    /// apikey 的位置：header（默认）/ query
    #[serde(rename = "in", default)]
    pub place: String,
}

/// 前端发起的请求描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// json / text / form-urlencoded / form-data；json 时在缺少 Content-Type 的情况下自动补 application/json
    #[serde(default)]
    pub body_type: Option<String>,
    /// form-urlencoded 与 form-data 的文本字段（form-data 时与 form_files 并存）
    #[serde(default)]
    pub form: Vec<HeaderPair>,
    /// form-data 的文件字段
    #[serde(default)]
    pub form_files: Vec<FormFile>,
    #[serde(default)]
    pub auth: Option<HttpAuth>,
    /// 超时毫秒数；缺省或 0 用默认 30s
    #[serde(default)]
    pub timeout_ms: Option<u64>,
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
    /// 响应 Content-Type（未带则为空串）
    pub content_type: String,
    /// 图片响应时的预览 dataURL（截断到 MAX_BODY，前端 <img> 直接展示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_url: Option<String>,
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

/// 认证凭据的落点：头（默认）或查询参数。
enum AuthCredential {
    None,
    Header(String, String),
    Query(String, String),
}

/// 由认证配置生成需要附加的凭据；kind 未知或必填字段为空时视为未配置。
fn auth_credential(auth: &HttpAuth) -> AuthCredential {
    match auth.kind.trim().to_ascii_lowercase().as_str() {
        "bearer" if !auth.token.trim().is_empty() => AuthCredential::Header(
            "Authorization".into(),
            format!("Bearer {}", auth.token.trim()),
        ),
        "basic" if !auth.username.trim().is_empty() => {
            let raw = format!("{}:{}", auth.username, auth.password);
            let encoded = B64.encode(raw.as_bytes());
            AuthCredential::Header("Authorization".into(), format!("Basic {encoded}"))
        }
        "apikey" if !auth.key.trim().is_empty() && !auth.value.trim().is_empty() => {
            let name = auth.key.trim().to_string();
            let value = auth.value.clone();
            if auth.place.trim().eq_ignore_ascii_case("query") {
                AuthCredential::Query(name, value)
            } else {
                AuthCredential::Header(name, value)
            }
        }
        _ => AuthCredential::None,
    }
}

/// form-urlencoded 键值对编码为 `a=1&b=2`（按 x-www-form-urlencoded 序列化规则）。
fn encode_form(pairs: &[HeaderPair]) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for pair in pairs {
        query.append_pair(pair.name.trim(), &pair.value);
    }
    query.finish()
}

/// 全局共享 Cookie jar：自持 cookie_store（reqwest 的 Jar 不暴露迭代，无法做管理界面），
/// 实现 reqwest 的 CookieStore trait，跨请求保持会话，应用生命周期内存活。
struct SharedJar(StdMutex<CookieStore>);

impl reqwest::cookie::CookieStore for SharedJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let iter = cookie_headers
            .filter_map(|val| {
                RawCookie::parse(val.to_str().ok()?)
                    .map(|c| c.into_owned())
                    .ok()
            });
        self.0.lock().unwrap().store_response_cookies(iter, url);
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let joined = self
            .0
            .lock()
            .unwrap()
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if joined.is_empty() {
            None
        } else {
            HeaderValue::from_str(&joined).ok()
        }
    }
}

fn shared_cookie_jar() -> Arc<SharedJar> {
    static JAR: OnceLock<Arc<SharedJar>> = OnceLock::new();
    JAR
        .get_or_init(|| {
            Arc::new(SharedJar(StdMutex::new(
                CookieStore::default(),
            )))
        })
        .clone()
}

/// 模板求值（环境变量优先、其次函数），返回新 spec。
fn rendered_spec(spec: &HttpRequestSpec, vars: &Vars) -> HttpRequestSpec {
    let render = |s: &str| crate::template::render(s, vars);
    HttpRequestSpec {
        url: render(&spec.url),
        method: spec.method.clone(),
        params: spec
            .params
            .iter()
            .map(|p| HeaderPair {
                name: p.name.clone(),
                value: render(&p.value),
            })
            .collect(),
        headers: spec
            .headers
            .iter()
            .map(|p| HeaderPair {
                name: p.name.clone(),
                value: render(&p.value),
            })
            .collect(),
        body: spec.body.as_deref().map(|b| render(b)),
        body_type: spec.body_type.clone(),
        form: spec
            .form
            .iter()
            .map(|p| HeaderPair {
                name: p.name.clone(),
                value: render(&p.value),
            })
            .collect(),
        form_files: spec.form_files.clone(),
        auth: spec.auth.as_ref().map(|a| HttpAuth {
            kind: a.kind.clone(),
            token: render(&a.token),
            username: render(&a.username),
            password: render(&a.password),
            key: render(&a.key),
            value: render(&a.value),
            place: a.place.clone(),
        }),
        timeout_ms: spec.timeout_ms,
    }
}

/// 组装完成的请求（RequestBuilder 内部持有 Client）。
struct PreparedRequest {
    request: RequestBuilder,
    headers: Vec<HeaderPair>,
}

/// 由已求值的 spec 构建请求（认证、默认头、体）；普通发送与 SSE 共用。
async fn prepare_request(spec: &HttpRequestSpec) -> Result<PreparedRequest, String> {
    let credential = spec.auth.as_ref().map(auth_credential).unwrap_or(AuthCredential::None);
    // 认证为 query 落点时并入查询参数（与用户 params 一起编码）
    let mut params = spec.params.clone();
    if let AuthCredential::Query(name, value) = &credential {
        params.push(HeaderPair {
            name: name.clone(),
            value: value.clone(),
        });
    }
    let url = build_url(&spec.url, &params)?;
    let method = parse_method(&spec.method)?;
    let timeout = spec
        .timeout_ms
        .filter(|&ms| ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(TIMEOUT);
    let client = Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_provider(shared_cookie_jar())
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

    // 认证凭据以头落点时附加；与用户显式头同名时后者优先
    if let AuthCredential::Header(name, value) = &credential {
        let exists = request_headers.iter().any(|h| h.name.eq_ignore_ascii_case(name));
        if !exists {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| format!("认证头名称无效: {name}"))?;
            let header_value =
                HeaderValue::from_str(value).map_err(|_| format!("认证头 {name} 的值无效"))?;
            request = request.header(header_name, header_value);
            request_headers.push(HeaderPair {
                name: name.clone(),
                value: value.clone(),
            });
        }
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

    let body_type = spec.body_type.as_deref().unwrap_or("json");
    if body_type == "form-data" {
        // multipart：文本字段 + 文件字段；boundary 由 reqwest 自动生成
        let mut form = reqwest::multipart::Form::new();
        for field in &spec.form {
            let name = field.name.trim();
            if !name.is_empty() {
                form = form.text(name.to_string(), field.value.clone());
            }
        }
        for file in &spec.form_files {
            let name = file.name.trim();
            if name.is_empty() || file.path.trim().is_empty() {
                continue;
            }
            let bytes = tokio::fs::read(&file.path)
                .await
                .map_err(|e| format!("读取文件 {} 失败: {e}", file.path))?;
            let file_name = std::path::Path::new(&file.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            let mut part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
            if let Some(mime) = mime_guess::from_path(&file.path).first() {
                part = part.mime_str(mime.as_ref()).map_err(|_| "Content-Type 无效")?;
            }
            form = form.part(name.to_string(), part);
        }
        if !has_content_type {
            // 展示用：不拼 boundary（reqwest 发送时自动补）
            request_headers.push(HeaderPair {
                name: "Content-Type".into(),
                value: "multipart/form-data".into(),
            });
        }
        request = request.multipart(form);
    } else if body_type == "form-urlencoded" {
        if !has_content_type {
            const VALUE: &str = "application/x-www-form-urlencoded";
            request = request.header(CONTENT_TYPE, VALUE);
            request_headers.push(HeaderPair {
                name: "Content-Type".into(),
                value: VALUE.into(),
            });
        }
        let encoded = encode_form(&spec.form);
        if !encoded.is_empty() {
            request_headers.push(HeaderPair {
                name: "Content-Length".into(),
                value: encoded.len().to_string(),
            });
            request = request.body(encoded);
        }
    } else if let Some(body) = spec.body.as_deref().filter(|b| !b.is_empty()) {
        if !has_content_type {
            let value = if body_type == "json" {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            request = request.header(CONTENT_TYPE, value);
            request_headers.push(HeaderPair {
                name: "Content-Type".into(),
                value: value.into(),
            });
        }
        request_headers.push(HeaderPair {
            name: "Content-Length".into(),
            value: body.len().to_string(),
        });
        request = request.body(body.to_string());
    }

    Ok(PreparedRequest {
        request,
        headers: request_headers,
    })
}

/// 发送请求；错误以可读中文文案返回给前端。无论成败由调用方记录历史。
pub async fn send(spec: &HttpRequestSpec, vars: &Vars) -> Result<HttpResponseData, String> {
    let spec = &rendered_spec(spec, vars);
    let prepared = prepare_request(spec).await?;
    let started = Instant::now();
    let response = prepared
        .request
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
    // 图片响应附 base64 dataURL 供前端直接预览（svg 已按文本展示，不走此分支）
    let data_url = if binary && content_type.to_ascii_lowercase().starts_with("image/") {
        let mime = content_type.split(';').next().unwrap_or("").trim();
        Some(format!(
            "data:{mime};base64,{}",
            B64.encode(&bytes[..size.min(MAX_BODY)])
        ))
    } else {
        None
    };

    Ok(HttpResponseData {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        duration_ms,
        size,
        request_headers: prepared.headers,
        response_headers,
        body,
        truncated,
        binary,
        content_type,
        data_url,
        final_url,
    })
}

/* ---------- SSE（text/event-stream）流式接收 ---------- */

/// 单个 SSE 事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

/// SSE 流结束统计。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SseStats {
    pub events: usize,
    pub bytes: usize,
    pub status: u16,
    pub duration_ms: u64,
    pub truncated: bool,
}

/// SSE 总量与时长上限（超限自动断开并标记 truncated）。
const SSE_MAX_BYTES: usize = 16 * 1024 * 1024;
const SSE_MAX_DURATION: Duration = Duration::from_secs(600);

/// 以流式接收 text/event-stream；每个解析出的事件回调 `on_event`，结束时返回统计。
pub async fn send_sse<F>(spec: &HttpRequestSpec, vars: &Vars, mut on_event: F) -> Result<SseStats, String>
where
    F: FnMut(SseEvent) + Send,
{
    let spec = &rendered_spec(spec, vars);
    let prepared = prepare_request(spec).await?;
    let started = Instant::now();
    let response = prepared
        .request
        .send()
        .await
        .map_err(|e| format!("SSE 连接失败: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "SSE 连接被拒绝: {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        ));
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut events = 0usize;
    let mut bytes_total = 0usize;
    let mut truncated = false;

    loop {
        if started.elapsed() > SSE_MAX_DURATION || bytes_total > SSE_MAX_BYTES || buffer.len() > SSE_MAX_BYTES
        {
            truncated = true;
            break;
        }
        match stream.next().await {
            Some(Ok(chunk)) => {
                bytes_total += chunk.len();
                buffer.extend_from_slice(&chunk);
                // 按空行分帧逐个解析
                while let Some(pos) = find_frame_end(&buffer) {
                    let frame: Vec<u8> = buffer.drain(..pos).collect();
                    if let Some(event) = parse_sse_frame(&frame) {
                        events += 1;
                        on_event(event);
                    }
                }
            }
            Some(Err(e)) => {
                // 已收到部分事件后中断视为正常收尾；一无所获才算错误
                if events == 0 {
                    return Err(format!("SSE 读取失败: {e}"));
                }
                truncated = true;
                break;
            }
            None => break,
        }
    }
    // 无结尾空行的尾帧同样解析
    if !buffer.is_empty() {
        if let Some(event) = parse_sse_frame(&buffer) {
            events += 1;
            on_event(event);
        }
    }

    Ok(SseStats {
        events,
        bytes: bytes_total,
        status: status.as_u16(),
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
    })
}

/// 定位帧结束（`\n\n` 或 `\r\n\r\n`）位置，返回含分隔符的长度。
fn find_frame_end(buffer: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < buffer.len() {
        if buffer[i] == b'\n' {
            if buffer[i + 1] == b'\n' {
                return Some(i + 2);
            }
            if buffer[i + 1] == b'\r' && i + 3 < buffer.len() && buffer[i + 2] == b'\n' {
                return Some(i + 4);
            }
        }
        i += 1;
    }
    None
}

/// 解析一帧 SSE 文本；仅注释或空的帧返回 None。
fn parse_sse_frame(frame: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(frame);
    let mut event: Option<String> = None;
    let mut id: Option<String> = None;
    let mut retry: Option<u64> = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "id" => id = Some(value.to_string()),
            "retry" => retry = value.parse().ok(),
            "data" => data_lines.push(value.to_string()),
            _ => {}
        }
    }
    if data_lines.is_empty() && event.is_none() && id.is_none() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
        id,
        retry,
    })
}

/* ---------- Cookie 管理 ---------- */

/// 一条 Cookie 的展示信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieEntry {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

/// 当前共享 jar 中的全部 Cookie。
pub fn cookie_list() -> Vec<CookieEntry> {
    let jar = shared_cookie_jar();
    let store = jar.0.lock().unwrap();
    store
        .iter_any()
        .map(|c| CookieEntry {
            name: c.name().to_string(),
            value: c.value().to_string(),
            domain: match &c.domain {
                CookieDomain::HostOnly(d) | CookieDomain::Suffix(d) => d.clone(),
                _ => String::new(),
            },
            path: c.path.to_string(),
        })
        .collect()
}

/// 清空共享 jar。
pub fn cookie_clear() {
    shared_cookie_jar().0.lock().unwrap().clear();
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

    #[test]
    fn auth_credential_builds_bearer_and_basic() {
        let bearer = auth_credential(&HttpAuth {
            kind: "bearer".into(),
            token: " abc123 ".into(),
            username: String::new(),
            password: String::new(),
            key: String::new(),
            value: String::new(),
            place: String::new(),
        });
        assert!(matches!(
            bearer,
            AuthCredential::Header(ref n, ref v) if n == "Authorization" && v == "Bearer abc123"
        ));

        let basic = auth_credential(&HttpAuth {
            kind: "Basic".into(),
            token: String::new(),
            username: "user".into(),
            password: "pa:ss".into(),
            key: String::new(),
            value: String::new(),
            place: String::new(),
        });
        assert!(matches!(
            basic,
            AuthCredential::Header(_, ref v) if v.as_str() == format!("Basic {}", B64.encode(b"user:pa:ss"))
        ));

        // none / 空字段 / 未知 kind 均视为未配置
        for kind in ["none", "bearer", "unknown"] {
            assert!(matches!(
                auth_credential(&HttpAuth {
                    kind: kind.into(),
                    token: if kind == "bearer" { "  ".into() } else { String::new() },
                    username: String::new(),
                    password: String::new(),
                    key: String::new(),
                    value: String::new(),
                    place: String::new(),
                }),
                AuthCredential::None
            ));
        }
    }

    #[test]
    fn auth_credential_builds_api_key() {
        let header = auth_credential(&HttpAuth {
            kind: "apikey".into(),
            token: String::new(),
            username: String::new(),
            password: String::new(),
            key: " X-API-Key ".into(),
            value: "secret".into(),
            place: String::new(),
        });
        assert!(matches!(
            header,
            AuthCredential::Header(ref n, ref v) if n == "X-API-Key" && v == "secret"
        ));

        let query = auth_credential(&HttpAuth {
            kind: "apikey".into(),
            token: String::new(),
            username: String::new(),
            password: String::new(),
            key: "api_key".into(),
            value: "v1".into(),
            place: "query".into(),
        });
        assert!(matches!(
            query,
            AuthCredential::Query(ref n, ref v) if n == "api_key" && v == "v1"
        ));

        // key 或 value 为空视为未配置
        assert!(matches!(
            auth_credential(&HttpAuth {
                kind: "apikey".into(),
                token: String::new(),
                username: String::new(),
                password: String::new(),
                key: "  ".into(),
                value: "v".into(),
                place: String::new(),
            }),
            AuthCredential::None
        ));
    }

    #[test]
    fn spec_deserializes_timeout_and_apikey() {
        let json = r#"{"url":"https://e.com","method":"GET","timeoutMs":5000,"auth":{"type":"apikey","key":"k","value":"v","in":"query"}}"#;
        let spec: HttpRequestSpec = serde_json::from_str(json).unwrap();
        assert_eq!(spec.timeout_ms, Some(5000));
        assert!(matches!(
            auth_credential(spec.auth.as_ref().unwrap()),
            AuthCredential::Query(..)
        ));
    }

    #[test]
    fn encode_form_urlencodes_pairs() {
        let pairs = vec![
            HeaderPair {
                name: "q".into(),
                value: "a b".into(),
            },
            HeaderPair {
                name: " k ".into(),
                value: "v&1=2".into(),
            },
        ];
        assert_eq!(encode_form(&pairs), "q=a+b&k=v%261%3D2");
        assert_eq!(encode_form(&[]), "");
    }

    #[test]
    fn spec_deserializes_ts_auth_shape() {
        // TS 契约：auth.type（非 kind）；回归防范再次出现 missing field 'kind'
        let json = r#"{"url":"https://e.com","method":"POST","params":[],"headers":[],"body":"{}","bodyType":"json","auth":{"type":"bearer","token":"t1"}}"#;
        let spec: HttpRequestSpec = serde_json::from_str(json).unwrap();
        let auth = spec.auth.as_ref().unwrap();
        assert_eq!(auth.kind, "bearer");
        assert_eq!(auth.token, "t1");

        // auth 缺失或 kind 缺省均不报错
        let bare: HttpRequestSpec = serde_json::from_str(r#"{"url":"https://e.com","method":"GET"}"#).unwrap();
        assert!(bare.auth.is_none());
        let partial: HttpRequestSpec = serde_json::from_str(r#"{"url":"https://e.com","method":"GET","auth":{}}"#).unwrap();
        assert_eq!(partial.auth.unwrap().kind, "");

        // 序列化回写时键名与 TS 对齐（历史/收藏的 spec JSON 往返一致）
        let out = serde_json::to_value(&spec).unwrap();
        assert_eq!(out["auth"]["type"], "bearer");
    }
}
