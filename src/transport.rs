use std::io;
use std::mem::size_of;
use std::os::fd::RawFd;

const SYS_RECVMSG: u64 = 27;
const SYS_SENDMSG: u64 = 28;
const SYS_CLOSE: u64 = 6;
const SOL_SOCKET: i32 = 0xffff;
const SCM_RIGHTS: i32 = 1;
const MSG_TRUNC: i32 = 0x10;
const MSG_CTRUNC: i32 = 0x20;
const CMSG_HEADER_SIZE: usize = 16;
const CMSG_ALIGN: usize = 8;

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

#[repr(C)]
struct MsgHdr {
    name: *mut u8,
    name_len: u32,
    iov: *mut Iovec,
    iov_len: i32,
    control: *mut u8,
    control_len: u32,
    flags: i32,
}

fn align(value: usize) -> usize {
    (value + (CMSG_ALIGN - 1)) & !(CMSG_ALIGN - 1)
}

fn cmsg_space(fd_count: usize) -> usize {
    align(CMSG_HEADER_SIZE) + align(fd_count * size_of::<i32>())
}

fn cmsg_len(fd_count: usize) -> usize {
    CMSG_HEADER_SIZE + fd_count * size_of::<i32>()
}

fn raw_syscall3(number: u64, a: usize, b: usize, c: usize) -> isize {
    #[cfg(target_arch = "x86_64")]
    {
        let mut result = number as isize;
        // SAFETY: the syscall ABI arguments are passed in the registers
        // required by FreeBSD amd64; the kernel does not retain pointers.
        unsafe {
            std::arch::asm!(
                "syscall",
                inlateout("rax") result,
                in("rdi") a,
                in("rsi") b,
                in("rdx") c,
                lateout("rcx") _,
                lateout("r11") _,
            );
        }
        result
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (number, a, b, c);
        -libc_unavailable_error()
    }
}

#[cfg(not(target_arch = "x86_64"))]
const fn libc_unavailable_error() -> isize {
    78
}

fn syscall_result(value: isize) -> io::Result<usize> {
    if value < 0 {
        Err(io::Error::from_raw_os_error((-value) as i32))
    } else {
        Ok(value as usize)
    }
}

fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

fn get_u32(buffer: &[u8], offset: usize) -> u32 {
    let mut value = [0; 4];
    value.copy_from_slice(&buffer[offset..offset + 4]);
    u32::from_ne_bytes(value)
}

pub(crate) fn close_fd(fd: RawFd) {
    let _ = raw_syscall3(SYS_CLOSE, fd as usize, 0, 0);
}

pub(crate) fn send(fd: RawFd, bytes: &[u8], descriptors: &[RawFd]) -> io::Result<()> {
    let mut control = vec![0u8; cmsg_space(descriptors.len())];
    if !descriptors.is_empty() {
        put_u32(&mut control, 0, cmsg_len(descriptors.len()) as u32);
        put_u32(&mut control, 8, SOL_SOCKET as u32);
        put_u32(&mut control, 12, SCM_RIGHTS as u32);
        for (index, descriptor) in descriptors.iter().enumerate() {
            let offset = CMSG_HEADER_SIZE + index * size_of::<i32>();
            control[offset..offset + 4].copy_from_slice(&descriptor.to_ne_bytes());
        }
    }
    let mut iov = Iovec {
        base: bytes.as_ptr() as *mut u8,
        len: bytes.len(),
    };
    let mut header = MsgHdr {
        name: std::ptr::null_mut(),
        name_len: 0,
        iov: &mut iov,
        iov_len: 1,
        control: if control.is_empty() {
            std::ptr::null_mut()
        } else {
            control.as_mut_ptr()
        },
        control_len: control.len() as u32,
        flags: 0,
    };
    let sent = syscall_result(raw_syscall3(
        SYS_SENDMSG,
        fd as usize,
        &mut header as *mut MsgHdr as usize,
        0,
    ))?;
    if sent != bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "short nvlist send",
        ));
    }
    Ok(())
}

pub(crate) fn recv(fd: RawFd) -> io::Result<(Vec<u8>, Vec<RawFd>)> {
    let mut bytes = vec![0u8; 1024 * 1024];
    let mut control = vec![0u8; cmsg_space(256)];
    let mut iov = Iovec {
        base: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    let mut header = MsgHdr {
        name: std::ptr::null_mut(),
        name_len: 0,
        iov: &mut iov,
        iov_len: 1,
        control: control.as_mut_ptr(),
        control_len: control.len() as u32,
        flags: 0,
    };
    let mut received = syscall_result(raw_syscall3(
        SYS_RECVMSG,
        fd as usize,
        &mut header as *mut MsgHdr as usize,
        0,
    ))?;
    if header.flags & (MSG_TRUNC | MSG_CTRUNC) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "truncated nvlist data or descriptor ancillary data",
        ));
    }

    // SOCK_STREAM may split one nvlist across multiple recvmsg calls.  The
    // root header carries the body length, so continue reading until the
    // complete frame is present.  Ancillary descriptors are delivered with
    // the first call and are deliberately not requested again.
    while received < 19 {
        let mut iov = Iovec {
            base: bytes[received..19].as_mut_ptr(),
            len: 19 - received,
        };
        let mut continuation = MsgHdr {
            name: std::ptr::null_mut(),
            name_len: 0,
            iov: &mut iov,
            iov_len: 1,
            control: std::ptr::null_mut(),
            control_len: 0,
            flags: 0,
        };
        let count = syscall_result(raw_syscall3(
            SYS_RECVMSG,
            fd as usize,
            &mut continuation as *mut MsgHdr as usize,
            0,
        ))?;
        if continuation.flags & MSG_TRUNC != 0 || count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short nvlist stream header",
            ));
        }
        received = received.checked_add(count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "nvlist size overflows usize")
        })?;
    }
    let byte_order_big = bytes[2] & 0x80 != 0;
    let mut size = [0u8; 8];
    size.copy_from_slice(&bytes[11..19]);
    let body_size = if byte_order_big {
        u64::from_be_bytes(size)
    } else {
        u64::from_le_bytes(size)
    };
    let expected = 19usize
        .checked_add(usize::try_from(body_size).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "nvlist size overflows usize")
        })?)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "nvlist size overflows usize"))?;
    if expected > bytes.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "nvlist exceeds transport buffer",
        ));
    }
    while received < expected {
        let mut iov = Iovec {
            base: bytes[received..].as_mut_ptr(),
            len: expected - received,
        };
        let mut continuation = MsgHdr {
            name: std::ptr::null_mut(),
            name_len: 0,
            iov: &mut iov,
            iov_len: 1,
            control: std::ptr::null_mut(),
            control_len: 0,
            flags: 0,
        };
        let count = syscall_result(raw_syscall3(
            SYS_RECVMSG,
            fd as usize,
            &mut continuation as *mut MsgHdr as usize,
            0,
        ))?;
        if continuation.flags & MSG_TRUNC != 0 || count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short nvlist stream frame",
            ));
        }
        received = received.checked_add(count).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "nvlist size overflows usize")
        })?;
    }
    if received != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "multiple nvlist frames in one receive",
        ));
    }
    let mut descriptors = Vec::new();
    let mut offset = 0usize;
    let control_len = header.control_len as usize;
    while offset + CMSG_HEADER_SIZE <= control_len {
        let length = get_u32(&control, offset) as usize;
        if length < CMSG_HEADER_SIZE || offset + length > control_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid nvlist control message",
            ));
        }
        let level = get_u32(&control, offset + 8) as i32;
        let kind = get_u32(&control, offset + 12) as i32;
        if level == SOL_SOCKET && kind == SCM_RIGHTS {
            let data_len = length - CMSG_HEADER_SIZE;
            if !data_len.is_multiple_of(size_of::<i32>()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid nvlist descriptor data",
                ));
            }
            for item in 0..data_len / size_of::<i32>() {
                let data_offset = offset + CMSG_HEADER_SIZE + item * size_of::<i32>();
                let mut value = [0; 4];
                value.copy_from_slice(&control[data_offset..data_offset + 4]);
                descriptors.push(i32::from_ne_bytes(value));
            }
        }
        offset += align(length);
    }
    bytes.truncate(received);
    Ok((bytes, descriptors))
}
