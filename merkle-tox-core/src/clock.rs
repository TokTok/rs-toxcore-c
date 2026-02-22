use crate::dag::{LogicalIdentityPk, PhysicalDevicePk};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
pub use tox_sequenced::time::{ManualTimeProvider, SystemTimeProvider, TimeProvider};

/// Maximum clock slewing rate (1% drift).
const MAX_SLEW_RATE: f64 = 0.01;
/// Threshold for jumping instead of slewing (10 minutes).
const SLEW_THRESHOLD_MS: i64 = 10 * 60 * 1000;

/// Median-based consensus clock with slewing and Byzantine resilience.
///
/// Spec: Multiple devices of same identity averaged into single per-identity
/// offset; one vote per logical identity.
pub struct NetworkClock {
    /// Per-logical-identity offset samples. Devices sharing identity
    /// are averaged into a single offset before computing weighted median.
    peer_offsets: BTreeMap<LogicalIdentityPk, Vec<(i64, u32)>>,
    /// Reverse mapping from device PK to logical identity for convenience.
    device_to_identity: BTreeMap<PhysicalDevicePk, LogicalIdentityPk>,
    /// Target offset (ms).
    target_offset: i64,
    /// Current offset (ms).
    current_offset: f64,
    /// The last instant we applied slewing.
    last_slew_instant: Instant,
    /// Monotonic base for time calculations.
    base_instant: Instant,
    /// System time (UNIX_EPOCH) at base_instant.
    base_system_time: i64,
    /// Last returned network time for monotonicity.
    last_network_time: i64,
    /// The time provider used for all time-related calls.
    time_provider: Arc<dyn TimeProvider>,
}

impl NetworkClock {
    pub fn new(time_provider: Arc<dyn TimeProvider>) -> Self {
        let now_inst = time_provider.now_instant();
        let now_sys = time_provider.now_system_ms();

        Self {
            peer_offsets: BTreeMap::new(),
            device_to_identity: BTreeMap::new(),
            target_offset: 0,
            current_offset: 0.0,
            last_slew_instant: now_inst,
            base_instant: now_inst,
            base_system_time: now_sys,
            last_network_time: now_sys,
            time_provider,
        }
    }

    pub fn time_provider(&self) -> &dyn TimeProvider {
        &*self.time_provider
    }

    /// Records time offset sample from trusted peer.
    /// Uses device PK as logical identity (single-device identity).
    pub fn update_peer_offset(&mut self, peer: PhysicalDevicePk, offset: i64) {
        let logical = LogicalIdentityPk::from(*peer.as_bytes());
        self.update_peer_offset_weighted(peer, logical, offset, 1);
    }

    /// Records time offset sample with weight, grouped by logical identity.
    /// Devices sharing identity contribute single per-identity vote.
    pub fn update_peer_offset_weighted(
        &mut self,
        peer: PhysicalDevicePk,
        logical_pk: LogicalIdentityPk,
        offset: i64,
        weight: u32,
    ) {
        self.device_to_identity.insert(peer, logical_pk);

        // Rebuild identity samples from mapping devices.
        // Each device contributes one (offset, weight) sample.
        let samples = self.peer_offsets.entry(logical_pk).or_default();
        *samples = vec![(offset, weight)];

        self.recalculate_consensus();
    }

    fn recalculate_consensus(&mut self) {
        if self.peer_offsets.is_empty() {
            return;
        }

        // Step 1: Average samples within logical identity producing
        // one (offset, weight) per identity.
        let mut identity_samples: Vec<(i64, u32)> = Vec::new();
        for samples in self.peer_offsets.values() {
            if samples.is_empty() {
                continue;
            }
            let total_w: u64 = samples.iter().map(|s| s.1 as u64).sum();
            let weighted_sum: i64 = samples
                .iter()
                .map(|&(o, w)| o.saturating_mul(w as i64))
                .sum::<i64>();
            let avg_offset = if total_w > 0 {
                weighted_sum / total_w as i64
            } else {
                0
            };
            let avg_weight = if total_w > 0 {
                (total_w / samples.len() as u64) as u32
            } else {
                1
            };
            identity_samples.push((avg_offset, avg_weight.max(1)));
        }

        if identity_samples.is_empty() {
            return;
        }

        // Step 2: Weighted median across per-identity offsets.
        identity_samples.sort_unstable_by_key(|s| s.0);

        let total_weight: u64 = identity_samples.iter().map(|s| s.1 as u64).sum();
        let target_weight = total_weight / 2;

        let mut current_weight = 0u64;
        let mut target = identity_samples.last().unwrap().0;

        for i in 0..identity_samples.len() {
            let (offset, weight) = identity_samples[i];
            current_weight += weight as u64;
            if current_weight > target_weight {
                target = offset;
                break;
            } else if current_weight == target_weight && total_weight.is_multiple_of(2) {
                // Median falls between two samples, average them.
                if i + 1 < identity_samples.len() {
                    target = (offset + identity_samples[i + 1].0) / 2;
                } else {
                    target = offset;
                }
                break;
            }
        }

        // Hard-cap at ±5 minutes from local OS clock.
        target = target.clamp(-300_000, 300_000);

        // First sample: jump directly to target.
        if self.target_offset == 0 && self.current_offset == 0.0 && self.peer_offsets.len() == 1 {
            self.current_offset = target as f64;
        }

        self.target_offset = target;

        // Jump on massive drift.
        let drift = (self.target_offset - self.current_offset as i64).abs();
        if drift > SLEW_THRESHOLD_MS {
            self.current_offset = self.target_offset as f64;
        }
    }

    /// Applies slewing to current offset.
    fn apply_slewing(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_slew_instant).as_secs_f64() * 1000.0;
        self.last_slew_instant = now;

        if (self.current_offset - self.target_offset as f64).abs() < 0.1 {
            self.current_offset = self.target_offset as f64;
            return;
        }

        // Calculate max allowed adjustment for this elapsed time.
        let max_adj = elapsed * MAX_SLEW_RATE;
        let remaining = self.target_offset as f64 - self.current_offset;

        if remaining > 0.0 {
            self.current_offset += remaining.min(max_adj);
        } else {
            self.current_offset -= (-remaining).min(max_adj);
        }
    }

    /// Returns consensus network time in milliseconds.
    pub fn network_time_ms(&mut self) -> i64 {
        let now = self.time_provider.now_instant();
        self.apply_slewing(now);

        let elapsed = now.duration_since(self.base_instant).as_millis() as i64;
        let net_time = self.base_system_time + elapsed + self.current_offset as i64;

        // Monotonicity check.
        if net_time < self.last_network_time {
            self.last_network_time
        } else {
            self.last_network_time = net_time;
            net_time
        }
    }

    pub fn consensus_offset(&self) -> i64 {
        self.current_offset as i64
    }

    /// Returns computed target offset (before slewing) for testing.
    pub fn consensus_target_offset(&self) -> i64 {
        self.target_offset
    }
}
