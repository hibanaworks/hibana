#[cfg(feature = "std")]
use core::panic::Location;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(feature = "std")]
pub(crate) struct Callsite {
    location: &'static Location<'static>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(feature = "std"))]
pub(crate) struct Callsite;

impl Callsite {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        #[cfg(feature = "std")]
        {
            Self {
                location: Location::caller(),
            }
        }

        #[cfg(not(feature = "std"))]
        {
            Self
        }
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    #[cfg(feature = "std")]
    pub(crate) const fn column(self) -> u32 {
        self.location.column()
    }
}
