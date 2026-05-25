use super::context;

/// Congestion control algorithm observed by a transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportAlgorithm {
    Cubic,
    Reno,
    Other(u8),
}

const SNAPSHOT_LATENCY_US: u16 = 1 << 0;
const SNAPSHOT_QUEUE_DEPTH: u16 = 1 << 1;
const SNAPSHOT_PACING_INTERVAL_US: u16 = 1 << 2;
const SNAPSHOT_CONGESTION_MARKS: u16 = 1 << 3;
const SNAPSHOT_RETRANSMISSIONS: u16 = 1 << 4;
const SNAPSHOT_PTO_COUNT: u16 = 1 << 5;
const SNAPSHOT_SRTT_US: u16 = 1 << 6;
const SNAPSHOT_LATEST_ACK_PN: u16 = 1 << 7;
const SNAPSHOT_CONGESTION_WINDOW: u16 = 1 << 8;
const SNAPSHOT_IN_FLIGHT_BYTES: u16 = 1 << 9;
const SNAPSHOT_ALGORITHM: u16 = 1 << 10;

/// Internal snapshot of transport-level observations supplied to routing policies.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransportSnapshot {
    present: u16,
    latency_us: u64,
    queue_depth: u32,
    pacing_interval_us: u64,
    congestion_marks: u32,
    retransmissions: u32,
    pto_count: u32,
    srtt_us: u64,
    latest_ack_pn: u64,
    congestion_window: u64,
    in_flight_bytes: u64,
    algorithm: TransportAlgorithm,
}

impl Default for TransportSnapshot {
    fn default() -> Self {
        Self {
            present: 0,
            latency_us: 0,
            queue_depth: 0,
            pacing_interval_us: 0,
            congestion_marks: 0,
            retransmissions: 0,
            pto_count: 0,
            srtt_us: 0,
            latest_ack_pn: 0,
            congestion_window: 0,
            in_flight_bytes: 0,
            algorithm: TransportAlgorithm::Other(0),
        }
    }
}

impl TransportSnapshot {
    /// Construct a transport snapshot from packed policy attributes.
    pub(crate) const fn from_policy_attrs(attrs: &context::PolicyAttrs) -> Self {
        Self {
            present: 0,
            latency_us: 0,
            queue_depth: 0,
            pacing_interval_us: 0,
            congestion_marks: 0,
            retransmissions: 0,
            pto_count: 0,
            srtt_us: 0,
            latest_ack_pn: 0,
            congestion_window: 0,
            in_flight_bytes: 0,
            algorithm: TransportAlgorithm::Other(0),
        }
        .set_latency_us(match attrs.get(context::core::LATENCY_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_queue_depth(match attrs.get(context::core::QUEUE_DEPTH) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_pacing_interval(match attrs.get(context::core::PACING_INTERVAL_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_congestion_marks(match attrs.get(context::core::CONGESTION_MARKS) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_retransmissions(match attrs.get(context::core::RETRANSMISSIONS) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_pto_count(match attrs.get(context::core::PTO_COUNT) {
            Some(value) => Some(value.as_u32()),
            None => None,
        })
        .set_srtt(match attrs.get(context::core::SRTT_US) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_latest_ack(match attrs.get(context::core::LATEST_ACK_PN) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_congestion_window(match attrs.get(context::core::CONGESTION_WINDOW) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_in_flight(match attrs.get(context::core::IN_FLIGHT_BYTES) {
            Some(value) => Some(value.as_u64()),
            None => None,
        })
        .set_algorithm(decode_transport_algorithm(
            attrs.get(context::core::TRANSPORT_ALGORITHM),
        ))
    }

    #[inline]
    pub const fn queue_depth(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_QUEUE_DEPTH) != 0 {
            Some(self.queue_depth)
        } else {
            None
        }
    }

    #[inline]
    pub const fn pacing_interval_us(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_PACING_INTERVAL_US) != 0 {
            Some(self.pacing_interval_us)
        } else {
            None
        }
    }

    #[inline]
    pub const fn congestion_marks(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_CONGESTION_MARKS) != 0 {
            Some(self.congestion_marks)
        } else {
            None
        }
    }

    #[inline]
    pub const fn retransmissions(&self) -> Option<u32> {
        if (self.present & SNAPSHOT_RETRANSMISSIONS) != 0 {
            Some(self.retransmissions)
        } else {
            None
        }
    }

    #[inline]
    pub const fn srtt_us(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_SRTT_US) != 0 {
            Some(self.srtt_us)
        } else {
            None
        }
    }

    #[inline]
    pub const fn congestion_window(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_CONGESTION_WINDOW) != 0 {
            Some(self.congestion_window)
        } else {
            None
        }
    }

    #[inline]
    pub const fn in_flight_bytes(&self) -> Option<u64> {
        if (self.present & SNAPSHOT_IN_FLIGHT_BYTES) != 0 {
            Some(self.in_flight_bytes)
        } else {
            None
        }
    }

    #[inline]
    pub const fn algorithm(&self) -> Option<TransportAlgorithm> {
        if (self.present & SNAPSHOT_ALGORITHM) != 0 {
            Some(self.algorithm)
        } else {
            None
        }
    }

    #[inline]
    const fn set_latency_us(mut self, latency_us: Option<u64>) -> Self {
        match latency_us {
            Some(value) => {
                self.present |= SNAPSHOT_LATENCY_US;
                self.latency_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_LATENCY_US;
                self.latency_us = 0;
            }
        }
        self
    }

    #[inline]
    const fn set_queue_depth(mut self, queue_depth: Option<u32>) -> Self {
        match queue_depth {
            Some(value) => {
                self.present |= SNAPSHOT_QUEUE_DEPTH;
                self.queue_depth = value;
            }
            None => {
                self.present &= !SNAPSHOT_QUEUE_DEPTH;
                self.queue_depth = 0;
            }
        }
        self
    }

    /// Attach congestion mark statistics (ECN-CE or equivalent) to the snapshot.
    const fn set_congestion_marks(mut self, congestion_marks: Option<u32>) -> Self {
        match congestion_marks {
            Some(value) => {
                self.present |= SNAPSHOT_CONGESTION_MARKS;
                self.congestion_marks = value;
            }
            None => {
                self.present &= !SNAPSHOT_CONGESTION_MARKS;
                self.congestion_marks = 0;
            }
        }
        self
    }

    /// Attach a pacing interval recommendation (microseconds between packets).
    const fn set_pacing_interval(mut self, pacing_interval_us: Option<u64>) -> Self {
        match pacing_interval_us {
            Some(value) => {
                self.present |= SNAPSHOT_PACING_INTERVAL_US;
                self.pacing_interval_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_PACING_INTERVAL_US;
                self.pacing_interval_us = 0;
            }
        }
        self
    }

    /// Attach retransmission statistics to the snapshot.
    const fn set_retransmissions(mut self, retransmissions: Option<u32>) -> Self {
        match retransmissions {
            Some(value) => {
                self.present |= SNAPSHOT_RETRANSMISSIONS;
                self.retransmissions = value;
            }
            None => {
                self.present &= !SNAPSHOT_RETRANSMISSIONS;
                self.retransmissions = 0;
            }
        }
        self
    }

    /// Attach PTO count statistics to the snapshot.
    const fn set_pto_count(mut self, pto_count: Option<u32>) -> Self {
        match pto_count {
            Some(value) => {
                self.present |= SNAPSHOT_PTO_COUNT;
                self.pto_count = value;
            }
            None => {
                self.present &= !SNAPSHOT_PTO_COUNT;
                self.pto_count = 0;
            }
        }
        self
    }

    /// Attach an RTT estimate (Smoothed RTT in microseconds).
    const fn set_srtt(mut self, srtt_us: Option<u64>) -> Self {
        match srtt_us {
            Some(value) => {
                self.present |= SNAPSHOT_SRTT_US;
                self.srtt_us = value;
            }
            None => {
                self.present &= !SNAPSHOT_SRTT_US;
                self.srtt_us = 0;
            }
        }
        self
    }

    /// Attach the most recent acknowledged packet number.
    const fn set_latest_ack(mut self, latest_ack_pn: Option<u64>) -> Self {
        match latest_ack_pn {
            Some(value) => {
                self.present |= SNAPSHOT_LATEST_ACK_PN;
                self.latest_ack_pn = value;
            }
            None => {
                self.present &= !SNAPSHOT_LATEST_ACK_PN;
                self.latest_ack_pn = 0;
            }
        }
        self
    }

    /// Attach a congestion window estimate (bytes) to the snapshot.
    const fn set_congestion_window(mut self, congestion_window: Option<u64>) -> Self {
        match congestion_window {
            Some(value) => {
                self.present |= SNAPSHOT_CONGESTION_WINDOW;
                self.congestion_window = value;
            }
            None => {
                self.present &= !SNAPSHOT_CONGESTION_WINDOW;
                self.congestion_window = 0;
            }
        }
        self
    }

    /// Attach the number of bytes currently considered in flight.
    const fn set_in_flight(mut self, in_flight_bytes: Option<u64>) -> Self {
        match in_flight_bytes {
            Some(value) => {
                self.present |= SNAPSHOT_IN_FLIGHT_BYTES;
                self.in_flight_bytes = value;
            }
            None => {
                self.present &= !SNAPSHOT_IN_FLIGHT_BYTES;
                self.in_flight_bytes = 0;
            }
        }
        self
    }

    /// Attach the congestion control algorithm affecting this snapshot.
    const fn set_algorithm(mut self, algorithm: Option<TransportAlgorithm>) -> Self {
        match algorithm {
            Some(value) => {
                self.present |= SNAPSHOT_ALGORITHM;
                self.algorithm = value;
            }
            None => {
                self.present &= !SNAPSHOT_ALGORITHM;
                self.algorithm = TransportAlgorithm::Other(0);
            }
        }
        self
    }

    /// Encode the snapshot into transport metrics tap arguments.
    ///
    /// The primary tuple encodes algorithm/queue depth/SRTT and congestion window/in-flight
    /// counters. When additional fields are available (retransmissions, congestion marks,
    /// pacing interval), a secondary tuple is produced which callers emit using the
    /// `ids::TRANSPORT_METRICS_EXT` tap identifier.
    ///
    /// * `arg0` — `[ algo | queue_depth | srtt_scaled ]`
    ///   * bits 31-28 store the algorithm identifier (0 reserved)
    ///   * bits 27-16 store the queue depth (saturated to 12 bits)
    ///   * bits 15-0 store `srtt_us / 32` (saturated to 16 bits)
    /// * `arg1` — `[ congestion_window_kib | in_flight_kib ]`
    ///   * bits 31-16 store the congestion window in KiB (saturated to 16 bits)
    ///   * bits 15-0 store in-flight bytes in KiB (saturated to 16 bits)
    pub fn encode_tap_metrics(&self) -> Option<TransportMetricsTapPayload> {
        let algorithm = self.algorithm()?;
        let algo_bits = match algorithm {
            TransportAlgorithm::Cubic => 1u32,
            TransportAlgorithm::Reno => 2u32,
            TransportAlgorithm::Other(code) => (code as u32).min(0xF).max(1),
        };
        let queue_depth = self
            .queue_depth()
            .map(|value| value.min(0x0FFE) + 1)
            .unwrap_or(0);
        let srtt_units = self
            .srtt_us()
            .map(|value| ((value / 32).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let congestion_window = self
            .congestion_window()
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let in_flight = self
            .in_flight_bytes()
            .map(|bytes| ((bytes / 1024).min(0xFFFE) as u32) + 1)
            .unwrap_or(0);
        let arg0 = (algo_bits << 28) | (queue_depth << 16) | srtt_units;
        let arg1 = (congestion_window << 16) | in_flight;
        let extension_needed = self.retransmissions().is_some()
            || self.congestion_marks().is_some()
            || self.pacing_interval_us().is_some();
        let extension = if extension_needed {
            let retransmissions = self
                .retransmissions()
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let congestion_marks = self
                .congestion_marks()
                .map(|value| value.min(0xFFFE) + 1)
                .unwrap_or(0);
            let pacing_interval = self
                .pacing_interval_us()
                .map(|value| {
                    let clamped = value.min(u32::MAX as u64 - 1);
                    (clamped as u32) + 1
                })
                .unwrap_or(0);
            let ext_arg0 = (retransmissions << 16) | congestion_marks;
            Some((ext_arg0, pacing_interval))
        } else {
            None
        };
        Some(TransportMetricsTapPayload {
            primary: (arg0, arg1),
            extension,
        })
    }
}

#[inline]
const fn decode_transport_algorithm(
    value: Option<context::ContextValue>,
) -> Option<TransportAlgorithm> {
    match value {
        Some(value) => match value.as_u32() {
            1 => Some(TransportAlgorithm::Cubic),
            2 => Some(TransportAlgorithm::Reno),
            raw if raw >= 0x100 => Some(TransportAlgorithm::Other((raw - 0x100) as u8)),
            raw => Some(TransportAlgorithm::Other(raw as u8)),
        },
        None => None,
    }
}

/// Packed tap payload emitted for transport metrics sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TransportMetricsTapPayload {
    primary: (u32, u32),
    extension: Option<(u32, u32)>,
}

impl TransportMetricsTapPayload {
    #[inline]
    pub(crate) const fn primary(&self) -> (u32, u32) {
        self.primary
    }

    #[inline]
    pub(crate) const fn extension(&self) -> Option<(u32, u32)> {
        self.extension
    }
}
