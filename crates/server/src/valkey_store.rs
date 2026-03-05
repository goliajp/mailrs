use redis::aio::ConnectionManager;
use redis::Client;

pub async fn create_connection(url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = Client::open(url)?;
    ConnectionManager::new(client).await
}

/// parse a redis URL to validate it without connecting
pub fn validate_url(url: &str) -> Result<(), String> {
    Client::open(url).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_valid() {
        assert!(validate_url("redis://localhost:6379").is_ok());
        assert!(validate_url("redis://127.0.0.1:6379/0").is_ok());
    }

    #[test]
    fn validate_url_invalid() {
        assert!(validate_url("not-a-url").is_err());
    }

    #[test]
    fn validate_url_with_password() {
        assert!(validate_url("redis://:password@localhost:6379").is_ok());
    }
}
