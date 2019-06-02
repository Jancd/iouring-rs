//
// Kornilios Kourtis <kkourt@kkourt.io>
//
// vim: set expandtab softtabstop=4 tabstop=4 shiftwidth=4:
//

use libc;
use std::mem;
use std::io;
use std::convert::TryFrom;

use crate::kernel_abi::{
    SYS_io_uring_register,
    SYS_io_uring_enter,
    SYS_io_uring_setup
};

/*
 * Magic offsets for the application to mmap the data it needs
 */
const IORING_OFF_SQ_RING: i64 = 0;
const IORING_OFF_CQ_RING: i64 = 0x8000000;
const IORING_OFF_SQES:    i64 = 0x10000000;

/// mmap helper
fn mmap(len: libc::size_t, fd: libc::c_int, off: libc::off_t) -> *mut libc::c_void {
    let prot  = libc::PROT_READ | libc::PROT_WRITE;
    let flags = libc::MAP_SHARED | libc::MAP_POPULATE;
    let null = 0 as *mut libc::c_void;
    unsafe {
        libc::mmap(null, len, prot, flags, fd, off)
    }
}

/// Submission queue
struct SQ {
    khead: *mut u32,
    ktail: *mut u32,
    kring_mask: *mut u32,
    kring_entries: *mut u32,
    kflags: *mut u32,
    kdropped: *mut u32,
    array: *mut u32,

    sqes: *mut io_uring_sqe,
    sqe_head: u32,
    sqe_tail: u32,

    ring_sz: libc::size_t,
    ring_ptr: *mut libc::c_void,
}

/// Completion queue
struct CQ {
    khead: *mut u32,
    ktail: *mut u32,
    kring_mask: *mut u32,
    kring_entries: *mut u32,
    overflow: *mut u32,

    cqes: *mut io_uring_sqe,

    ring_sz: libc::size_t,
    ring_ptr: *mut libc::c_void,
}


pub struct IoUring {
    fd: libc::c_int,
    sq: SQ,
    cq: CQ,
}

type KernelRwf = libc::c_int;

#[repr(C)]
union io_uring_sqe_arg {
    rw_flags: KernelRwf,
    fsync_flags: u32,
    poll_events: u16,
    sync_range_flags: u32,
}

#[repr(C)]
union io_uring_sqe_idx {
    buf_index: u16,
    __pad2: [u64; 3],
}

#[repr(C)]
struct io_uring_sqe {
    opcode: u8,                /* type of operation for this sqe */
    flags: u8,                 /* IOSQE_ flags */
    ioprio: u16,               /* ioprio for the request */
    fd: i32,                   /* file descriptor to do IO on */
    off: u64,                  /* offset into file */
    addr: u64,                 /* pointer to buffer or iovecs */
    len: u32,                  /* buffer size or number of iovecs */
    arg: io_uring_sqe_arg,
    user_data: u64,
    idx: io_uring_sqe_idx,
}

#[repr(C)]
struct io_uring_cqe {
    user_data: u64,   /* sqe->data submission passed back */
    res: i32,         /* result code for this event */
    flags: u32,
}


#[repr(C)]
struct io_sqring_offsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    flags: u32,
    dropped: u32,
    array: u32,
    resv1: u32,
    resv2: u64,
}

#[repr(C)]
struct io_cqring_offsets {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    overflow: u32,
    cqes: u32,
    resv: [u64; 2],
}


#[repr(C)]
struct io_uring_params {
    sq_entries: u32,
    cq_entries: u32,
    flags: u32,
    sq_thread_cpu: u32,
    sq_thread_idle: u32,
    resv: [u32; 5],
    sq_off: io_sqring_offsets,
    cq_off: io_cqring_offsets,
}

unsafe fn io_uring_register(
    fd: libc::c_int,
    opcode: libc::c_uint,
    arg: *mut libc::c_void,
    nr_args: libc::c_uint)
-> libc::c_long {
    libc::syscall(SYS_io_uring_register, fd, opcode, arg, nr_args)
}

unsafe fn io_uring_setup(entries: libc::c_uint, params: *mut io_uring_params)
-> libc::c_int {
    let ret = libc::syscall(SYS_io_uring_setup, entries, params);
    libc::c_int::try_from(ret).unwrap_or(-1)
}

unsafe fn io_uring_enter(
    fd: libc::c_int,
    to_submit: libc::c_uint,
    min_complete: libc::c_uint,
    flags: libc::c_uint,
    sigset: *mut libc::sigset_t)
-> libc::c_long {
    // NB: From looking at the kernel code, the sigset size needs to match the kernel sigset size,
    // which I guess is different from sizeof(sigset_t) in userspace.
    //
    // References:
    //  liburing io_uring_enter wrapper
    //    http://git.kernel.dk/cgit/liburing/tree/src/syscall.c?id=1a90a51ecd678d4331990d7f696153b59583d378#n39
    //
    //  function called by io_uring_enter() syscall:
    //    https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/kernel/signal.c?h=v5.1#n2810
    //
    //  sigset kernel definition
    //    https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/x86/include/asm/signal.h?h=v5.1#n11
    //
    //  sigset GNU libc definition:
    //    http://www.sourceware.org/git/?p=glibc.git;a=blob;f=sysdeps/unix/sysv/linux/bits/types/__sigset_t.h;h=e2f18acf30f43496567b1511456089dcd1798425;hb=fef7c63cd5a5a3150dc9465687359351afab5010
    //    indeed, sizeof(sigset_t) is 128)
    //
    const NSIG_: libc::c_uint = 65;
    let sigset_size: libc::c_uint = NSIG_ / 8;
    libc::syscall(SYS_io_uring_enter, fd, to_submit, min_complete, flags, sigset, sigset_size)
}

impl IoUring {

    pub fn init(nentries: libc::c_uint) -> io::Result<IoUring> {

        let mut params: io_uring_params = unsafe { std::mem::zeroed() };
        let params_p = &mut params as *mut io_uring_params;
        let fd = unsafe { io_uring_setup(nentries, params_p) };
        if fd < 0 {
            return Err(io::Error::last_os_error())
        }

        let mut ret : IoUring = IoUring {
            fd: fd,
            sq: unsafe { std::mem::zeroed() },
            cq: unsafe { std::mem::zeroed() },
        };

        let err = ret.queue_mmap(&mut params);
        if err.is_err() {
            unsafe { libc::close(ret.fd); }
        }
        Ok(ret)
    }



    fn queue_mmap(&mut self, p: &mut io_uring_params) -> io::Result<()> {

        // convinience function for computing pointer offsets
        let ptr_off = |p: *const libc::c_void, off: u32| -> *mut libc::c_uint {
            let mut ptr = p as libc::uintptr_t;
            ptr += libc::uintptr_t::try_from(off).unwrap();
            ptr as *mut libc::c_uint
        };

        /*
         * mmap submission queue
         */
        let sq = &mut self.sq;

        // The addition of sq_off.array to the length of the region accounts for the fact that the
        // ring located at the end of the data structure.
        let sq_ring_sz  = {
            let s1 = libc::size_t::try_from(p.sq_off.array).unwrap();
            let s2 = libc::size_t::try_from(p.sq_entries).unwrap() * mem::size_of::<u32>();
            s1 + s2
        };

        // mmap the submission queue structure
        let sq_ring_ptr = {
            let ptr = mmap(sq_ring_sz, self.fd, IORING_OFF_SQ_RING);
            if ptr == libc::MAP_FAILED {
                return Err(io::Error::last_os_error())
            }
            ptr
        };

        let sqes_size = {
            let nentries = libc::size_t::try_from(p.sq_entries).unwrap();
            let esz = libc::size_t::try_from(mem::size_of::<io_uring_sqe>()).unwrap();
            nentries*esz
        };

        // mmap the submission queue entries array
        let sqes_ptr = {
            let sqp = mmap(sqes_size, self.fd, IORING_OFF_SQES);
            if sqp == libc::MAP_FAILED {
                unsafe { libc::munmap(sq_ring_ptr, sq_ring_sz) };
                return Err(io::Error::last_os_error());
            }
            sqp as *mut io_uring_sqe
        };

        // initialize the SQ structure
        // setup pointers to submission queue structure using the sq offsets
        *sq = {
            let ptr = sq_ring_ptr;
            let off : &io_sqring_offsets = &p.sq_off;
            SQ {
                khead         : ptr_off(ptr, off.head),
                ktail         : ptr_off(ptr, off.tail),
                kring_mask    : ptr_off(ptr, off.ring_mask),
                kring_entries : ptr_off(ptr, off.ring_entries),
                kflags        : ptr_off(ptr, off.flags),
                kdropped      : ptr_off(ptr, off.dropped),
                array         : ptr_off(ptr, off.array),
                sqes          : sqes_ptr,
                sqe_head      : 0,
                sqe_tail      : 0,
                ring_sz       : sq_ring_sz,
                ring_ptr      : ptr,
            }
        };

        // these two have to be the same so that the unmap when closing io_uring works properly
        assert_eq!(p.sq_entries, unsafe { *sq.kring_entries });

        /*
         * mmap completion queue
         */
        let cq = &mut self.cq;

        let cq_ring_sz = {
            let s1 = libc::size_t::try_from(p.cq_off.cqes).unwrap();
            let s2 = libc::size_t::try_from(p.cq_entries).unwrap() * mem::size_of::<io_uring_cqe>();
            s1 + s2
        };

        let cq_ring_ptr  = {
            let ptr = mmap(cq_ring_sz, self.fd, IORING_OFF_CQ_RING);
            if ptr == libc::MAP_FAILED {
                unsafe {
                    libc::munmap(sq_ring_ptr, sq_ring_sz);
                    libc::munmap(sqes_ptr as *mut libc::c_void, sqes_size);
                }
                return Err(io::Error::last_os_error())
            }
            ptr
        };

        *cq = {
            let ptr = cq_ring_ptr;
            let off : &io_cqring_offsets = &p.cq_off;
            CQ {
                khead: ptr_off(ptr, off.head),
                ktail: ptr_off(ptr, off.tail),
                kring_mask: ptr_off(ptr, off.ring_mask),
                kring_entries: ptr_off(ptr, off.ring_entries),
                overflow: ptr_off(ptr, off.overflow),
                cqes: ptr_off(ptr, off.cqes) as *mut io_uring_sqe,
                ring_sz: cq_ring_sz,
                ring_ptr: ptr
            }
        };

        Ok(())
    }
}
