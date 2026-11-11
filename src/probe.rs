//! Асинхронная TCP-проверка доступности серверов.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// Таймаут одной попытки TCP-подключения.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Статус проверки сервера.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProbeStatus {
    /// Проверка ещё не запускалась.
    #[default]
    Unknown,
    /// Проверка выполняется прямо сейчас.
    Checking,
    /// Сервер отвечает; время отклика в миллисекундах.
    Online { rtt_ms: u128 },
    /// Сервер недоступен; причина.
    Offline { reason: String },
}

impl ProbeStatus {
    /// Короткая подпись для строки сервера.
    pub fn short_label(&self) -> String {
        match self {
            Self::Unknown => "не проверялся".to_string(),
            Self::Checking => "проверка…".to_string(),
            Self::Online { rtt_ms } => format!("{rtt_ms} мс"),
            Self::Offline { .. } => "недоступен".to_string(),
        }
    }
}

/// Сообщение из фонового потока проверки в UI.
#[derive(Debug)]
pub struct ProbeMessage {
    /// Идентификатор проверяемого сервера.
    pub server_id: u64,
    /// Время отклика в мс либо текст ошибки.
    pub result: Result<u128, String>,
}

/// Проверяет TCP-доступность `host:port` с таймаутом.
pub fn probe_host(host: &str, port: u16, timeout: Duration) -> Result<u128, String> {
    let endpoint = format!("{}:{}", host.trim(), port);
    let addresses: Vec<SocketAddr> = endpoint
        .to_socket_addrs()
        .map_err(|error| format!("не удалось разрешить адрес: {error}"))?
        .collect();

    if addresses.is_empty() {
        return Err("адрес не разрешился".to_string());
    }

    let mut last_error = String::from("нет ответа");
    for address in addresses {
        let started = Instant::now();
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                drop(stream);
                return Ok(started.elapsed().as_millis());
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn probes_open_local_port() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("свободный порт");
        let port = listener.local_addr().expect("адрес").port();
        let acceptor = std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let result = probe_host("127.0.0.1", port, Duration::from_secs(3));
        assert!(result.is_ok(), "ожидался успех: {result:?}");
        assert!(
            result.expect("успех") < 5_000,
            "отклик должен быть меньше 5 с"
        );

        let _ = acceptor.join();
    }

    #[test]
    fn reports_closed_port() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("свободный порт");
        let port = listener.local_addr().expect("адрес").port();
        drop(listener);

        let result = probe_host("127.0.0.1", port, Duration::from_millis(500));
        assert!(result.is_err(), "закрытый порт должен давать ошибку");
    }

    #[test]
    fn short_labels_describe_status() {
        assert_eq!(ProbeStatus::Unknown.short_label(), "не проверялся");
        assert_eq!(ProbeStatus::Checking.short_label(), "проверка…");
        assert_eq!(ProbeStatus::Online { rtt_ms: 42 }.short_label(), "42 мс");
        assert_eq!(
            ProbeStatus::Offline {
                reason: "отказ".to_string()
            }
            .short_label(),
            "недоступен"
        );
    }
}
