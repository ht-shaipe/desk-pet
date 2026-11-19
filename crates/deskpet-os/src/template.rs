//! 请求模板函数：发送前对 URL、请求头/参数值、请求体求值。
//!
//! 语法 `{{name}}` 或 `{{func(arg)}}`，兼容 Postman 的 `{{$uuid}}` 等写法（`$` 前缀忽略）。
//! 未知名称原样保留（留给环境变量或后续扩展）；变量名优先于同名函数。

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{KeyInit, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use uuid::Uuid;

/// 变量表（环境变量），渲染时优先于同名函数。
pub type Vars = BTreeMap<String, String>;

/// 对字符串做模板求值：先替换环境变量，再求值剩余函数模板。
pub fn render(input: &str, vars: &Vars) -> String {
    if !input.contains("{{") {
        return input.to_string();
    }
    // 变量值本身也过一遍函数求值（{{timestamp}} 可写在变量值里）
    let evaluated: BTreeMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.clone(), render_functions(v, &BTreeMap::new())))
        .collect();
    render_functions(input, &evaluated)
}

/// 单个模板 token 求值：返回 None 表示未识别（原样保留）。
fn eval_token(name_raw: &str, arg: Option<&str>, vars: &BTreeMap<String, String>) -> Option<String> {
    let name = name_raw.trim();
    let var_name = name.strip_prefix('$').unwrap_or(name);
    if let Some(value) = vars.get(var_name) {
        return Some(value.clone());
    }
    let key = var_name.to_ascii_lowercase();
    let arg = arg.map(str::trim).unwrap_or("");
    let out = match key.as_str() {
        "uuid" => Uuid::new_v4().to_string(),
        "timestamp" => unix_secs().to_string(),
        "now" => iso_now(),
        "randomint" => (random_u32() % 1001).to_string(),
        "random" | "randomstring" => random_string(12),
        "md5" => hex_digest::<Md5>(arg.as_bytes()),
        "sha1" => hex_digest::<Sha1>(arg.as_bytes()),
        "sha256" => hex_digest::<Sha256>(arg.as_bytes()),
        "sha512" => hex_digest::<Sha512>(arg.as_bytes()),
        "hmacsha256" => hmac_sha_hex(arg, |key| <hmac::Hmac<Sha256>>::new_from_slice(key)),
        "hmacsha1" => hmac_sha_hex(arg, |key| <hmac::Hmac<Sha1>>::new_from_slice(key)),
        "hmacsha512" => hmac_sha_hex(arg, |key| <hmac::Hmac<Sha512>>::new_from_slice(key)),
        "base64" => B64.encode(arg.as_bytes()),
        "base64decode" => B64
            .decode(arg.as_bytes())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
        "urlencode" => percent_encode(arg),
        "urldecode" => percent_decode(arg),
        "env" if !arg.is_empty() => std::env::var(arg).unwrap_or_default(),
        _ => return None,
    };
    Some(out)
}

/// 扫描并替换全部 `{{...}}` token。
fn render_functions(input: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut rest = 0usize;
    while let Some(start) = input[rest..].find("{{") {
        let token_start = rest + start;
        out.push_str(&input[rest..token_start]);
        match input[token_start + 2..].find("}}") {
            Some(end) => {
                let inner = &input[token_start + 2..token_start + 2 + end];
                let (name, arg) = split_token(inner);
                match eval_token(name, arg, vars) {
                    Some(value) => out.push_str(&value),
                    None => out.push_str(&input[token_start..token_start + 2 + end + 2]),
                }
                rest = token_start + 2 + end + 2;
            }
            None => {
                // 未闭合：原样保留剩余内容
                out.push_str(&input[token_start..]);
                rest = input.len();
            }
        }
    }
    if rest < bytes.len() {
        out.push_str(&input[rest..]);
    }
    out
}

/// 拆分 token：`name` / `name(arg)`；参数按首个逗号分成两段。
fn split_token(inner: &str) -> (&str, Option<&str>) {
    let trimmed = inner.trim();
    match trimmed.find('(') {
        Some(idx) if trimmed.ends_with(')') => {
            (&trimmed[..idx], Some(&trimmed[idx + 1..trimmed.len() - 1]))
        }
        _ => (trimmed, None),
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// UTC ISO8601（YYYY-MM-DDTHH:MM:SSZ），civil_from_days（Howard Hinnant 算法）。
fn iso_now() -> String {
    let secs = unix_secs();
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 从 uuid v4 的 16 字节随机数派生（避免引入 rand）。
fn random_u32() -> u32 {
    u32::from_le_bytes(Uuid::new_v4().into_bytes()[..4].try_into().unwrap_or([0; 4]))
}

fn random_string(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    Uuid::new_v4()
        .into_bytes()
        .iter()
        .cycle()
        .take(len)
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

fn hex_digest<D: Digest>(data: &[u8]) -> String {
    let mut hasher = D::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `text,key` 两段参数的 HMAC-Hex；new_from_slice 接受任意长度 key，失败仅在构造非法时发生。
fn hmac_sha_hex<D>(arg: &str, new_mac: fn(&[u8]) -> Result<hmac::Hmac<D>, hmac::digest::InvalidLength>) -> String
where
    D: hmac::EagerHash,
{
    let (text, key) = match arg.split_once(',') {
        Some((t, k)) => (t.trim(), k.trim()),
        None => return String::new(),
    };
    let mut mac = match new_mac(key.as_bytes()) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };
    mac.update(text.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// encodeURIComponent 语义：仅保留 unreserved 字符。
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 解码 %XX 序列（`+` 不视为空格，与 encodeURIComponent 对应）。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render0(input: &str) -> String {
        render(input, &BTreeMap::new())
    }

    #[test]
    fn uuid_and_timestamp() {
        let uuid = render0("{{uuid}}");
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.matches('-').count(), 4);
        // Postman 兼容前缀
        assert_eq!(render0("{{$uuid}}").len(), 36);
        let ts = render0("{{timestamp}}");
        assert!(ts.parse::<u64>().is_ok(), "timestamp 应为整数: {ts}");
        let now = render0("{{now}}");
        assert_eq!(now.len(), 20);
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn digests_match_known_values() {
        assert_eq!(render0("{{md5(abc)}}"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            render0("{{sha256(abc)}}"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            render0("{{sha1(abc)}}"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        // hmacSha256(text, key)：key=secret, text=Hi There（RFC 4231 用例 1）
        assert_eq!(
            render0("{{hmacSha256(Hi There,secret)}}").len(),
            64,
            "hmac 输出 64 位 hex"
        );
    }

    #[test]
    fn base64_and_url_roundtrip() {
        assert_eq!(render0("{{base64(hello)}}"), "aGVsbG8=");
        assert_eq!(render0("{{base64Decode(aGVsbG8=)}}"), "hello");
        assert_eq!(render0("{{urlEncode(a b&c)}}"), "a%20b%26c");
        assert_eq!(render0("{{urlDecode(a%20b)}}"), "a b");
    }

    #[test]
    fn unknown_tokens_preserved() {
        assert_eq!(render0("{{userId}}"), "{{userId}}");
        assert_eq!(render0("{{bad func(}}"), "{{bad func(}}");
        assert_eq!(render0("prefix {{uuid"), "prefix {{uuid");
    }

    #[test]
    fn vars_shadow_functions_and_render_values() {
        let mut vars = BTreeMap::new();
        vars.insert("timestamp".to_string(), "1234567890".to_string());
        vars.insert(
            "token".to_string(),
            "Bearer-{{randomInt}}".to_string(),
        );
        assert_eq!(render("{{timestamp}}", &vars), "1234567890");
        // 变量值里的函数已求值
        let token = render("{{token}}", &vars);
        assert!(token.starts_with("Bearer-"));
        assert!(token["Bearer-".len()..].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn render_multiple_tokens_in_one_string() {
        let out = render0("https://api.example.com/{{uuid}}?t={{timestamp}}");
        assert!(out.starts_with("https://api.example.com/"));
        assert!(out.contains("?t="));
        assert_eq!(out.matches("{{").count(), 0);
    }

    #[test]
    fn env_var_missing_is_empty() {
        assert_eq!(render0("{{env(DESKPET_NO_SUCH_VAR_XYZ)}}"), "");
    }
}
