// Test: Crash message next step becomes Stop<Step>, which is not MaySend.
// Expected: compile error because CrashMsg::Next<E0> does not implement MaySend.

use hibana::control::cap::{E0, MaySend};
use hibana::control::cap::payload::CrashNotice;
use hibana::g::Msg;

fn require_may_send<S: MaySend>() {}

fn main() {
    type CrashMsg = Msg<{ hibana::runtime::consts::LABEL_CRASH }, CrashNotice>;
    require_may_send::<CrashMsg::Next<E0>>();
}
