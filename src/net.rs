//! Plain HTTP(S) and websockets through Windows' own WinHTTP (system TLS and proxy settings),
//! so Sticky needs no TLS library. Used for Home Assistant.

use std::ffi::c_void;
use windows_sys::Win32::Networking::WinHttp::*;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A parsed `http(s)://host[:port][/base]` address.
#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// Path with no trailing slash ("" for the root).
    pub base: String,
}

impl Url {
    pub fn parse(s: &str) -> Option<Url> {
        let s = s.trim().trim_end_matches('/');
        let (secure, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://") {
            (false, r)
        } else {
            (true, s)
        };
        let (hostport, base) = match rest.find('/') {
            Some(i) => (&rest[..i], rest[i..].to_string()),
            None => (rest, String::new()),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (hostport.to_string(), if secure { 443 } else { 80 }),
        };
        if host.is_empty() || host.contains(' ') {
            return None;
        }
        Some(Url { secure, host, port, base })
    }
    pub fn origin(&self) -> String {
        let def = if self.secure { 443 } else { 80 };
        let port = if self.port == def { String::new() } else { format!(":{}", self.port) };
        format!("{}://{}{}", if self.secure { "https" } else { "http" }, self.host, port)
    }
}

/// An open WinHTTP handle, closed on drop.
struct H(*mut c_void);
unsafe impl Send for H {}
unsafe impl Sync for H {}
impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn last_error(what: &str) -> String {
    let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    let why = match e {
        12002 => "timed out".to_string(),
        12007 => "address not found".to_string(),
        12029 | 12030 => "couldn't connect".to_string(),
        12175 => "secure connection failed".to_string(),
        _ => format!("error {e}"),
    };
    format!("{what}: {why}")
}

fn session() -> Result<H, String> {
    let agent = wide(concat!("Sticky/", env!("CARGO_PKG_VERSION")));
    let s = unsafe { WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0) };
    if s.is_null() {
        return Err(last_error("open"));
    }
    unsafe { WinHttpSetTimeouts(s, 10_000, 10_000, 15_000, 30_000) };
    Ok(H(s))
}

/// Connection + request handles for one request (the session must outlive them).
struct Req {
    _session: H,
    _connect: H,
    req: H,
}

fn open(url: &Url, method: &str, path: &str) -> Result<Req, String> {
    let session = session()?;
    let host = wide(&url.host);
    let c = unsafe { WinHttpConnect(session.0, host.as_ptr(), url.port, 0) };
    if c.is_null() {
        return Err(last_error("connect"));
    }
    let connect = H(c);
    let verb = wide(method);
    let obj = wide(&format!("{}{}", url.base, path));
    let flags = if url.secure { WINHTTP_FLAG_SECURE } else { 0 };
    let r = unsafe { WinHttpOpenRequest(connect.0, verb.as_ptr(), obj.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags) };
    if r.is_null() {
        return Err(last_error("request"));
    }
    Ok(Req { _session: session, _connect: connect, req: H(r) })
}

fn send(r: &Req, headers: &str, body: &[u8]) -> Result<u32, String> {
    let h = wide(headers);
    let ok = unsafe {
        WinHttpSendRequest(r.req.0, if headers.is_empty() { std::ptr::null() } else { h.as_ptr() }, u32::MAX, body.as_ptr() as *const c_void, body.len() as u32, body.len() as u32, 0)
    };
    if ok == 0 {
        return Err(last_error("send"));
    }
    if unsafe { WinHttpReceiveResponse(r.req.0, std::ptr::null_mut()) } == 0 {
        return Err(last_error("response"));
    }
    let mut code: u32 = 0;
    let mut len = 4u32;
    unsafe {
        WinHttpQueryHeaders(r.req.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), &mut code as *mut u32 as *mut c_void, &mut len, std::ptr::null_mut());
    }
    Ok(code)
}

fn read_all(r: &Req, max: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let mut n = 0u32;
        if unsafe { WinHttpReadData(r.req.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut n) } == 0 {
            return Err(last_error("read"));
        }
        if n == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&buf[..n as usize]);
        if out.len() > max {
            return Err("response too large".into());
        }
    }
}

fn auth_header(token: Option<&str>) -> String {
    token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default()
}

/// GET a path (or a full URL) and return the body. `token` adds a bearer header.
pub fn get(url: &Url, path: &str, token: Option<&str>) -> Result<Vec<u8>, String> {
    let r = open(url, "GET", path)?;
    let code = send(&r, &auth_header(token), &[])?;
    let body = read_all(&r, 40 << 20)?;
    match code {
        200..=299 => Ok(body),
        401 | 403 => Err("the token was refused".into()),
        404 => Err("not found".into()),
        c => Err(format!("HTTP {c}")),
    }
}

/// GET any absolute URL, with the token only when it goes to `trusted`.
pub fn get_any(full: &str, trusted: &Url, token: &str) -> Result<Vec<u8>, String> {
    if full.starts_with('/') {
        return get(trusted, full, Some(token));
    }
    let u = Url::parse(full).ok_or("bad picture address")?;
    let path = full.find("://").and_then(|i| full[i + 3..].find('/').map(|j| full[i + 3 + j..].to_string())).unwrap_or_else(|| "/".into());
    let same = u.host == trusted.host && u.port == trusted.port;
    let base = Url { base: String::new(), ..u };
    get(&base, &path, same.then_some(token))
}

/// A websocket. `recv` and `send` may run on different threads at the same time; `close` from
/// another thread makes a waiting `recv` return an error.
pub struct Ws {
    _req: Req,
    ws: H,
}

impl Ws {
    pub fn connect(url: &Url, path: &str) -> Result<Ws, String> {
        let r = open(url, "GET", path)?;
        if unsafe { WinHttpSetOption(r.req.0, WINHTTP_OPTION_UPGRADE_TO_WEB_SOCKET, std::ptr::null(), 0) } == 0 {
            return Err(last_error("websocket"));
        }
        let code = send(&r, "", &[])?;
        if code != 101 {
            return Err(match code {
                403 => "blocked (403): is the address behind a sign-in page?".into(),
                404 => "no Home Assistant at that address (404)".into(),
                c => format!("websocket refused (HTTP {c})"),
            });
        }
        let ws = unsafe { WinHttpWebSocketCompleteUpgrade(r.req.0, 0) };
        if ws.is_null() {
            return Err(last_error("websocket upgrade"));
        }
        Ok(Ws { _req: r, ws: H(ws) })
    }

    pub fn send(&self, text: &str) -> Result<(), String> {
        let e = unsafe { WinHttpWebSocketSend(self.ws.0, WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE, text.as_ptr() as *mut c_void, text.len() as u32) };
        if e != 0 { Err(format!("send failed ({e})")) } else { Ok(()) }
    }

    /// The next whole text message.
    pub fn recv(&self) -> Result<String, String> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut n = 0u32;
            let mut kind: WINHTTP_WEB_SOCKET_BUFFER_TYPE = 0;
            let e = unsafe { WinHttpWebSocketReceive(self.ws.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut n, &mut kind) };
            if e != 0 {
                return Err(format!("connection lost ({e})"));
            }
            out.extend_from_slice(&buf[..n as usize]);
            if out.len() > 64 << 20 {
                return Err("message too large".into());
            }
            match kind {
                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE | WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => {
                    return Ok(String::from_utf8_lossy(&out).into_owned());
                }
                WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => return Err("closed by the server".into()),
                _ => {} // a fragment: keep reading
            }
        }
    }

    pub fn close(&self) {
        unsafe { WinHttpWebSocketClose(self.ws.0, 1000, std::ptr::null_mut(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls() {
        let u = Url::parse("https://ha.example.com/").unwrap();
        assert_eq!((u.secure, u.host.as_str(), u.port, u.base.as_str()), (true, "ha.example.com", 443, ""));
        let u = Url::parse("http://192.168.1.5:8123").unwrap();
        assert_eq!((u.secure, u.port), (false, 8123));
        assert_eq!(u.origin(), "http://192.168.1.5:8123");
        let u = Url::parse("ha.example.com/sub").unwrap();
        assert_eq!((u.secure, u.base.as_str()), (true, "/sub"));
        assert!(Url::parse("").is_none());
    }
}
