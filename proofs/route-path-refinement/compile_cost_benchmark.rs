use super::*;
use crate::eff::{EffAtom, EventOrigin};
use crate::global::const_dsl::{ScopeId, ReentryMark};
const ROUTES: usize = if cfg!(measure_medium) {32} else if cfg!(measure_large) {64} else {16};
const fn source() -> EffList<{ROUTES*6}> {
    let mut source=EffList::new_partitioned(ROUTES*2,ROUTES*4,0);
    let mut i=0;
    while i<ROUTES*2 {
        source.push_event_mut(EffAtom {from:0,to:1,label:0,payload_schema:0,origin:EventOrigin::User,lane:0});
        i+=1;
    }
    i=0;
    while i<ROUTES {
        source.push_route_scope_mut(ScopeId::route(i as u16),i*2,i*2+1,i*2+2,ReentryMark::Reentrant);
        i+=1;
    }
    color_roll_frame_labels(&mut source,0,ROUTES*2);
    source
}
const SOURCE: EffList<{ROUTES*6}> = source();
const _: () = assert!(SOURCE.frame_label_at(ROUTES*2-1) == (ROUTES*2-1) as u8);
