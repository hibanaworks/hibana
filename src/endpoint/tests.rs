#[cfg(test)]
mod tests {
    use crate::endpoint::{
        Endpoint, RouteBranch, flow, futures::DecodeFuture, futures::OfferFuture,
        futures::RawRecvFlags, futures::RecvFuture, kernel,
    };
    use core::mem::size_of;

    type RecvFut = RecvFuture<'static, 'static, 0, crate::g::Msg<7, ()>>;
    type DecodeFut = DecodeFuture<'static, 'static, 0, crate::g::Msg<7, ()>>;
    type RecvFutU8 = RecvFuture<'static, 'static, 0, crate::g::Msg<8, u8>>;
    type RecvFutU64 = RecvFuture<'static, 'static, 0, crate::g::Msg<9, u64>>;
    type RecvFutBytes = RecvFuture<'static, 'static, 0, crate::g::Msg<10, [u8; 32]>>;
    type DecodeFutU8 = DecodeFuture<'static, 'static, 0, crate::g::Msg<11, u8>>;
    type DecodeFutU64 = DecodeFuture<'static, 'static, 0, crate::g::Msg<12, u64>>;
    type DecodeFutBytes = DecodeFuture<'static, 'static, 0, crate::g::Msg<13, [u8; 32]>>;
    type FlowU8 = flow::Flow<'static, 'static, 0, crate::g::Msg<14, u8>>;
    type FlowBytes = flow::Flow<'static, 'static, 0, crate::g::Msg<15, [u8; 32]>>;
    type SendFut = flow::SendFuture<'static, 'static, 'static, 0>;

    #[test]
    fn endpoint_surface_size_gates_hold() {
        const WORD: usize = size_of::<usize>();
        assert!(
            size_of::<Endpoint<'static, 0>>() <= 3 * WORD,
            "Endpoint<'_, ROLE> must stay within the 3-word budget"
        );
        assert!(
            size_of::<RouteBranch<'static, 'static, 0>>() <= 2 * WORD,
            "RouteBranch<'_, '_, ROLE> must stay within the 2-word budget"
        );
        assert!(
            size_of::<OfferFuture<'static, 'static, 0>>() <= 3 * WORD,
            "OfferFuture must stay within the 3-word budget"
        );
        assert!(
            size_of::<RecvFut>() <= 3 * WORD,
            "RecvFuture must stay within the 3-word budget"
        );
        assert!(
            size_of::<DecodeFut>() <= 3 * WORD,
            "DecodeFuture must stay within the 3-word budget"
        );
    }

    #[test]
    fn message_type_variation_does_not_change_future_layout() {
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutU8>());
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutU64>());
        assert_eq!(size_of::<RecvFut>(), size_of::<RecvFutBytes>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutU8>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutU64>());
        assert_eq!(size_of::<DecodeFut>(), size_of::<DecodeFutBytes>());
    }

    #[test]
    fn raw_recv_flags_cache_empty_payload_fact_and_completion() {
        let mut accepts_empty = RawRecvFlags::new(true, true);
        assert!(accepts_empty.accepts_empty_payload());
        assert!(accepts_empty.leased());
        assert!(!accepts_empty.completed());
        accepts_empty.mark_completed();
        assert!(accepts_empty.completed());

        let rejects_empty = RawRecvFlags::new(false, false);
        assert!(!rejects_empty.accepts_empty_payload());
        assert!(!rejects_empty.leased());
        assert!(!rejects_empty.completed());
    }

    #[test]
    fn send_flow_and_runtime_descriptor_size_gates_hold() {
        const WORD: usize = size_of::<usize>();
        assert_eq!(
            size_of::<FlowU8>(),
            size_of::<FlowBytes>(),
            "Flow layout must remain payload-type independent",
        );
        assert!(
            size_of::<FlowU8>() <= 2 * WORD,
            "Flow must stay a drop guard, not a send preview owner",
        );
        assert!(
            size_of::<SendFut>() <= 3 * WORD,
            "SendFuture must stay within the raw-future wrapper budget",
        );
        assert!(
            size_of::<kernel::RecvRuntimeDesc>() <= WORD,
            "RecvRuntimeDesc must stay smaller than a pointer-sized descriptor",
        );
        assert!(
            size_of::<kernel::DecodeRuntimeDesc>() <= 3 * WORD,
            "DecodeRuntimeDesc must be core plus decode metadata only",
        );
        assert!(
            size_of::<kernel::SendRuntimeDesc>() <= 6 * WORD,
            "SendRuntimeDesc must be send-specific metadata, not a union descriptor",
        );
    }

    #[test]
    fn final_form_future_layout_measurement_report() {
        std::println!(
            "future-layout Endpoint={} RouteBranch={} OfferFuture={} RecvFuture={} DecodeFuture={} SendFuture={} Flow={} RecvFutureU8={} RecvFutureU64={} RecvFutureBytes={} DecodeFutureU8={} DecodeFutureU64={} DecodeFutureBytes={}",
            size_of::<Endpoint<'static, 0>>(),
            size_of::<RouteBranch<'static, 'static, 0>>(),
            size_of::<OfferFuture<'static, 'static, 0>>(),
            size_of::<RecvFut>(),
            size_of::<DecodeFut>(),
            size_of::<SendFut>(),
            size_of::<FlowU8>(),
            size_of::<RecvFutU8>(),
            size_of::<RecvFutU64>(),
            size_of::<RecvFutBytes>(),
            size_of::<DecodeFutU8>(),
            size_of::<DecodeFutU64>(),
            size_of::<DecodeFutBytes>(),
        );
    }
}
