use super::wire_envelope;

#[kani::proof]
fn owned_envelope_preserves_the_resource_and_full_event_identifier() {
    let resource: u8 = kani::any();
    let event: u16 = kani::any();
    let encoded = wire_envelope(resource, event);
    assert_eq!(encoded[0], resource);
    assert_eq!(u16::from_be_bytes([encoded[1], encoded[2]]), event);
}

#[kani::proof]
fn owned_envelope_cannot_alias_different_resources_or_events() {
    let first_resource: u8 = kani::any();
    let first_event: u16 = kani::any();
    let second_resource: u8 = kani::any();
    let second_event: u16 = kani::any();
    if wire_envelope(first_resource, first_event) == wire_envelope(second_resource, second_event) {
        assert_eq!(first_resource, second_resource);
        assert_eq!(first_event, second_event);
    }
}
