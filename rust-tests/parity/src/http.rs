//! A loopback-only HTTP mock that records bounded raw request and response bytes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 512 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const DEFAULT_SERVER_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpExchange {
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

/// A configured sequence is served one request at a time, in order.
pub struct MockHttpServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<std::io::Result<Vec<HttpExchange>>>>,
    expected_exchanges: usize,
}

impl MockHttpServer {
    pub fn start(responses: &[Vec<u8>]) -> std::io::Result<Self> {
        Self::start_with_timeout(responses, DEFAULT_SERVER_TIMEOUT)
    }

    pub fn start_with_timeout(responses: &[Vec<u8>], timeout: Duration) -> std::io::Result<Self> {
        if responses.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP fixture requires at least one configured response",
            ));
        }
        if responses
            .iter()
            .any(|response| response.len() > MAX_RESPONSE_BYTES)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP fixture response exceeds the bounded response limit",
            ));
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let responses = responses.to_vec();
        let expected_exchanges = responses.len();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            let io_timeout = timeout
                .min(Duration::from_secs(1))
                .max(Duration::from_millis(1));
            let mut exchanges = Vec::with_capacity(responses.len());
            while exchanges.len() < responses.len() {
                if thread_stop.load(Ordering::Acquire) {
                    return Ok(exchanges);
                }
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "HTTP mock deadline expired before all exchanges arrived",
                    ));
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(io_timeout))?;
                        stream.set_write_timeout(Some(io_timeout))?;
                        let request = read_http_request(&mut stream)?;
                        let response = responses[exchanges.len()].clone();
                        stream.write_all(&response)?;
                        stream.flush()?;
                        exchanges.push(HttpExchange { request, response });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(exchanges)
        });
        Ok(Self {
            address,
            stop,
            join: Some(join),
            expected_exchanges,
        })
    }

    pub fn address(&self) -> std::net::SocketAddr {
        self.address
    }

    pub fn finish(mut self) -> std::io::Result<Vec<HttpExchange>> {
        let exchanges = self
            .join
            .take()
            .expect("mock join handle exists")
            .join()
            .map_err(|_| std::io::Error::other("HTTP mock panicked"))??;
        if exchanges.len() != self.expected_exchanges {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP fixture did not receive its configured exchanges",
            ));
        }
        Ok(exchanges)
    }
}

impl Drop for MockHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "HTTP request ended before headers were complete",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_REQUEST_BYTES || bytes.len() > MAX_HEADER_BYTES + MAX_BODY_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request exceeds the bounded request limit",
            ));
        }
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if end + 4 > MAX_HEADER_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "HTTP request headers exceed the bounded header limit",
                ));
            }
            break end;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request headers exceed the bounded header limit",
            ));
        }
    };

    let (content_length, has_transfer_encoding) = parse_headers(&bytes[..header_end])?;
    if has_transfer_encoding {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported HTTP transfer framing",
        ));
    }
    let body_start = header_end + 4;
    if content_length == 0 && bytes.len() > body_start {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unexpected HTTP bytes without a declared body",
        ));
    }
    let total = body_start.checked_add(content_length).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "HTTP length overflow")
    })?;
    if content_length > MAX_BODY_BYTES || total > MAX_REQUEST_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "HTTP request body exceeds the bounded body limit",
        ));
    }
    while bytes.len() < total {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "HTTP request body ended before Content-Length",
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > total || bytes.len() > MAX_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request body exceeds Content-Length",
            ));
        }
    }
    bytes.truncate(total);
    Ok(bytes)
}

fn parse_headers(headers: &[u8]) -> std::io::Result<(usize, bool)> {
    let text = std::str::from_utf8(headers).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "HTTP headers are not valid UTF-8",
        )
    })?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split(' ');
    let method = request_parts.next().unwrap_or_default();
    let target = request_parts.next().unwrap_or_default();
    let version = request_parts.next().unwrap_or_default();
    if method.is_empty()
        || target.is_empty()
        || request_parts.next().is_some()
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "malformed HTTP request line",
        ));
    }
    let mut content_length = None;
    let mut transfer_encoding = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "malformed HTTP header")
        })?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "malformed HTTP header name",
            ));
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            transfer_encoding = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() || value.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "duplicate or malformed HTTP Content-Length",
                ));
            }
            let length = value.parse::<usize>().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "malformed HTTP Content-Length",
                )
            })?;
            content_length = Some(length);
        }
    }
    Ok((content_length.unwrap_or(0), transfer_encoding))
}

pub fn record_exchange(request: &[u8], response: &[u8]) -> HttpExchange {
    HttpExchange {
        request: request.to_vec(),
        response: response.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_CONTENT: &[u8] = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n";

    #[test]
    fn records_raw_loopback_exchange() {
        let server = MockHttpServer::start(&[NO_CONTENT.to_vec()]).unwrap();
        let mut client = TcpStream::connect(server.address()).unwrap();
        client
            .write_all(b"POST /mock HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc")
            .unwrap();
        let exchanges = server.finish().unwrap();
        assert_eq!(exchanges.len(), 1);
        assert!(exchanges[0].request.ends_with(b"abc"));
        assert!(exchanges[0].response.starts_with(b"HTTP/1.1 204"));
    }

    #[test]
    fn serves_configured_exchange_sequence_in_order() {
        let server = MockHttpServer::start(&[NO_CONTENT.to_vec(), NO_CONTENT.to_vec()]).unwrap();
        let mut first = TcpStream::connect(server.address()).unwrap();
        first
            .write_all(b"GET /one HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut second = TcpStream::connect(server.address()).unwrap();
        second
            .write_all(b"GET /two HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let exchanges = server.finish().unwrap();
        assert_eq!(exchanges.len(), 2);
        assert!(exchanges[0].request.starts_with(b"GET /one"));
        assert!(exchanges[1].request.starts_with(b"GET /two"));
    }

    #[test]
    fn rejects_an_empty_exchange_sequence() {
        let error = match MockHttpServer::start(&[]) {
            Ok(_) => panic!("empty sequence must be explicit"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("at least one"));
    }

    #[test]
    fn rejects_transfer_encoding_and_malformed_content_length() {
        assert!(
            parse_headers(b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked")
                .unwrap()
                .1
        );
        assert!(parse_headers(b"GET / HTTP/1.1\r\nContent-Length: nope").is_err());
    }

    #[test]
    fn drop_stops_a_mock_that_received_no_request() {
        let server =
            MockHttpServer::start_with_timeout(&[NO_CONTENT.to_vec()], Duration::from_millis(20))
                .unwrap();
        drop(server);
    }
}
