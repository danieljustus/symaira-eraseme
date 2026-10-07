//! Retry EINTR without renewing the caller's existing socket-read budget.
use std::io;
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// Re-arm the socket read timeout. Once the peer has reset the connection,
/// macOS rejects `SO_RCVTIMEO` with EINVAL although the response bytes already
/// received stay readable; the previously armed timeout, never larger than the
/// original budget, then remains in force and the read drains those bytes.
pub fn rearm(stream: &TcpStream, timeout: Duration) -> io::Result<()> {
    match stream.set_read_timeout(Some(timeout)) {
        Err(error) if cfg!(target_os = "macos") && error.raw_os_error() == Some(22) => Ok(()),
        result => result,
    }
}

pub fn retry_until<T>(
    deadline: Instant,
    mut read: impl FnMut(std::time::Duration) -> io::Result<T>,
) -> io::Result<T> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "HTTP read deadline elapsed"))?;
        match read(remaining) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn rearm_after_peer_reset_still_drains_the_buffered_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(b"request bytes the server never reads")
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        server.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
        // Closing with unread input makes the kernel send RST.
        drop(server);
        std::thread::sleep(Duration::from_millis(200));
        rearm(&client, Duration::from_secs(2)).unwrap();
        if cfg!(target_os = "macos") {
            // The reset socket rejects SO_RCVTIMEO directly; rearm must not.
            let raw = client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap_err();
            assert_eq!(raw.raw_os_error(), Some(22));
            let mut chunk = [0; 64];
            let count = client.read(&mut chunk).unwrap();
            assert_eq!(&chunk[..count], b"HTTP/1.1 200 OK\r\n\r\n");
        }
    }

    #[test]
    fn rearm_preserves_every_other_error() {
        // A fresh, connected socket rejects a zero timeout on every platform.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let error = rearm(&client, Duration::ZERO).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(error.raw_os_error(), None);
    }

    #[test]
    fn interrupted_socket_read_retains_bytes_and_remaining_budget() {
        let budget = Duration::from_secs(4);
        let mut attempts = 0;
        let mut previous = budget;
        let result = retry_until(Instant::now() + budget, |remaining| {
            assert!(remaining <= previous && remaining > Duration::ZERO);
            previous = remaining;
            attempts += 1;
            if attempts <= 2 {
                Err(io::Error::from(io::ErrorKind::Interrupted))
            } else {
                Ok(b"HTTP/1.1 200 OK\r\n".to_vec())
            }
        })
        .unwrap();
        assert_eq!(result, b"HTTP/1.1 200 OK\r\n");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn expired_budget_does_not_attempt_or_renew_a_read() {
        let error = retry_until(Instant::now(), |_| -> io::Result<()> {
            panic!("expired read must not execute");
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn connection_reset_is_preserved_without_retry() {
        let mut attempts = 0;
        let error = retry_until(
            Instant::now() + Duration::from_secs(4),
            |_| -> io::Result<()> {
                attempts += 1;
                Err(io::Error::from(io::ErrorKind::ConnectionReset))
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
        assert_eq!(attempts, 1);
    }
}
