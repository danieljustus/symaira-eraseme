//! Retry EINTR without renewing the caller's existing socket-read budget.
use std::io;
use std::time::Instant;

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
    use std::time::Duration;

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
