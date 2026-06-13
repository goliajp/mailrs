//! ip/domain reputation engine
//!
//! tracks sending metrics per domain: delivery rate, bounce rate,
//! complaint rate. surfaces alerts when thresholds are exceeded.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DomainReputation {
    pub domain: String,
    pub total_sent: i64,
    pub delivered: i64,
    pub bounced: i64,
    pub suppressed: i64,
    pub delivery_rate: f64,
    pub bounce_rate: f64,
    pub health: &'static str, // "good", "warning", "critical"
}

const BOUNCE_RATE_WARNING: f64 = 0.05; // 5%
const BOUNCE_RATE_CRITICAL: f64 = 0.10; // 10%

fn classify_health(bounce_rate: f64) -> &'static str {
    if bounce_rate >= BOUNCE_RATE_CRITICAL {
        "critical"
    } else if bounce_rate >= BOUNCE_RATE_WARNING {
        "warning"
    } else {
        "good"
    }
}

/// compute reputation metrics from the outbound queue and suppression list
pub async fn compute_reputation(pool: &crate::pg::BackendPool) -> Vec<DomainReputation> {
    // sent/delivered/bounced per sender domain (last 30 days)
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT \
           SPLIT_PART(sender, '@', 2) as domain, \
           COUNT(*) as total, \
           COUNT(CASE WHEN status = 'delivered' THEN 1 END) as delivered, \
           COUNT(CASE WHEN status = 'bounced' THEN 1 END) as bounced \
         FROM outbound_queue \
         WHERE created_at > NOW() - INTERVAL '30 days' \
           AND SPLIT_PART(sender, '@', 2) != '' \
         GROUP BY SPLIT_PART(sender, '@', 2) \
         ORDER BY total DESC \
         LIMIT 50",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    // suppressed count per domain
    let suppressed_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT SPLIT_PART(email, '@', 2) as domain, COUNT(*) \
         FROM suppression_list \
         GROUP BY SPLIT_PART(email, '@', 2)",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let suppressed_map: std::collections::HashMap<String, i64> =
        suppressed_rows.into_iter().collect();

    rows.into_iter()
        .map(|(domain, total, delivered, bounced)| {
            let suppressed = suppressed_map.get(&domain).copied().unwrap_or(0);
            let delivery_rate = if total > 0 {
                delivered as f64 / total as f64
            } else {
                1.0
            };
            let bounce_rate = if total > 0 {
                bounced as f64 / total as f64
            } else {
                0.0
            };
            DomainReputation {
                domain,
                total_sent: total,
                delivered,
                bounced,
                suppressed,
                delivery_rate,
                bounce_rate,
                health: classify_health(bounce_rate),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_classification() {
        assert_eq!(classify_health(0.0), "good");
        assert_eq!(classify_health(0.03), "good");
        assert_eq!(classify_health(0.05), "warning");
        assert_eq!(classify_health(0.08), "warning");
        assert_eq!(classify_health(0.10), "critical");
        assert_eq!(classify_health(0.25), "critical");
    }
}
