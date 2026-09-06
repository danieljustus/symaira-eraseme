//! A loopback-only HTTP mock that records raw request and response bytes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpExchange {
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

pub struct MockHttpServer {
    address: std::net::SocketAddr,
    join: Option<thread::JoinHandle<std::io::Result<HttpExchange>>>,
}

impl MockHttpServer {
    pub fn start(response: &[u8]) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        let response = response.to_vec();
        let join = thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let request = read_http_request(&mut stream)?;
            stream.write_all(&response)?;
            stream.flush()?;
            Ok(HttpExchange { request, response })
        });
        Ok(Self {
            address,
            join: Some(join),
        })
    }

    pub fn address(&self) -> std::net::SocketAddr {
        self.address
    }

    pub fn finish(mut self) -> std::io::Result<HttpExchange> {
        self.join
            .take()
            .expect("mock join handle exists")
            .join()
            .map_err(|_| std::io::Error::other("HTTP mock panicked"))?
    }
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let mut content_length = None;
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if content_length.is_none()
            && let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .or(Some(0));
        }
        if let (Some(header_end), Some(length)) = (
            bytes.windows(4).position(|window| window == b"\r\n\r\n"),
            content_length,
        ) && bytes.len() >= header_end + 4 + length
        {
            break;
        }
    }
    Ok(bytes)
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

    #[test]
    fn records_raw_loopback_exchange() {
        let server =
            MockHttpServer::start(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").unwrap();
        let mut client = TcpStream::connect(server.address()).unwrap();
        client
            .write_all(b"POST /mock HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc")
            .unwrap();
        let exchange = server.finish().unwrap();
        assert!(exchange.request.ends_with(b"abc"));
        assert!(exchange.response.starts_with(b"HTTP/1.1 204"));
    }
}
