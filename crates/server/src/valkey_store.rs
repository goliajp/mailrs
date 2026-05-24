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
#[path = "valkey_store_tests.rs"]
mod tests;
