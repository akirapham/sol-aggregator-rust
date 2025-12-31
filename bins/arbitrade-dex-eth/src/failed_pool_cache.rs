use dashmap::DashMap;
use lazy_static::lazy_static;
use std::time::{SystemTime, UNIX_EPOCH};

// Cache failing pools for 3 hours
const CACHE_DURATION_SECS: u64 = 3 * 60 * 60;

lazy_static! {
    static ref FAILED_POOLS: DashMap<String, u64> = DashMap::new();
}

/// Check if a pool is in the failed cache and the cache is still valid
pub fn is_pool_failed(pool_address: &str) -> bool {
    if let Some(timestamp) = FAILED_POOLS.get(pool_address) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now - *timestamp < CACHE_DURATION_SECS {
            return true;
        } else {
            // Cache expired, remove it (lazily)
            // We don't remove here to avoid deadlock if we were to upgrade reference,
            // but we return false so it will be retried.
            // The next mark_pool_failed will overwrite it.
            return false;
        }
    }
    false
}

/// Mark a pool as failed with current timestamp
pub fn mark_pool_failed(pool_address: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    FAILED_POOLS.insert(pool_address.to_string(), now);
}

/// Clear the cache (useful for tests)
#[allow(dead_code)]
pub fn clear_cache() {
    FAILED_POOLS.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_failed_pool_cache() {
        clear_cache();
        let pool = "0x123";

        assert!(!is_pool_failed(pool));

        mark_pool_failed(pool);
        assert!(is_pool_failed(pool));
    }
}
