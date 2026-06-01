use super::CAP_NONCE_LEN;
use core::marker::PhantomData;

pub(crate) struct NonceSeed {
    counter: u64,
}

impl NonceSeed {
    #[inline(always)]
    pub(crate) const fn counter(counter: u64) -> Self {
        Self { counter }
    }

    #[inline(always)]
    pub(crate) const fn counter_value(&self) -> u64 {
        self.counter
    }
}

/// Trait implemented by const minting specifications.
pub(crate) trait CapMintSpec {
    /// Derive the nonce bytes using the rendezvous-provided seed.
    fn nonce(seed: NonceSeed) -> [u8; CAP_NONCE_LEN];
}

/// Canonical trusted-domain strategy: counter-based nonce.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NullMintSpec;

impl CapMintSpec for NullMintSpec {
    #[inline(always)]
    fn nonce(seed: NonceSeed) -> [u8; CAP_NONCE_LEN] {
        let mut out = [0u8; CAP_NONCE_LEN];
        let bytes = seed.counter_value().to_be_bytes();
        let offset = CAP_NONCE_LEN - bytes.len();
        out[offset..].copy_from_slice(&bytes);
        out
    }
}

/// Endpoint mint policy – the attached endpoint may mint control payloads.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EndpointMintPolicy;

/// Marker trait implemented by policies that permit endpoint minting.
pub(crate) trait AllowsEndpointMint {}

impl AllowsEndpointMint for EndpointMintPolicy {}

/// Zero-sized minting strategy value.
#[derive(Debug, Default)]
pub(crate) struct CapMintStrategy<S: CapMintSpec> {
    _spec: PhantomData<S>,
}

impl<S: CapMintSpec> Copy for CapMintStrategy<S> {}

impl<S: CapMintSpec> Clone for CapMintStrategy<S> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: CapMintSpec> CapMintStrategy<S> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self { _spec: PhantomData }
    }

    #[inline(always)]
    pub(crate) fn derive_nonce(&self, seed: NonceSeed) -> [u8; CAP_NONCE_LEN] {
        S::nonce(seed)
    }
}

/// Zero-sized mint configuration baked into role programs.
#[derive(Debug)]
pub(crate) struct MintConfig<S: CapMintSpec = NullMintSpec, P: Copy = EndpointMintPolicy> {
    strategy: CapMintStrategy<S>,
    _policy: PhantomData<P>,
}

impl<S, P> Copy for MintConfig<S, P>
where
    S: CapMintSpec,
    P: Copy,
{
}

impl<S, P> Clone for MintConfig<S, P>
where
    S: CapMintSpec,
    P: Copy,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: CapMintSpec, P: Copy> Default for MintConfig<S, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: CapMintSpec, P: Copy> MintConfig<S, P> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            strategy: CapMintStrategy::<S>::new(),
            _policy: PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) const fn strategy(&self) -> CapMintStrategy<S> {
        self.strategy
    }
}

/// Marker trait enabling `MintConfig` specialisation.
pub(crate) trait MintConfigMarker: Copy {
    type Spec: CapMintSpec;
    type Policy: Copy;
    const INSTANCE: Self;

    fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy>;
}

impl<S, P> MintConfigMarker for MintConfig<S, P>
where
    S: CapMintSpec,
    P: Copy,
{
    type Spec = S;
    type Policy = P;
    const INSTANCE: Self = MintConfig::<S, P>::new();

    #[inline(always)]
    fn as_config(&self) -> MintConfig<Self::Spec, Self::Policy> {
        MintConfig::<S, P>::new()
    }
}
