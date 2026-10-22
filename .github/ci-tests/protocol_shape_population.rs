//! Count and unrank trees with at least one converging exchange.

#[derive(Clone, Copy, Debug)]
pub(super) enum Population {
    Exchange,
    Any,
    Converging,
}

impl Population {
    pub(super) fn children(self) -> &'static [(Self, Self)] {
        match self {
            Self::Exchange => &[(Self::Exchange, Self::Exchange)],
            Self::Any => &[(Self::Any, Self::Any)],
            // These sets are disjoint: the left child either contains a
            // converging exchange, or is exchange-only and the right does.
            Self::Converging => &[
                (Self::Converging, Self::Any),
                (Self::Exchange, Self::Converging),
            ],
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Count {
    pub(super) exchange: usize,
    pub(super) any: usize,
}

impl Count {
    pub(super) fn get(self, population: Population) -> usize {
        match population {
            Population::Exchange => self.exchange,
            Population::Any => self.any,
            Population::Converging => self.any - self.exchange,
        }
    }
}

impl core::ops::Add for Count {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            exchange: self.exchange + other.exchange,
            any: self.any + other.any,
        }
    }
}

impl core::ops::Mul for Count {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self {
            exchange: self.exchange * other.exchange,
            any: self.any * other.any,
        }
    }
}

impl core::ops::Mul<Count> for usize {
    type Output = Count;
    fn mul(self, other: Count) -> Count {
        Count {
            exchange: self * other.exchange,
            any: self * other.any,
        }
    }
}
