use super::*;
use crate::eff::{EffAtom, EventOrigin};
use crate::global::const_dsl::{ScopeId, ReentryMark};
const ROUTES: usize = 64;
const fn source() -> EffList<{ROUTES*6}> {
    let mut source=EffList::new_partitioned(ROUTES*2,ROUTES*4,0);
    let from=if cfg!(measure_max_sparse) {254} else if cfg!(measure_tls_sparse) {24} else {0};
    let mut i=0;
    while i<ROUTES*2 {
        source.push_event_mut(EffAtom {from,to:from+1,label:(i%2+1) as u8,payload_schema:0,origin:EventOrigin::User,lane:0});
        i+=1;
    }
    i=0;
    while i<ROUTES {
        source.push_route_scope_mut(ScopeId::route(i as u16),i*2,i*2+1,i*2+2,ReentryMark::SinglePass);
        i+=1;
    }
    source
}
const SOURCE: EffList<{ROUTES*6}> = source();
const SUMMARY: CompiledProgramImage = CompiledProgramImage::scan_const(&SOURCE);
const _: () = assert!(validate_route_projection_guarantees(&SUMMARY,&SOURCE).is_none());
