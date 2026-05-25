use super::*;

#[test]
fn claim_cap_rejects_malformed_endpoint_control_header() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);
        let role = 5;
        let nonce = [0xCD; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        header[13] = 0x80;

        let token = endpoint_cap_token_from_wire(nonce, header);

        assert!(matches!(
            rendezvous.claim_cap(&token),
            Err(CapError::Mismatch)
        ));
    });
}

#[test]
fn claim_cap_rejects_malformed_endpoint_handle_payload() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);
        let role = 5;
        let nonce = [0xCE; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("claim test must bind cap storage");
        rendezvous
            .mint_cap::<crate::control::cap::mint::EndpointResource>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                handle,
            )
            .expect("valid capability mint must succeed");

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        header[crate::control::cap::mint::CAP_CONTROL_HEADER_FIXED_LEN + 6] = 0x7F;

        let token = endpoint_cap_token_from_wire(nonce, header);

        assert!(matches!(
            rendezvous.claim_cap(&token),
            Err(CapError::Mismatch)
        ));
    });
}

#[test]
fn claim_cap_rejects_malformed_handle_without_consuming_one_shot_authority() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(9);
        let lane = Lane::new(1);
        let role = 3;
        let nonce = [0xCF; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle_bytes =
            <RejectingHandleKind as crate::control::cap::mint::ResourceKind>::encode_handle(&());

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("claim test must bind cap storage");
        rendezvous
            .mint_cap::<RejectingHandleKind>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                (),
            )
            .expect("malformed handle fixture must mint ledger authority");

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            <RejectingHandleKind as crate::control::cap::mint::ResourceKind>::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            handle_bytes,
        )
        .encode(&mut header);
        let token = crate::control::cap::mint::GenericCapToken::<RejectingHandleKind>::from_bytes(
            cap_token_wire_image(nonce, header),
        );

        assert!(matches!(
            rendezvous.claim_cap(&token),
            Err(CapError::Mismatch)
        ));

        let claim_id = crate::observe::cap_claim::<RejectingHandleKind>();
        let exhaust_id = crate::observe::cap_exhaust::<RejectingHandleKind>();
        assert!(
            rendezvous
                .tap()
                .as_slice()
                .iter()
                .all(|event| event.id != claim_id && event.id != exhaust_id),
            "malformed handle preflight must not publish claim or exhaust events",
        );

        let exhausted = rendezvous
            .caps
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                <RejectingHandleKind as crate::control::cap::mint::ResourceKind>::TAG,
                role,
                crate::control::cap::mint::CapShot::One,
                &handle_bytes,
                2,
            )
            .expect("malformed typed decode must not consume one-shot ledger authority");
        assert!(exhausted);
    });
}

#[test]
fn claim_cap_emits_one_shot_claim_before_exhaust() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(11);
        let lane = Lane::new(1);
        let role = 4;
        let nonce = [0xC1; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("claim order test must bind cap storage");
        rendezvous
            .mint_cap::<crate::control::cap::mint::EndpointResource>(
                sid,
                lane,
                crate::control::cap::mint::CapShot::One,
                role,
                nonce,
                handle,
            )
            .expect("one-shot capability mint must succeed");

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        let token = endpoint_cap_token_from_wire(nonce, header);

        rendezvous
            .claim_cap(&token)
            .expect("one-shot capability claim must succeed");

        let claim_id = crate::observe::cap_claim::<crate::control::cap::mint::EndpointResource>();
        let exhaust_id =
            crate::observe::cap_exhaust::<crate::control::cap::mint::EndpointResource>();
        let mut lifecycle = [0u16; 2];
        let mut len = 0usize;
        for event in rendezvous.tap().as_slice() {
            if event.id == claim_id || event.id == exhaust_id {
                assert!(
                    len < lifecycle.len(),
                    "claim lifecycle emitted too many events"
                );
                lifecycle[len] = event.id;
                len += 1;
            }
        }

        assert_eq!(len, 2, "one-shot claim must emit claim and exhaust");
        assert_eq!(lifecycle[0], claim_id);
        assert_eq!(lifecycle[1], exhaust_id);
    });
}

#[test]
fn delegate_and_claim_reject_noncanonical_decodable_endpoint_headers() {
    fn endpoint_token_with_mutated_header(
        mutate: fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
    ) -> crate::control::cap::mint::GenericCapToken<crate::control::cap::mint::EndpointResource>
    {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);
        let role = 5;
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        mutate(&mut header);

        endpoint_cap_token_from_wire([0xCD; crate::control::cap::mint::CAP_NONCE_LEN], header)
    }

    fn mutate_tag(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[7] = crate::control::cap::resource_kinds::LoopContinueKind::TAG;
    }

    fn mutate_op(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[8] = ControlOp::TopologyBegin.as_u8();
    }

    fn mutate_path(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[9] = crate::control::cap::mint::ControlPath::Wire.as_u8();
    }

    fn mutate_shot(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[10] = crate::control::cap::mint::CapShot::Many.as_u8();
    }

    fn mutate_scope_kind(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[11] = crate::global::const_dsl::ControlScopeKind::Route as u8;
    }

    fn mutate_flags(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[12] = 0x01;
    }

    fn mutate_scope_id(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[13..15].copy_from_slice(&1u16.to_be_bytes());
    }

    fn mutate_epoch(header: &mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]) {
        header[15..17].copy_from_slice(&1u16.to_be_bytes());
    }

    let cases: &[(
        &str,
        fn(&mut [u8; crate::control::cap::mint::CAP_HEADER_LEN]),
    )] = &[
        ("tag", mutate_tag),
        ("op", mutate_op),
        ("path", mutate_path),
        ("shot", mutate_shot),
        ("scope_kind", mutate_scope_kind),
        ("flags", mutate_flags),
        ("scope_id", mutate_scope_id),
        ("epoch", mutate_epoch),
    ];

    with_epf_test_rendezvous(|rendezvous| {
        rendezvous.assoc.register(Lane::new(1), SessionId::new(7));
        for (name, mutate) in cases {
            let token = endpoint_token_with_mutated_header(*mutate);
            assert!(
                token.control_header().is_ok(),
                "{name} mutation must remain decodable to exercise canonical validation",
            );

            let envelope = CpCommand::new(ControlOp::CapDelegate).with_delegate(
                crate::control::cluster::core::DelegateOperands {
                    claim: false,
                    token,
                },
            );
            assert!(
                matches!(
                    EffectRunner::run_effect(rendezvous, envelope),
                    Err(CpError::Delegation(_))
                ),
                "{name} mutation must be rejected by delegate execution",
            );
            assert!(
                matches!(rendezvous.claim_cap(&token), Err(CapError::Mismatch)),
                "{name} mutation must be rejected by claim_cap",
            );
        }
    });
}

#[test]
fn cap_delegate_rejects_unregistered_lane_without_panicking() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);
        let role = 5;
        let nonce = [0xD1; crate::control::cap::mint::CAP_NONCE_LEN];
        let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);

        let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
        crate::control::cap::mint::CapHeader::new(
            sid,
            lane,
            role,
            crate::control::cap::mint::EndpointResource::TAG,
            ControlOp::Fence,
            crate::control::cap::mint::ControlPath::Local,
            crate::control::cap::mint::CapShot::One,
            crate::global::const_dsl::ControlScopeKind::None,
            0,
            0,
            0,
            crate::control::cap::mint::EndpointResource::encode_handle(&handle),
        )
        .encode(&mut header);
        let token = endpoint_cap_token_from_wire(nonce, header);

        let envelope = CpCommand::new(ControlOp::CapDelegate).with_delegate(
            crate::control::cluster::core::DelegateOperands {
                claim: false,
                token,
            },
        );

        assert!(matches!(
            EffectRunner::run_effect(rendezvous, envelope),
            Err(CpError::Delegation(
                crate::control::cluster::error::DelegationError::InvalidToken
            ))
        ));
    });
}

#[test]
fn cap_delegate_reports_resource_exhaustion_when_cap_table_is_full() {
    with_epf_test_rendezvous(|rendezvous| {
        let sid = SessionId::new(7);
        let lane = Lane::new(1);
        let role = 5;

        rendezvous.assoc.register(lane, sid);
        rendezvous
            .ensure_endpoint_resident_budget(EndpointResidentBudget::with_route_storage(
                0, 0, 0, 1, 0,
            ))
            .expect("delegate test must bind one cap entry");

        let make_token = |nonce| {
            let handle = crate::control::cap::mint::EndpointHandle::new(sid, lane, role);
            let mut header = [0u8; crate::control::cap::mint::CAP_HEADER_LEN];
            crate::control::cap::mint::CapHeader::new(
                sid,
                lane,
                role,
                crate::control::cap::mint::EndpointResource::TAG,
                ControlOp::Fence,
                crate::control::cap::mint::ControlPath::Local,
                crate::control::cap::mint::CapShot::One,
                crate::global::const_dsl::ControlScopeKind::None,
                0,
                0,
                0,
                crate::control::cap::mint::EndpointResource::encode_handle(&handle),
            )
            .encode(&mut header);
            endpoint_cap_token_from_wire(nonce, header)
        };

        let first = CpCommand::new(ControlOp::CapDelegate).with_delegate(
            crate::control::cluster::core::DelegateOperands {
                claim: false,
                token: make_token([0xD2; crate::control::cap::mint::CAP_NONCE_LEN]),
            },
        );
        EffectRunner::run_effect(rendezvous, first)
            .expect("first delegate mint must consume the only cap slot");

        let second = CpCommand::new(ControlOp::CapDelegate).with_delegate(
            crate::control::cluster::core::DelegateOperands {
                claim: false,
                token: make_token([0xD3; crate::control::cap::mint::CAP_NONCE_LEN]),
            },
        );
        assert!(matches!(
            EffectRunner::run_effect(rendezvous, second),
            Err(CpError::ResourceExhausted {
                resource: ResourceScope::Generic
            })
        ));
    });
}
