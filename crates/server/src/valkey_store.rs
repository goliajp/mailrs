use redis::aio::ConnectionManager;
use redis::Client;

pub async fn create_connection(url: &str) -> Result<ConnectionManager, redis::RedisError> {
    let client = Client::open(url)?;
    ConnectionManager::new(client).await
}
