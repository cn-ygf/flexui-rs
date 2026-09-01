use flexui::{MainProxy, WindowCtx};

const MAX_RESPONSE_BYTES: usize = 16 * 1024;

pub(crate) fn start_request(ctx: &mut WindowCtx, ui: MainProxy) {
    let url = ctx.text("http_url").unwrap_or_default().trim().to_owned();
    if let Err(error) = validate_url(&url) {
        ctx.set_text("http_status", "Invalid URL");
        ctx.set_text("http_response", error);
        return;
    }

    ctx.set_enabled("http_go", false);
    ctx.set_text("http_go", "请求中");
    ctx.set_text("http_status", "Loading...");
    ctx.set_text(
        "http_response",
        format!("GET {url}\n\nWaiting for response..."),
    );

    std::thread::spawn(move || {
        let result = fetch(&url);
        let _ = ui.post(move |ctx| {
            ctx.set_enabled("http_go", true);
            ctx.set_text("http_go", "转到");
            match result {
                Ok(response) => {
                    ctx.set_text("http_status", response.summary);
                    ctx.set_text("http_response", response.content);
                }
                Err(error) => {
                    ctx.set_text("http_status", "Request failed");
                    ctx.set_text("http_response", error);
                }
            }
        });
    });
}

struct HttpResponse {
    summary: String,
    content: String,
}

fn validate_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("Enter a URL first.".to_owned());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only http:// and https:// URLs are supported.".to_owned());
    }
    Ok(())
}

fn fetch(url: &str) -> Result<HttpResponse, String> {
    let response = minreq::get(url)
        .with_timeout(20)
        .with_max_redirects(8)
        .with_header("User-Agent", concat!("FlexUI-Gallery/", env!("CARGO_PKG_VERSION")))
        .with_header("Accept-Encoding", "identity")
        .send_lazy()
        .map_err(|error| format!("GET {url}\n\n{error}"))?;
    let status = response.status_code;
    let reason = response.reason_phrase.clone();
    let content_type = response
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let mut bytes = Vec::with_capacity(16 * 1024);
    let mut truncated = false;
    for item in response {
        let (byte, remaining) =
            item.map_err(|error| format!("Could not read response:\n{error}"))?;
        if bytes.len() == MAX_RESPONSE_BYTES {
            truncated = true;
            break;
        }
        bytes.reserve(remaining.min(16 * 1024));
        bytes.push(byte);
    }
    let body = String::from_utf8_lossy(&bytes);
    let suffix = if truncated {
        format!(
            "\n\n[Response truncated at {} KiB]",
            MAX_RESPONSE_BYTES / 1024
        )
    } else {
        String::new()
    };
    let content = format!(
        "HTTP {} {}\nContent-Type: {}\nReceived: {} bytes\n\n{}{}",
        status,
        reason,
        content_type,
        bytes.len(),
        body,
        suffix
    );
    Ok(HttpResponse {
        summary: format!("HTTP {status} · {} bytes", bytes.len()),
        content,
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::{fetch, validate_url, MAX_RESPONSE_BYTES};

    #[test]
    fn 只接受_http_和_https_url() {
        assert!(validate_url("https://example.com/path").is_ok());
        assert!(validate_url("http://127.0.0.1:8080/").is_ok());
        assert!(validate_url("file:///tmp/data.txt").is_err());
        assert!(validate_url("").is_err());
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn 后台请求读取_http_状态和正文() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let read = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).starts_with("GET /demo HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nhello flexui",
                )
                .unwrap();
        });

        let response = fetch(&format!("http://{address}/demo")).unwrap();
        server.join().unwrap();
        assert_eq!(response.summary, "HTTP 200 · 12 bytes");
        assert!(response.content.contains("Content-Type: text/plain"));
        assert!(response.content.ends_with("hello flexui"));
    }

    #[test]
    fn 后台请求限制响应正文大小() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).unwrap();
            let body = vec![b'x'; MAX_RESPONSE_BYTES + 1];
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        });

        let response = fetch(&format!("http://{address}/large")).unwrap();
        server.join().unwrap();
        assert_eq!(
            response.summary,
            format!("HTTP 200 · {MAX_RESPONSE_BYTES} bytes")
        );
        assert!(response.content.ends_with(&format!(
            "[Response truncated at {} KiB]",
            MAX_RESPONSE_BYTES / 1024
        )));
    }
}
