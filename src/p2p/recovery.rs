//! Network recovery manager for handling wake from sleep scenarios.
//!
//! Combines three recovery strategies:
//! 1. Wake detection - detect time jumps indicating sleep/wake
//! 2. Periodic refresh - every 60s when peers=0
//! 3. Immediate retry - after 5s of peers=0, retry every 10s

use std::time::{Duration, Instant};

/// Network recovery state and configuration.
pub struct NetworkRecovery {
    /// Whether we had peers before (for stale detection)
    had_peers_before: bool,
    /// When peers dropped to zero
    zero_peers_since: Option<Instant>,
    /// Last periodic refresh time
    last_refresh: Instant,
    /// Last heartbeat tick time (for wake detection)
    last_tick: Instant,
    /// Number of consecutive recovery attempts
    retry_count: u8,
    /// Whether we're in active recovery mode
    in_recovery: bool,

    // Configuration
    /// Threshold before triggering immediate recovery (default 5s)
    stale_threshold: Duration,
    /// Periodic refresh interval (default 60s)
    refresh_interval: Duration,
    /// Retry interval after failed recovery (default 10s)
    retry_interval: Duration,
    /// Max retries before backing off (default 5)
    max_retries: u8,
    /// Threshold for detecting sleep/wake (default 30s)
    sleep_threshold: Duration,
}

impl Default for NetworkRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkRecovery {
    /// Create a new NetworkRecovery with default settings.
    pub fn new() -> Self {
        Self {
            had_peers_before: false,
            zero_peers_since: None,
            last_refresh: Instant::now(),
            last_tick: Instant::now(),
            retry_count: 0,
            in_recovery: false,
            stale_threshold: Duration::from_secs(5),
            refresh_interval: Duration::from_secs(60),
            retry_interval: Duration::from_secs(10),
            max_retries: 5,
            sleep_threshold: Duration::from_secs(30),
        }
    }

    /// Detect if system just woke from sleep via time jump.
    /// Call this at the start of each heartbeat tick.
    /// Returns Some(WakeFromSleep) if a large time jump is detected.
    pub fn detect_wake(&mut self) -> Option<RecoveryReason> {
        let elapsed = self.last_tick.elapsed();
        self.last_tick = Instant::now();

        // If elapsed time is much larger than expected heartbeat interval,
        // the system likely just woke from sleep
        if elapsed > self.sleep_threshold {
            tracing::info!(
                "[NetworkRecovery] Detected wake from sleep: {}s elapsed (threshold: {}s)",
                elapsed.as_secs(),
                self.sleep_threshold.as_secs()
            );
            // Reset recovery state for fresh start
            self.zero_peers_since = None;
            self.retry_count = 0;
            self.in_recovery = false;
            return Some(RecoveryReason::WakeFromSleep {
                elapsed_secs: elapsed.as_secs(),
            });
        }
        None
    }

    /// Check if recovery should be triggered based on current peer count.
    ///
    /// Returns `Some(reason)` if `start_listening()` should be called,
    /// `None` otherwise.
    pub fn should_recover(&mut self, current_peer_count: usize) -> Option<RecoveryReason> {
        let now = Instant::now();

        // If we have peers, reset recovery state
        if current_peer_count > 0 {
            self.had_peers_before = true;
            self.zero_peers_since = None;
            self.retry_count = 0;
            self.in_recovery = false;
            return None;
        }

        // We have zero peers - check recovery conditions

        // 1. Periodic refresh (always runs when peers=0)
        if now.duration_since(self.last_refresh) >= self.refresh_interval {
            self.last_refresh = now;
            return Some(RecoveryReason::PeriodicRefresh);
        }

        // 2. Stale detection (had peers before, now 0)
        if self.had_peers_before {
            match self.zero_peers_since {
                Some(since) => {
                    let elapsed = now.duration_since(since);

                    // First trigger: after stale_threshold
                    if !self.in_recovery && elapsed >= self.stale_threshold {
                        self.in_recovery = true;
                        self.retry_count = 1;
                        return Some(RecoveryReason::StaleDetected);
                    }

                    // Subsequent retries
                    if self.in_recovery && self.retry_count < self.max_retries {
                        let retry_elapsed =
                            self.stale_threshold + self.retry_interval * self.retry_count as u32;
                        if elapsed >= retry_elapsed {
                            self.retry_count += 1;
                            return Some(RecoveryReason::Retry {
                                attempt: self.retry_count,
                            });
                        }
                    }
                }
                None => {
                    // Start tracking when we dropped to zero
                    self.zero_peers_since = Some(now);
                }
            }
        } else {
            // First time with zero peers, start tracking
            self.zero_peers_since = Some(now);
        }

        None
    }

    /// Get current recovery status for logging.
    pub fn status(&self) -> RecoveryStatus {
        RecoveryStatus {
            had_peers: self.had_peers_before,
            in_recovery: self.in_recovery,
            retry_count: self.retry_count,
            zero_since: self.zero_peers_since.map(|t| t.elapsed()),
        }
    }
}

/// Reason why recovery was triggered.
#[derive(Debug, Clone)]
pub enum RecoveryReason {
    /// System just woke from sleep
    WakeFromSleep { elapsed_secs: u64 },
    /// Periodic 60s refresh
    PeriodicRefresh,
    /// Stale detection triggered (first attempt)
    StaleDetected,
    /// Retry attempt after failed recovery
    Retry { attempt: u8 },
}

impl std::fmt::Display for RecoveryReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoveryReason::WakeFromSleep { elapsed_secs } => {
                write!(f, "wake from sleep ({}s elapsed)", elapsed_secs)
            }
            RecoveryReason::PeriodicRefresh => write!(f, "periodic refresh"),
            RecoveryReason::StaleDetected => write!(f, "stale detection"),
            RecoveryReason::Retry { attempt } => write!(f, "retry #{}", attempt),
        }
    }
}

/// Current recovery status for diagnostics.
#[derive(Debug)]
pub struct RecoveryStatus {
    pub had_peers: bool,
    pub in_recovery: bool,
    pub retry_count: u8,
    pub zero_since: Option<Duration>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_no_recovery_needed_with_peers() {
        let mut recovery = NetworkRecovery::new();
        assert!(recovery.should_recover(3).is_none());
    }

    #[test]
    fn test_periodic_refresh_triggers() {
        let mut recovery = NetworkRecovery::new();
        recovery.refresh_interval = Duration::from_millis(50);

        // First check - no trigger (just started)
        assert!(recovery.should_recover(0).is_none());

        // Wait for refresh interval
        sleep(Duration::from_millis(60));

        // Should trigger periodic refresh
        assert!(matches!(
            recovery.should_recover(0),
            Some(RecoveryReason::PeriodicRefresh)
        ));
    }
}
