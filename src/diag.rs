use core::panic::Location;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Callsite {
    location: &'static Location<'static>,
}

impl Callsite {
    #[inline]
    #[track_caller]
    pub(crate) fn caller() -> Self {
        Self {
            location: Location::caller(),
        }
    }

    #[inline]
    pub(crate) const fn file(self) -> &'static str {
        self.location.file()
    }

    #[inline]
    pub(crate) const fn line(self) -> u32 {
        self.location.line()
    }

    #[inline]
    pub(crate) const fn column(self) -> u32 {
        self.location.column()
    }
}
