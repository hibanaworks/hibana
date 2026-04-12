use crate::{
    control::cap::{
        mint::GenericCapToken,
        resource_kinds::{LoopBreakKind, LoopContinueKind},
    },
    g::{self, Program},
    global::{
        CanonicalControl,
        steps::{self, PolicySteps, RouteSteps, SeqSteps, StepCons, StepNil},
    },
    observe::core::{TAP_BATCH_MAX_EVENTS, TapEvent},
    runtime::consts::{
        LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE, LABEL_OBSERVE_BATCH, LABEL_OBSERVE_STREAM_END,
        LABEL_OBSERVE_SUBSCRIBE,
    },
    transport::wire::{CodecError, WireDecode, WireEncode},
};

use super::{
    payload::SubscribeReq,
    request_reply::{ROLE_CLUSTER, ROLE_CONTROLLER},
};

const TAP_BATCH_HEADER_LEN: usize = 5;
const TAP_EVENT_WIRE_LEN: usize = 20;
const STREAM_LOOP_POLICY_ID: u16 = 701;

/// Internal batched observe-stream payload. Public substrate surface stays on
/// `tap::TapEvent`; batching is a lower-layer stream detail.
#[derive(Clone, Copy, Debug)]
pub struct TapBatch {
    events: [TapEvent; TAP_BATCH_MAX_EVENTS],
    count: u8,
    lost_events: u32,
}

impl Default for TapBatch {
    fn default() -> Self {
        Self::empty()
    }
}

impl TapBatch {
    #[inline]
    const fn empty() -> Self {
        Self {
            events: [TapEvent::zero(); TAP_BATCH_MAX_EVENTS],
            count: 0,
            lost_events: 0,
        }
    }

    #[inline]
    fn push(&mut self, event: TapEvent) -> bool {
        if (self.count as usize) < TAP_BATCH_MAX_EVENTS {
            self.events[self.count as usize] = event;
            self.count += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    const fn len(&self) -> usize {
        self.count as usize
    }

    #[inline]
    const fn lost_events(&self) -> u32 {
        self.lost_events
    }

    #[inline]
    fn set_lost_events(&mut self, lost: u32) {
        self.lost_events = lost;
    }

    #[inline]
    fn iter(&self) -> impl Iterator<Item = &TapEvent> {
        self.events[..self.count as usize].iter()
    }
}

impl WireEncode for TapBatch {
    fn encoded_len(&self) -> Option<usize> {
        Some(TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN)
    }

    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let total_len = TAP_BATCH_HEADER_LEN + self.len() * TAP_EVENT_WIRE_LEN;
        if out.len() < total_len {
            return Err(CodecError::Truncated);
        }

        out[0] = self.len() as u8;
        out[1..5].copy_from_slice(&self.lost_events().to_be_bytes());

        let mut offset = TAP_BATCH_HEADER_LEN;
        for event in self.iter() {
            event.encode_into(&mut out[offset..])?;
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(total_len)
    }
}

impl<'a> WireDecode<'a> for TapBatch {
    fn decode_from(input: &'a [u8]) -> Result<Self, CodecError> {
        if input.len() < TAP_BATCH_HEADER_LEN {
            return Err(CodecError::Truncated);
        }

        let count = input[0] as usize;
        if count > TAP_BATCH_MAX_EVENTS {
            return Err(CodecError::Invalid("batch count exceeds maximum"));
        }

        let lost_events = u32::from_be_bytes([input[1], input[2], input[3], input[4]]);
        let expected_len = TAP_BATCH_HEADER_LEN + count * TAP_EVENT_WIRE_LEN;
        if input.len() < expected_len {
            return Err(CodecError::Truncated);
        }

        let mut batch = TapBatch::empty();
        batch.set_lost_events(lost_events);

        let mut offset = TAP_BATCH_HEADER_LEN;
        for _ in 0..count {
            let event = TapEvent::decode_from(&input[offset..])?;
            batch.push(event);
            offset += TAP_EVENT_WIRE_LEN;
        }

        Ok(batch)
    }
}

const STREAM_SUBSCRIBE: Program<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CONTROLLER>,
            g::Role<ROLE_CLUSTER>,
            g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>,
        >,
        StepNil,
    >,
> = g::send::<
    g::Role<ROLE_CONTROLLER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>,
    0,
>();

type StreamLoopContinueHead = PolicySteps<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CLUSTER>,
            g::Role<ROLE_CLUSTER>,
            g::Msg<
                LABEL_LOOP_CONTINUE,
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
        >,
        StepNil,
    >,
    STREAM_LOOP_POLICY_ID,
>;
type StreamLoopBreakHead = PolicySteps<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CLUSTER>,
            g::Role<ROLE_CLUSTER>,
            g::Msg<
                LABEL_LOOP_BREAK,
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
        >,
        StepNil,
    >,
    STREAM_LOOP_POLICY_ID,
>;
type StreamLoopContinueArm = SeqSteps<
    StreamLoopContinueHead,
    StepCons<
        steps::SendStep<g::Role<ROLE_CLUSTER>, g::Role<ROLE_CONTROLLER>, g::Msg<LABEL_OBSERVE_BATCH, TapBatch>>,
        StepNil,
    >,
>;
type StreamLoopBreakArm = SeqSteps<
    StreamLoopBreakHead,
    StepCons<
        steps::SendStep<g::Role<ROLE_CLUSTER>, g::Role<ROLE_CONTROLLER>, g::Msg<LABEL_OBSERVE_STREAM_END, ()>>,
        StepNil,
    >,
>;
type StreamLoopRoute = RouteSteps<StreamLoopContinueArm, StreamLoopBreakArm>;

const STREAM_LOOP_CONTINUE_PREFIX: Program<StreamLoopContinueHead> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<
        LABEL_LOOP_CONTINUE,
        GenericCapToken<LoopContinueKind>,
        CanonicalControl<LoopContinueKind>,
    >,
    0,
>()
.policy::<STREAM_LOOP_POLICY_ID>();

const STREAM_LOOP_CONTINUE_ARM: Program<StreamLoopContinueArm> = g::seq(
    STREAM_LOOP_CONTINUE_PREFIX,
    g::send::<
        g::Role<ROLE_CLUSTER>,
        g::Role<ROLE_CONTROLLER>,
        g::Msg<LABEL_OBSERVE_BATCH, TapBatch>,
        0,
    >(),
);

const STREAM_LOOP_BREAK_PREFIX: Program<StreamLoopBreakHead> = g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CLUSTER>,
    g::Msg<LABEL_LOOP_BREAK, GenericCapToken<LoopBreakKind>, CanonicalControl<LoopBreakKind>>,
    0,
>()
.policy::<STREAM_LOOP_POLICY_ID>();

const STREAM_LOOP_BREAK_ARM: Program<StreamLoopBreakArm> = STREAM_LOOP_BREAK_PREFIX.then(g::send::<
    g::Role<ROLE_CLUSTER>,
    g::Role<ROLE_CONTROLLER>,
    g::Msg<LABEL_OBSERVE_STREAM_END, ()>,
    0,
>());

const STREAM_LOOP_ROUTE: Program<StreamLoopRoute> =
    g::route(STREAM_LOOP_CONTINUE_ARM, STREAM_LOOP_BREAK_ARM);

pub type ProgramSteps = SeqSteps<
    StepCons<
        steps::SendStep<
            g::Role<ROLE_CONTROLLER>,
            g::Role<ROLE_CLUSTER>,
            g::Msg<LABEL_OBSERVE_SUBSCRIBE, SubscribeReq>,
        >,
        StepNil,
    >,
    StreamLoopRoute,
>;

pub const PROGRAM: Program<ProgramSteps> = crate::g::seq(STREAM_SUBSCRIBE, STREAM_LOOP_ROUTE);
