//! Fixed-size const formatting for rustc E0080 diagnostics; never runtime state.
use super::{ProjectionDiagnostic, ProjectionProblem};
struct Text {
    bytes: [u8; 768],
    len: usize,
}
impl Text {
    const fn new() -> Self {
        Self {
            bytes: [0; 768],
            len: 0,
        }
    }
    const fn push(&mut self, value: &str) {
        let bytes = value.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if self.len == self.bytes.len() {
                panic!("projection diagnostic message capacity");
            }
            self.bytes[self.len] = bytes[i];
            self.len += 1;
            i += 1;
        }
    }
    const fn number(&mut self, mut n: usize) {
        let mut digits = [0u8; 20];
        let mut count = 0;
        loop {
            digits[count] = b'0' + (n % 10) as u8;
            count += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            self.bytes[self.len] = digits[count];
            self.len += 1;
        }
    }
}
pub(super) const fn panic_diagnostic(d: ProjectionDiagnostic) -> ! {
    let mut text = Text::new();
    text.push(match d.problem {
        ProjectionProblem::RouteControllerMismatch => "route arms use different first visible controllers",
        ProjectionProblem::ReceiveLaneCausality => "receive lane sender change requires a causal handoff or exclusive route arms",
        ProjectionProblem::ParallelSelector => "parallel endpoint operations must be unambiguous",
        ProjectionProblem::ReentrySelector => "rolled reentry endpoint operations must be unambiguous",
        ProjectionProblem::UnprojectableRoute | ProjectionProblem::MissingBranchKnowledge => "Route unprojectable for this role: invalid, ambiguous endpoint operation, or ambiguous first-visible endpoint operation",
    });
    if let Some(role) = d.role {
        text.push("; role=");
        text.number(role as usize);
    }
    if let Some(scope) = d.scope {
        text.push("; scope=");
        text.number(scope as usize);
    }
    if let Some(controller) = d.controller {
        text.push("; controller=");
        text.number(controller as usize);
    }
    if let Some(arms) = d.arms {
        text.push("; arms=");
        text.number(arms[0].start);
        text.push("..");
        text.number(arms[0].end);
        text.push("|");
        text.number(arms[1].start);
        text.push("..");
        text.number(arms[1].end);
    }
    let events = [d.first, d.second];
    let mut i = 0;
    while i < 2 {
        if let Some(event) = events[i] {
            text.push("; event#");
            text.number(event.index);
            text.push("(");
            text.number(event.from as usize);
            text.push("->");
            text.number(event.to as usize);
            text.push(" label=");
            text.number(event.label as usize);
            text.push(" lane=");
            text.number(event.lane as usize);
            text.push(")");
        }
        i += 1;
    }
    text.push("; help: ");
    text.push(d.help());
    let bytes = text.bytes.split_at(text.len).0;
    match core::str::from_utf8(bytes) {
        Ok(message) => panic!("{}", message),
        Err(_) => panic!("invalid diagnostic encoding"),
    }
}
