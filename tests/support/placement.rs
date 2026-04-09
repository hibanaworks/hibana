#[inline]
pub(crate) unsafe fn write_value<T>(dst: *mut T, value: T) {
    unsafe {
        dst.write(value);
    }
}
