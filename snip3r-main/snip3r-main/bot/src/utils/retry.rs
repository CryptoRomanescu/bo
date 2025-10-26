// =============================================================================
// utils/retry.rs
// =============================================================================
// Helper: call_rpc_with_retry - timeout + exponential backoff + full jitter
// Usage: wrap every external RPC call with this to get robust retries.
//
// Requires in Cargo.toml:
// rand = "0.8"
// anyhow = "1.0"
// tokio = { version = "...", features = ["time"] }
// =============================================================================

use anyhow::Result;
use rand::Rng;
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// Default RPC timeout per attempt (ms)
pub const RPC_TIMEOUT_MS: u64 = 1500;
/// Max attempts including first
pub const RPC_MAX_RETRIES: usize = 3;
/// Base backoff (ms)
const BACKOFF_BASE_MS: u64 = 50;
/// Maximum backoff cap (ms)
const BACKOFF_MAX_MS: u64 = 5000;

/// Call async closure `op` with standardized retry/backoff logic.
/// `op` must be a zero-arg closure that returns a boxed future -> Result<T>
///
/// # Arguments
/// * `op` - Async operation to retry
///
/// # Returns
/// * `Ok(T)` - Success result from the operation
/// * `Err` - Error after all retries exhausted
///
/// # Example
/// ```no_run
/// use h_5n1p3r::utils::retry::call_rpc_with_retry;
///
/// async fn example() -> anyhow::Result<String> {
///     call_rpc_with_retry(|| async {
///         // Your RPC call here
///         Ok("result".to_string())
///     }).await
/// }
/// ```
pub async fn call_rpc_with_retry<F, Fut, T>(op: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut attempt = 0;
    
    loop {
        attempt += 1;

        // Apply timeout to the operation
        let result = match timeout(Duration::from_millis(RPC_TIMEOUT_MS), op()).await {
            Ok(res) => res,
            Err(_) => {
                if attempt >= RPC_MAX_RETRIES {
                    return Err(anyhow::anyhow!("RPC timeout after {} attempts", RPC_MAX_RETRIES));
                }
                
                // Calculate exponential backoff with full jitter
                let backoff_ms = calculate_backoff_with_jitter(attempt);
                tracing::debug!(
                    "RPC call timed out (attempt {}/{}). Retrying in {}ms...",
                    attempt,
                    RPC_MAX_RETRIES,
                    backoff_ms
                );
                sleep(Duration::from_millis(backoff_ms)).await;
                continue;
            }
        };

        match result {
            Ok(value) => return Ok(value),
            Err(e) => {
                if attempt >= RPC_MAX_RETRIES {
                    return Err(e);
                }

                // Calculate exponential backoff with full jitter
                let backoff_ms = calculate_backoff_with_jitter(attempt);
                tracing::debug!(
                    "RPC call failed (attempt {}/{}): {}. Retrying in {}ms...",
                    attempt,
                    RPC_MAX_RETRIES,
                    e,
                    backoff_ms
                );
                sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}

/// Calculate exponential backoff with full jitter
///
/// Formula: random(0, min(BACKOFF_MAX_MS, BACKOFF_BASE_MS * 2^attempt))
///
/// # Arguments
/// * `attempt` - Current retry attempt number (1-indexed)
///
/// # Returns
/// Backoff duration in milliseconds
fn calculate_backoff_with_jitter(attempt: usize) -> u64 {
    let mut rng = rand::thread_rng();

    // Calculate exponential backoff: base * 2^(attempt-1)
    let exp_backoff = BACKOFF_BASE_MS
        .saturating_mul(2_u64.saturating_pow((attempt.saturating_sub(1)) as u32));

    // Cap at maximum backoff
    let capped_backoff = exp_backoff.min(BACKOFF_MAX_MS);

    // Apply full jitter: random value between 0 and capped_backoff
    rng.gen_range(0..=capped_backoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        // Test that backoff increases exponentially
        let backoff1 = calculate_backoff_with_jitter(1);
        let backoff2 = calculate_backoff_with_jitter(2);
        let backoff3 = calculate_backoff_with_jitter(3);

        // All should be within valid ranges
        assert!(backoff1 <= BACKOFF_BASE_MS);
        assert!(backoff2 <= BACKOFF_BASE_MS * 2);
        assert!(backoff3 <= BACKOFF_BASE_MS * 4);

        // All should be capped at max
        let backoff_large = calculate_backoff_with_jitter(20);
        assert!(backoff_large <= BACKOFF_MAX_MS);
    }

    #[tokio::test]
    async fn test_retry_success_on_first_attempt() {
        let result = call_rpc_with_retry(|| async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_exhaustion() {
        let result: Result<i32> = call_rpc_with_retry(|| async { 
            Err(anyhow::anyhow!("Permanent failure")) 
        })
        .await;
        assert!(result.is_err());
    }
}
