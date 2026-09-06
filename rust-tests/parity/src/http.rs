//! A loopback-only HTTP mock that records raw request and response bytes.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpExchange {
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

pub struct MockHttpServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<std::io::Result<HttpExchange>>>,
}

impl MockHttpServer {
    pub fn start(response: &[u8]) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let response = response.to_vec();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let join = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if thread_stop.load(Ordering::Acquire) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "HTTP mock stopped before receiving a request",
                    ));
                }
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "HTTP mock timed out waiting for a request",
                    ));
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                        let request = read_http_request(&mut stream)?;
                        stream.write_all(&response)?;
                        stream.flush()?;
                        return Ok(HttpExchange { request, response });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error),
                }
            }
        });
        Ok(Self {
            address,
            stop,
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

    #[test]
    fn drop_stops_a_mock_that_received_no_request() {
        let server = MockHttpServer::start(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
        drop(server);
    }
}
