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

/// A configured sequence is served one request at a time, in order.
pub struct MockHttpServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<std::io::Result<Vec<HttpExchange>>>>,
    expected_exchanges: usize,
}

impl MockHttpServer {
    pub fn start(responses: &[Vec<u8>]) -> std::io::Result<Self> {
        if responses.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "HTTP fixture requires at least one configured response",
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
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut exchanges = Vec::with_capacity(responses.len());
            while exchanges.len() < responses.len() {
                if thread_stop.load(Ordering::Acquire) {
                    return Ok(exchanges);
                }
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!(
                            "HTTP mock timed out after {} of {} configured exchanges",
                            exchanges.len(),
                            responses.len()
                        ),
                    ));
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false)?;
                        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
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
                format!(
                    "HTTP fixture expected {} exchanges but received {}",
                    self.expected_exchanges,
                    exchanges.len()
                ),
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
    fn drop_stops_a_mock_that_received_no_request() {
        let server = MockHttpServer::start(&[NO_CONTENT.to_vec()]).unwrap();
        drop(server);
    }
}
