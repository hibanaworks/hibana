use super::*;

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn next_nonce_seed(&self) -> NonceSeed {
        let ordinal = self.cap_nonce.get();
        let next = ordinal
            .checked_add(1)
            .expect("capability nonce counter exhausted");
        self.cap_nonce.set(next);
        NonceSeed::counter(ordinal)
    }

    #[inline]
    pub(crate) fn next_cap_revision(&self) -> u64 {
        let next = self
            .cap_revision
            .get()
            .checked_add(1)
            .expect("capability revision counter exhausted");
        self.cap_revision.set(next);
        next
    }

    #[inline]
    pub(crate) fn cap_release_ctx(&self, lane: Lane) -> CapReleaseCtx {
        CapReleaseCtx::new(&self.caps, &self.state_snapshots, &self.cap_revision, lane)
    }

    pub(crate) fn mint_cap<K: ResourceKind>(
        &self,
        sid: SessionId,
        lane: Lane,
        shot: CapShot,
        dest_role: u8,
        nonce: [u8; 16],
        mut handle: K::Handle,
    ) -> Result<(), CapError> {
        let kind_tag = K::TAG;
        let registered_sid = self
            .assoc
            .get_sid(lane)
            .ok_or(CapError::WrongSessionOrLane)?;
        if registered_sid != sid {
            return Err(CapError::WrongSessionOrLane);
        }

        let handle_bytes = K::encode_handle(&handle);
        K::zeroize(&mut handle);

        let entry = CapEntry {
            sid,
            lane_raw: lane.as_wire(),
            kind_tag,
            shot_state: shot.as_u8(),
            role: dest_role,
            mint_revision: self.next_cap_revision(),
            consumed_revision: 0,
            released_revision: 0,
            nonce,
            handle: handle_bytes,
        };
        self.caps
            .insert_entry(entry)
            .map_err(|_| CapError::TableFull)?;

        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(self.clock.now32(), crate::observe::cap_mint::<K>())
                .with_arg0(sid.raw())
                .with_arg1(((lane.as_wire() as u32) << 16) | (dest_role as u32)),
        );
        Ok(())
    }

    pub(crate) fn claim_cap<K: crate::control::cap::mint::ResourceKind>(
        &self,
        token: &GenericCapToken<K>,
    ) -> Result<(), CapError> {
        let nonce = token.nonce();

        // Check if AUTO (all zeros)
        if nonce == [0u8; crate::control::cap::mint::CAP_NONCE_LEN] && token.is_auto() {
            return Err(CapError::UnknownToken);
        }

        let header = token.control_header().map_err(|_| CapError::Mismatch)?;
        if header.tag() == crate::control::cap::mint::EndpointResource::TAG {
            let endpoint_token = crate::control::cap::mint::GenericCapToken::<
                crate::control::cap::mint::EndpointResource,
            >::from_bytes(token.into_bytes());
            endpoint_token
                .endpoint_identity()
                .map_err(|_| CapError::Mismatch)?;
        }

        let sid = header.sid();
        let lane = header.lane();
        let role = header.role();
        let kind_tag = header.tag();
        let shot = match header.shot() {
            crate::control::cap::mint::CapShot::One => CapShot::One,
            crate::control::cap::mint::CapShot::Many => CapShot::Many,
        };

        if self.assoc.get_sid(lane) != Some(sid) {
            return Err(CapError::WrongSessionOrLane);
        }

        if kind_tag != K::TAG {
            return Err(CapError::Mismatch);
        }

        let token_handle = token.handle_bytes();
        let mut handle = token.decode_handle().map_err(|_| CapError::Mismatch)?;

        // Claim authority is the rendezvous-local nonce ledger plus descriptor validation.
        let claim_revision = self.next_cap_revision();
        let exhausted = match self
            .caps
            .claim_by_nonce(
                &nonce,
                sid,
                lane,
                kind_tag,
                role,
                shot,
                &token_handle,
                claim_revision,
            )
            .map_err(|e| match e {
                CapError::UnknownToken => CapError::UnknownToken,
                CapError::WrongSessionOrLane => CapError::WrongSessionOrLane,
                CapError::Exhausted => CapError::Exhausted,
                CapError::TableFull => CapError::TableFull,
                CapError::Mismatch => CapError::Mismatch,
            }) {
            Ok(exhausted) => exhausted,
            Err(err) => {
                K::zeroize(&mut handle);
                return Err(err);
            }
        };

        let claim_id = crate::observe::cap_claim::<K>();
        let exhaust_id = crate::observe::cap_exhaust::<K>();

        let now = self.clock.now32();
        let tap = self.tap();
        emit(
            tap,
            RawEvent::new(now, claim_id)
                .with_arg0(sid.raw())
                .with_arg1(0),
        );

        if exhausted {
            let tap = self.tap();
            emit(
                tap,
                RawEvent::new(now, exhaust_id)
                    .with_arg0(sid.raw())
                    .with_arg1(0),
            );
        }

        K::zeroize(&mut handle);
        Ok(())
    }
}
