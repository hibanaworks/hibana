//! Encode actual public polls and their sealed source for the Lean comparison.
pub(super) fn observed_poll<T>(
    poll: &core::task::Poll<Result<T, hibana::EndpointError>>,
) -> &'static str {
    match poll {
        core::task::Poll::Pending => "pending",
        core::task::Poll::Ready(Ok(_)) => "ready",
        core::task::Poll::Ready(Err(error)) if error.is_unavailable() => "unavailable",
        core::task::Poll::Ready(Err(_)) => "other-error",
    }
}

pub(super) fn record_observation(row: &str) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("HIBANA_VISIT_WAIT_OBSERVATIONS") {
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        writeln!(file, "{row}").unwrap();
    }
}

pub(super) fn record_contracts<const ROLE: u8>(
    direction: &str,
    program: &hibana::runtime::program::RoleProgram<ROLE>,
) {
    use hibana::runtime::tap;
    use std::fmt::Write;
    let mut row = format!("source {direction}");
    for point in tap::projected(program) {
        match point.id() {
            tap::PROJECTED_SOURCE => {
                let [sender, receiver, _, label] = point.arg0().to_be_bytes();
                write!(
                    row,
                    " {sender},{receiver},{label},{},{}",
                    point.causal_key() >> 8,
                    point.arg1()
                )
                .unwrap();
            }
            tap::PROJECTED_SCOPE | tap::PROJECTED_CHOICE => {
                let kind = if point.id() == tap::PROJECTED_SCOPE {
                    "scope"
                } else {
                    "choice"
                };
                write!(
                    row,
                    " {kind},{},{},{}",
                    point.causal_key(),
                    point.arg0(),
                    point.arg1()
                )
                .unwrap();
            }
            _ => {}
        }
    }
    record_observation(&row);
}
