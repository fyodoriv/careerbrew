#![cfg_attr(all(target_os = "macos", panic = "abort"), no_std)]
#![cfg_attr(all(target_os = "macos", panic = "abort"), no_main)]

#[cfg(not(all(target_os = "macos", panic = "abort")))]
fn main() {
    std::process::exit(78);
}

#[cfg(all(target_os = "macos", panic = "abort"))]
mod leaf {
    use core::ptr::{read_volatile, write_volatile};

    #[link(name = "System")]
    unsafe extern "C" {
        // SDK mach_init.h maps mach_task_self() to this existing C ABI variable.
        #[link_name = "mach_task_self_"]
        static BORROWED_TASK_PORT: libc::mach_port_t;
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        unsafe { libc::_exit(101) }
    }

    // Refuse every foreign-unwind path; never return an unwinder success or continuation.
    #[unsafe(no_mangle)]
    extern "C" fn rust_eh_personality(
        _version: libc::c_int,
        _actions: libc::c_int,
        _exception_class: u64,
        _exception_object: *mut core::ffi::c_void,
        _context: *mut core::ffi::c_void,
    ) -> libc::c_int {
        unsafe { libc::_exit(101) }
    }

    fn refuse(status: i32) -> ! {
        unsafe { libc::_exit(status) }
    }

    unsafe fn descriptor(text: *const u8) -> i32 {
        let mut value = 0_i32;
        for index in 0..=10 {
            let byte = unsafe { *text.add(index) };
            if byte == 0 {
                if index == 0 || value < 3 {
                    refuse(64);
                }
                return value;
            }
            if index == 10 || !byte.is_ascii_digit() {
                refuse(64);
            }
            value = match value
                .checked_mul(10)
                .and_then(|v| v.checked_add(i32::from(byte - b'0')))
            {
                Some(v) => v,
                None => refuse(64),
            };
        }
        refuse(64)
    }

    fn put32(record: &mut [u8; 96], offset: usize, value: u32) {
        record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put64(record: &mut [u8; 96], offset: usize, value: u64) {
        record[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// # Safety
    /// The C runtime supplies argc and valid NUL-terminated argv pointers.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn main(argc: i32, argv: *const *const u8) -> i32 {
        if argc != 3 || argv.is_null() {
            refuse(64);
        }
        let mode_arg = unsafe { *argv.add(1) };
        let fd_arg = unsafe { *argv.add(2) };
        if mode_arg.is_null() || fd_arg.is_null() {
            refuse(64);
        }
        let mode = match unsafe { *mode_arg } {
            b'S' => 0_u32,
            b'L' => 1_u32,
            _ => refuse(64),
        };
        // A nonzero first byte guarantees another byte through the C string's terminator.
        if unsafe { *mode_arg.add(1) } != 0 {
            refuse(64);
        }
        let fd = unsafe { descriptor(fd_arg) };
        let uid = unsafe { libc::getuid() };
        if uid == 0
            || uid != unsafe { libc::geteuid() }
            || unsafe { libc::getgid() } != unsafe { libc::getegid() }
        {
            refuse(66);
        }
        let mut stat: libc::stat = unsafe { core::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut stat) } != 0
            || stat.st_uid != uid
            || stat.st_size != 0
            || stat.st_mode != (libc::S_IFREG | 0o600)
        {
            refuse(65);
        }
        // LibSystem initializes these process-lifetime values before C entry.
        // Borrow the task port by value; never mutate or deallocate it.
        let page = unsafe { libc::vm_page_size };
        let task = unsafe { BORROWED_TASK_PORT };
        if page != 4096 && page != 16384 {
            refuse(66);
        }
        let payload = if mode == 0 {
            16_usize << 20
        } else {
            128_usize << 20
        };
        if !payload.is_multiple_of(page) {
            refuse(66);
        }
        let expected = if mode == 0 {
            0x3fba67b5df100000_u64
        } else {
            0x941c6daef8800000_u64
        };
        let pid = unsafe { libc::getpid() };
        if pid <= 0 {
            refuse(66);
        }
        let mut record = [0_u8; 96];
        record[..8].copy_from_slice(b"CBRSSU01");
        put32(&mut record, 8, 1);
        put32(&mut record, 12, pid as u32);
        put32(&mut record, 16, mode);
        put32(&mut record, 20, fd as u32);
        put64(&mut record, 24, payload as u64);
        put64(&mut record, 32, page as u64);
        // Darwin dev_t is signed32; preserve its unsigned32 identity bits.
        put64(
            &mut record,
            64,
            u64::from(u32::from_ne_bytes(stat.st_dev.to_ne_bytes())),
        );
        put64(&mut record, 72, stat.st_ino);
        put32(&mut record, 80, uid);
        put32(&mut record, 84, u32::from(stat.st_mode));

        let mapping = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                payload,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if mapping == libc::MAP_FAILED
            || mapping.is_null()
            || !(mapping as usize).is_multiple_of(page)
        {
            refuse(67);
        }
        let words = mapping.cast::<u64>();
        let count = payload / 8;
        for index in 0..count {
            let value = (index as u64)
                .wrapping_mul(0x9e3779b97f4a7c15)
                .wrapping_add(0xd1b54a32d192ed03);
            unsafe {
                write_volatile(words.add(index), value);
            }
        }
        for offset in [40, 48] {
            let mut sum = 0_u64;
            for index in 0..count {
                sum = sum.wrapping_add(unsafe { read_volatile(words.add(index)) });
            }
            if sum != expected {
                refuse(68);
            }
            put64(&mut record, offset, sum);
        }
        let mut info: libc::mach_task_basic_info = unsafe { core::mem::zeroed() };
        let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
        let result = unsafe {
            libc::task_info(
                task,
                libc::MACH_TASK_BASIC_INFO,
                (&mut info as *mut libc::mach_task_basic_info).cast(),
                &mut count,
            )
        };
        if result != libc::KERN_SUCCESS || count != libc::MACH_TASK_BASIC_INFO_COUNT {
            refuse(69);
        }
        let maximum = info.resident_size_max;
        put64(&mut record, 56, maximum);
        let mut offset = 0;
        for _ in 0..32 {
            let written = unsafe {
                libc::write(
                    fd,
                    record.as_ptr().add(offset).cast(),
                    record.len() - offset,
                )
            };
            if written > 0 {
                offset += written as usize;
                if offset == record.len() {
                    unsafe { libc::_exit(0) }
                }
            } else if written == 0 || unsafe { *libc::__error() } != libc::EINTR {
                refuse(70);
            }
        }
        refuse(70)
    }
}
