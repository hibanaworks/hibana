use core::marker::PhantomData;

use crate::{
    epf::{
        PolicyMode,
        host::{HostError, HostSlots, Machine},
        loader::ImageLoader,
        verifier::Header,
        vm::Slot,
    },
    global::compiled::{CompiledProgram, LoweringSummary},
    observe::{
        core::push,
        events::{PolicyCommit, PolicyRollback},
    },
    rendezvous::slots::{SLOT_COUNT, SlotStorage, slot_index},
};

use super::{
    payload::{MgmtError, PolicyStats, slot_id},
    request_reply::{CLUSTER_PROGRAM, CONTROLLER_PROGRAM},
};

/// Per-slot digest pointers for O(1) activation/revert bookkeeping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PolicyDigestState {
    pub active_digest: Option<u32>,
    pub standby_digest: Option<u32>,
    pub last_good_digest: Option<u32>,
}

/// Promotion gate thresholds for Shadow → Enforce rollout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PromotionGateThresholds {
    pub min_samples: u32,
    pub max_divergence_ppm: u32,
    pub max_reject_delta_ppm: u32,
    pub max_p99_eval_us: u32,
    pub max_latency_increase_ppm: u32,
    pub max_fail_closed_ppm: u32,
    pub required_consecutive_windows: u8,
}

impl Default for PromotionGateThresholds {
    fn default() -> Self {
        Self {
            min_samples: 10_000,
            max_divergence_ppm: 1_000,
            max_reject_delta_ppm: 500,
            max_p99_eval_us: 250,
            max_latency_increase_ppm: 200_000,
            max_fail_closed_ppm: 100,
            required_consecutive_windows: 3,
        }
    }
}

/// Single promotion-gate measurement window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PromotionGateWindow {
    pub sample_count: u32,
    pub divergence_ppm: u32,
    pub reject_delta_ppm: u32,
    pub p99_eval_us: u32,
    pub latency_increase_ppm: u32,
    pub fail_closed_ppm: u32,
}

/// Running promotion-gate status for a slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PromotionGateState {
    pub consecutive_windows: u8,
}

impl PromotionGateState {
    #[inline]
    pub(crate) fn observe(
        &mut self,
        window: PromotionGateWindow,
        thresholds: PromotionGateThresholds,
    ) -> bool {
        let pass = window.sample_count >= thresholds.min_samples
            && window.divergence_ppm <= thresholds.max_divergence_ppm
            && window.reject_delta_ppm <= thresholds.max_reject_delta_ppm
            && window.p99_eval_us <= thresholds.max_p99_eval_us
            && window.latency_increase_ppm <= thresholds.max_latency_increase_ppm
            && window.fail_closed_ppm <= thresholds.max_fail_closed_ppm;
        if pass {
            self.consecutive_windows = self.consecutive_windows.saturating_add(1);
        } else {
            self.consecutive_windows = 0;
        }
        self.consecutive_windows >= thresholds.required_consecutive_windows
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cold;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AwaitBegin;

pub(crate) trait ManagerState {}
impl ManagerState for Cold {}
impl ManagerState for AwaitBegin {}

#[derive(Clone)]
struct SlotState {
    loader: ImageLoader,
    inventory: SlotInventory,
    pending: PendingVersion,
    version_counter: u32,
    active_epoch: u32,
    pending_epoch: Option<u32>,
    last_policy_stats: PolicyStats,
    digest_state: PolicyDigestState,
    policy_mode: PolicyMode,
    promotion_gate: PromotionGateState,
}

impl SlotState {
    fn new() -> Self {
        Self {
            loader: ImageLoader::new(),
            inventory: SlotInventory::new(),
            pending: PendingVersion::default(),
            version_counter: 0,
            active_epoch: 0,
            pending_epoch: None,
            last_policy_stats: PolicyStats::default(),
            digest_state: PolicyDigestState::default(),
            policy_mode: PolicyMode::Shadow,
            promotion_gate: PromotionGateState::default(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Manager<State, const SLOTS: usize>
where
    State: ManagerState,
{
    slot_states: [SlotState; SLOTS],
    _state: PhantomData<State>,
    timestamp: u32,
}

impl<const SLOTS: usize> Default for Manager<Cold, SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Manager<Cold, SLOTS> {
    pub(crate) fn new() -> Self {
        Self {
            slot_states: core::array::from_fn(|_| SlotState::new()),
            _state: PhantomData,
            timestamp: 0,
        }
    }

    pub(crate) fn into_await_begin(self) -> Manager<AwaitBegin, SLOTS> {
        let ts = self.timestamp;
        self.transition(AwaitBegin, ts)
    }
}

impl<State, const SLOTS: usize> Manager<State, SLOTS>
where
    State: ManagerState,
{
    fn transition<Next: ManagerState>(self, _state: Next, timestamp: u32) -> Manager<Next, SLOTS> {
        let Manager { slot_states, .. } = self;
        Manager {
            slot_states,
            _state: PhantomData,
            timestamp,
        }
    }

    fn slot_state(&mut self, slot: Slot) -> &mut SlotState {
        &mut self.slot_states[slot_index(slot)]
    }

    fn next_ts(&mut self) -> u32 {
        let current = self.timestamp;
        self.timestamp = self.timestamp.saturating_add(1);
        current
    }

    pub(crate) fn load_begin(&mut self, slot: Slot, header: Header) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        state.loader.begin(header)?;
        let version = state.version_counter.saturating_add(1);
        state.pending.begin(version);
        Ok(version)
    }

    pub(crate) fn load_chunk(
        &mut self,
        slot: Slot,
        offset: u32,
        chunk: &[u8],
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.loader.write(offset, chunk)?;
        Ok(())
    }

    pub(crate) fn load_commit(
        &mut self,
        slot: Slot,
        storage: &mut SlotStorage,
    ) -> Result<u32, MgmtError> {
        let state = self.slot_state(slot);
        let version = state.pending.take()?;
        let verified = state.loader.commit_for_slot(slot)?;
        let code = verified.code;
        storage.staging_mut()[..code.len()].copy_from_slice(code);

        let mut header = verified.header;
        header.code_len = code.len() as u16;
        let meta = ImageMeta::new(version, header);
        state.digest_state.standby_digest = Some(meta.header.hash);
        state.inventory.stage(meta);
        state.version_counter = version;
        Ok(version)
    }

    pub(crate) fn schedule_activate(
        &mut self,
        slot: Slot,
    ) -> Result<super::TransitionReport, MgmtError> {
        let state = self.slot_state(slot);
        let staged = state.inventory.staged().ok_or(MgmtError::NoStagedImage)?;
        state.pending_epoch = Some(staged.version);
        Ok(super::TransitionReport {
            version: staged.version,
            policy_stats: state.last_policy_stats,
        })
    }

    pub(crate) fn on_decision_boundary<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<Option<super::TransitionReport>, MgmtError> {
        let pending_epoch = {
            let state = self.slot_state(slot);
            state.pending_epoch
        };
        let Some(pending_epoch) = pending_epoch else {
            return Ok(None);
        };
        let staged_version = {
            let state = self.slot_state(slot);
            state.inventory.staged().map(|meta| meta.version)
        };
        if staged_version != Some(pending_epoch) {
            let state = self.slot_state(slot);
            state.pending_epoch = None;
            return Ok(None);
        }
        let report = self.activate_committed(slot, storage, host_slots)?;
        Ok(Some(report))
    }

    fn activate_committed<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<super::TransitionReport, MgmtError> {
        let ts = self.next_ts();
        let state = self.slot_state(slot);
        let previous_active = state.inventory.current_active();
        let staged = state.inventory.take_stage()?;

        if let Some(active_meta) = previous_active {
            storage.copy_active_to_backup(active_meta.code_len());
            if let Err(err) = host_slots.uninstall(slot)
                && !matches!(err, HostError::SlotEmpty)
            {
                return Err(err.into());
            }
        }

        storage.copy_staging_to_active(staged.meta().code_len());

        let active_meta = staged.meta();
        state.inventory.install_active(active_meta, previous_active);
        state.active_epoch = active_meta.version;
        state.pending_epoch = None;
        if let Some(previous_active_meta) = previous_active {
            state.digest_state.last_good_digest = Some(previous_active_meta.header.hash);
        }
        state.digest_state.active_digest = Some(active_meta.header.hash);
        state.digest_state.standby_digest = None;

        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..active_meta.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            active_meta.header.mem_len as usize,
            active_meta.header.fuel_max,
        )?;
        if let Err(err) = host_slots.install(slot, machine) {
            return Err(err.into());
        }
        host_slots.set_policy_mode(slot, state.policy_mode);

        push(PolicyCommit::with_digest(
            ts,
            slot_id(slot),
            active_meta.version,
            active_meta.header.hash,
        ));
        let policy_stats = PolicyStats {
            commits: 1,
            last_commit: Some(active_meta.version),
            ..PolicyStats::default()
        };
        state.last_policy_stats = policy_stats;
        Ok(super::TransitionReport {
            version: active_meta.version,
            policy_stats,
        })
    }

    pub(crate) fn revert<'arena>(
        &mut self,
        slot: Slot,
        storage: &'arena mut SlotStorage,
        host_slots: &mut HostSlots<'arena>,
    ) -> Result<super::TransitionReport, MgmtError> {
        let ts = self.next_ts();
        let state = self.slot_state(slot);
        let active_meta = state
            .inventory
            .current_active()
            .ok_or(MgmtError::NoActiveImage)?;

        state.digest_state.standby_digest = Some(active_meta.header.hash);
        storage.copy_active_to_staging(active_meta.code_len());
        state.inventory.stage(active_meta);

        if let Err(err) = host_slots.uninstall(slot)
            && !matches!(err, HostError::SlotEmpty)
        {
            return Err(err.into());
        }

        let new_active = {
            let entry = state.inventory.active_mut()?;
            entry.revert()?
        };
        state.active_epoch = new_active.version;
        state.pending_epoch = None;

        storage.copy_backup_to_active(new_active.code_len());
        let (active_buf, scratch) = storage.active_and_scratch_mut();
        let code_slice = &active_buf[..new_active.code_len()];
        let machine = Machine::with_mem(
            code_slice,
            scratch,
            new_active.header.mem_len as usize,
            new_active.header.fuel_max,
        )?;
        host_slots.install(slot, machine)?;
        host_slots.set_policy_mode(slot, state.policy_mode);
        state.digest_state.active_digest = Some(new_active.header.hash);
        state.digest_state.last_good_digest = Some(new_active.header.hash);

        push(PolicyRollback::with_digest(
            ts,
            slot_id(slot),
            new_active.version,
            new_active.header.hash,
        ));
        let policy_stats = PolicyStats {
            rollbacks: 1,
            last_rollback: Some(new_active.version),
            ..PolicyStats::default()
        };
        state.last_policy_stats = policy_stats;
        Ok(super::TransitionReport {
            version: new_active.version,
            policy_stats,
        })
    }

    pub(crate) fn policy_mode(&self, slot: Slot) -> Result<PolicyMode, MgmtError> {
        Ok(self.slot_states[slot_index(slot)].policy_mode)
    }

    pub(crate) fn set_policy_mode<'arena>(
        &mut self,
        slot: Slot,
        mode: PolicyMode,
        host_slots: &HostSlots<'arena>,
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.policy_mode = mode;
        host_slots.set_policy_mode(slot, mode);
        Ok(())
    }

    pub(crate) fn set_policy_mode_staged(
        &mut self,
        slot: Slot,
        mode: PolicyMode,
    ) -> Result<(), MgmtError> {
        let state = self.slot_state(slot);
        state.policy_mode = mode;
        Ok(())
    }

    pub(crate) fn observe_promotion_window(
        &mut self,
        slot: Slot,
        window: PromotionGateWindow,
        thresholds: PromotionGateThresholds,
    ) -> Result<bool, MgmtError> {
        let state = self.slot_state(slot);
        Ok(state.promotion_gate.observe(window, thresholds))
    }
}

pub(crate) fn with_management_compiled_programs_for_test<F, R>(f: F) -> R
where
    F: FnOnce(&CompiledProgram, &CompiledProgram) -> R,
{
    let controller_summary = LoweringSummary::scan_const(CONTROLLER_PROGRAM.lowering_input());
    let cluster_summary = LoweringSummary::scan_const(CLUSTER_PROGRAM.lowering_input());
    let mut controller = core::mem::MaybeUninit::<CompiledProgram>::uninit();
    let mut cluster = core::mem::MaybeUninit::<CompiledProgram>::uninit();
    unsafe {
        CompiledProgram::init_from_summary(controller.as_mut_ptr(), &controller_summary);
        CompiledProgram::init_from_summary(cluster.as_mut_ptr(), &cluster_summary);
        let result = f(controller.assume_init_ref(), cluster.assume_init_ref());
        controller.assume_init_drop();
        cluster.assume_init_drop();
        result
    }
}

#[derive(Clone, Copy, Debug)]
struct ImageMeta {
    version: u32,
    header: Header,
}

impl ImageMeta {
    fn new(version: u32, header: Header) -> Self {
        Self { version, header }
    }

    fn code_len(&self) -> usize {
        self.header.code_len as usize
    }
}

#[derive(Clone, Copy, Debug)]
struct StageEntry(ImageMeta);

impl StageEntry {
    fn meta(&self) -> ImageMeta {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
struct ActiveEntry {
    current: ImageMeta,
    previous: Option<ImageMeta>,
}

impl ActiveEntry {
    fn new(current: ImageMeta) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    fn install_previous(&mut self, previous: ImageMeta) {
        self.previous = Some(previous);
    }

    fn current(&self) -> ImageMeta {
        self.current
    }

    fn revert(&mut self) -> Result<ImageMeta, MgmtError> {
        let previous = self.previous.ok_or(MgmtError::NoPreviousImage)?;
        self.previous = Some(self.current);
        self.current = previous;
        Ok(self.current)
    }
}

#[derive(Clone, Copy, Debug)]
struct SlotInventory {
    staged: Option<StageEntry>,
    active: Option<ActiveEntry>,
}

impl SlotInventory {
    fn new() -> Self {
        Self {
            staged: None,
            active: None,
        }
    }

    fn stage(&mut self, meta: ImageMeta) {
        self.staged = Some(StageEntry(meta));
    }

    fn take_stage(&mut self) -> Result<StageEntry, MgmtError> {
        self.staged.take().ok_or(MgmtError::NoStagedImage)
    }

    fn current_active(&self) -> Option<ImageMeta> {
        self.active.as_ref().map(ActiveEntry::current)
    }

    fn staged(&self) -> Option<ImageMeta> {
        self.staged.as_ref().map(StageEntry::meta)
    }

    fn install_active(&mut self, current: ImageMeta, previous: Option<ImageMeta>) {
        let mut entry = ActiveEntry::new(current);
        if let Some(prev) = previous {
            entry.install_previous(prev);
        }
        self.active = Some(entry);
    }

    fn active_mut(&mut self) -> Result<&mut ActiveEntry, MgmtError> {
        self.active.as_mut().ok_or(MgmtError::NoActiveImage)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PendingVersion(Option<u32>);

impl PendingVersion {
    fn begin(&mut self, version: u32) {
        self.0 = Some(version);
    }

    fn take(&mut self) -> Result<u32, MgmtError> {
        self.0.take().ok_or(MgmtError::LoaderNotFinalised)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epf::{ops, verifier::compute_hash};

    fn stage_image(
        manager: &mut Manager<AwaitBegin, { SLOT_COUNT }>,
        slot: Slot,
        storage: &mut SlotStorage,
        code: &[u8],
    ) -> u32 {
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 64,
            flags: 0,
            hash: compute_hash(code),
        };
        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, code).unwrap();
        manager.load_commit(slot, storage).unwrap()
    }

    #[test]
    fn policy_switch_commits_only_on_decision_boundary() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let code = [0x01u8, 0x02, 0x03, 0x04];
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 16,
            mem_len: 64,
            flags: 0,
            hash: compute_hash(&code),
        };

        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, &code[..2]).unwrap();
        manager.load_chunk(slot, 2, &code[2..]).unwrap();

        let mut storage = SlotStorage::new();
        manager.load_commit(slot, &mut storage).unwrap();

        let mut host_slots = HostSlots::new();
        let scheduled = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled.version, 1);
        assert_eq!(host_slots.active_digest(slot), 0);

        let report = manager
            .on_decision_boundary(slot, &mut storage, &mut host_slots)
            .unwrap()
            .expect("decision boundary should commit scheduled activation");
        assert_eq!(report.version, 1);
        assert_ne!(host_slots.active_digest(slot), 0);
    }

    #[test]
    fn chunk_out_of_order_is_rejected() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Rendezvous;
        let header = Header {
            code_len: 2,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(&[0xAA, 0xBB]),
        };
        manager.load_begin(slot, header).unwrap();

        let err = manager
            .load_chunk(slot, 1, &[0xAA, 0xBB])
            .expect_err("chunk should be rejected");
        assert!(matches!(
            err,
            MgmtError::ChunkOutOfOrder {
                expected: 0,
                got: 1
            }
        ));
    }

    #[test]
    fn load_commit_rejects_get_input_for_forward_slot() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Forward;
        let code = [
            crate::epf::ops::instr::GET_INPUT,
            0x00,
            0x00,
            crate::epf::ops::instr::HALT,
        ];
        let header = Header {
            code_len: code.len() as u16,
            fuel_max: 8,
            mem_len: 32,
            flags: 0,
            hash: compute_hash(&code),
        };

        manager.load_begin(slot, header).unwrap();
        manager.load_chunk(slot, 0, &code).unwrap();

        let mut storage = SlotStorage::new();
        let err = manager.load_commit(slot, &mut storage).unwrap_err();
        assert!(matches!(err, MgmtError::LoaderNotFinalised));
    }

    #[test]
    fn set_policy_mode_updates_live_host_slots_immediately() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let host_slots = HostSlots::new();

        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Shadow);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Enforce);

        manager
            .set_policy_mode(slot, PolicyMode::Enforce, &host_slots)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Enforce);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Enforce);

        manager
            .set_policy_mode(slot, PolicyMode::Shadow, &host_slots)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Shadow);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);
    }

    #[test]
    fn set_policy_mode_staged_does_not_touch_live_host_slots() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let host_slots = HostSlots::new();

        manager
            .set_policy_mode(slot, PolicyMode::Shadow, &host_slots)
            .unwrap();
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);

        manager
            .set_policy_mode_staged(slot, PolicyMode::Enforce)
            .unwrap();
        assert_eq!(manager.policy_mode(slot).unwrap(), PolicyMode::Enforce);
        assert_eq!(host_slots.policy_mode(slot), PolicyMode::Shadow);
    }

    #[test]
    fn promotion_gate_requires_consecutive_windows_and_resets_on_failure() {
        let mut gate = PromotionGateState::default();
        let thresholds = PromotionGateThresholds {
            min_samples: 3,
            max_divergence_ppm: 10,
            max_reject_delta_ppm: 10,
            max_p99_eval_us: 100,
            max_latency_increase_ppm: 10,
            max_fail_closed_ppm: 10,
            required_consecutive_windows: 2,
        };
        let pass = PromotionGateWindow {
            sample_count: 3,
            divergence_ppm: 5,
            reject_delta_ppm: 5,
            p99_eval_us: 50,
            latency_increase_ppm: 5,
            fail_closed_ppm: 5,
        };
        let fail = PromotionGateWindow {
            sample_count: 3,
            divergence_ppm: 11,
            reject_delta_ppm: 5,
            p99_eval_us: 50,
            latency_increase_ppm: 5,
            fail_closed_ppm: 5,
        };

        assert!(!gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 1);
        assert!(gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 2);

        assert!(!gate.observe(fail, thresholds));
        assert_eq!(gate.consecutive_windows, 0);
        assert!(!gate.observe(pass, thresholds));
        assert_eq!(gate.consecutive_windows, 1);
    }

    #[test]
    fn manager_observe_promotion_window_tracks_slot_gate_state() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let thresholds = PromotionGateThresholds {
            min_samples: 2,
            max_divergence_ppm: 10,
            max_reject_delta_ppm: 10,
            max_p99_eval_us: 100,
            max_latency_increase_ppm: 10,
            max_fail_closed_ppm: 10,
            required_consecutive_windows: 2,
        };
        let pass = PromotionGateWindow {
            sample_count: 2,
            divergence_ppm: 0,
            reject_delta_ppm: 0,
            p99_eval_us: 10,
            latency_increase_ppm: 0,
            fail_closed_ppm: 0,
        };
        let fail = PromotionGateWindow {
            sample_count: 2,
            divergence_ppm: 20,
            reject_delta_ppm: 0,
            p99_eval_us: 10,
            latency_increase_ppm: 0,
            fail_closed_ppm: 0,
        };

        assert!(
            !manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
        assert!(
            manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
        assert!(
            !manager
                .observe_promotion_window(Slot::Forward, fail, thresholds)
                .unwrap()
        );
        assert!(
            manager
                .observe_promotion_window(Slot::Route, pass, thresholds)
                .unwrap()
        );
    }

    #[test]
    fn schedule_activate_overwrites_pending_epoch() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        let v1 = stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        let scheduled_v1 = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled_v1.version, v1);

        let v2 = stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x01, 0x00],
        );
        let scheduled_v2 = manager.schedule_activate(slot).unwrap();
        assert_eq!(scheduled_v2.version, v2);
        assert_eq!(
            manager.slot_states[slot_index(slot)].pending_epoch,
            Some(v2),
            "latest schedule must overwrite pending epoch"
        );
    }

    #[test]
    fn revert_clears_pending_epoch() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v1 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x02, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v2 activation");
            let _ = report;
        }

        let v3 = stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x03, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        assert_eq!(
            manager.slot_states[slot_index(slot)].pending_epoch,
            Some(v3)
        );

        let mut host_slots = HostSlots::new();
        manager.revert(slot, &mut storage, &mut host_slots).unwrap();
        assert_eq!(manager.slot_states[slot_index(slot)].pending_epoch, None);
    }

    #[test]
    fn schedule_then_revert_does_not_activate_stale_pending() {
        let mut manager = Manager::<Cold, { SLOT_COUNT }>::new().into_await_begin();
        let slot = Slot::Route;
        let mut storage = SlotStorage::new();

        stage_image(&mut manager, slot, &mut storage, &[ops::instr::HALT]);
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v1 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x02, 0x00],
        );
        manager.schedule_activate(slot).unwrap();
        {
            let mut host_slots = HostSlots::new();
            let report = manager
                .on_decision_boundary(slot, &mut storage, &mut host_slots)
                .unwrap()
                .expect("v2 activation");
            let _ = report;
        }

        stage_image(
            &mut manager,
            slot,
            &mut storage,
            &[ops::instr::ACT_ABORT, 0x03, 0x00],
        );
        manager.schedule_activate(slot).unwrap();

        {
            let mut host_slots = HostSlots::new();
            manager.revert(slot, &mut storage, &mut host_slots).unwrap();
        }
        let mut host_slots = HostSlots::new();
        let boundary = manager
            .on_decision_boundary(slot, &mut storage, &mut host_slots)
            .unwrap();
        assert!(
            boundary.is_none(),
            "stale pending must not activate after revert"
        );
    }
}
