//! Callgrind client requests for the Linux x86-64 measurement job.
//!
//! The request ABI is specified by Valgrind's callgrind.h and valgrind.h.
//! Other hosts still run the identical benchmark without profiling requests.

#[inline]
fn request(code: usize) {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let arguments = [code, 0, 0, 0, 0, 0];
        // SAFETY: Valgrind reads six live machine words synchronously. The
        // marker is inert on native x86-64; it restores rdi after 128 rotated
        // bits. All scratch registers, flags and memory effects are declared.
        unsafe {
            core::arch::asm!(
                "rol rdi, 3", "rol rdi, 13", "rol rdi, 61", "rol rdi, 51",
                "xchg rbx, rbx",
                in("rax") arguments.as_ptr(),
                inout("rdx") 0usize => _,
                lateout("rdi") _,
                options(nostack),
            );
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let _ = code;
}

#[inline]
pub(super) fn start() {
    request(0x4354_0004);
}

#[inline]
pub(super) fn stop() {
    request(0x4354_0005);
}
