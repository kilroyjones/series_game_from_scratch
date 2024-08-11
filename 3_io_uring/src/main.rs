#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// This will appear as "inactive" in VSCode, but it's necessary for the
// bindings.
#[cfg(not(rust_analyzer))]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::io;
use std::mem::zeroed;
use std::ptr::null_mut;

/// Initializes an io_uring instance
///
/// The queue_depth will be the size for the submission queue while the function
/// will double that value for the completition queue. All other parameters are
/// set to 0, and thus the default value.
///
fn setup_io_uring(queue_depth: u32) -> io::Result<io_uring> {
    let mut ring: io_uring = unsafe { zeroed() };
    let ret = unsafe { io_uring_queue_init(queue_depth, &mut ring, 0) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(ring)
}

/// Submits a NOOP to the submission queue
///
/// We get a pointer to the shared memory instance of an SQE, or submission
/// queue entry. This is then loaded with some dummy data and submitted.
///
fn submit_noop(ring: &mut io_uring) -> io::Result<()> {
    unsafe {
        let sqe = io_uring_get_sqe(ring);
        if sqe.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to get SQE"));
        }

        io_uring_prep_nop(sqe);
        (*sqe).user_data = 0x88;

        let ret = io_uring_submit(ring);
        if ret < 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Wait for our submission to complete
///
/// We're blocking on the queue waiting for any thing finish. When we get one,
/// we print out the details.
///
fn wait_for_completion(ring: &mut io_uring) -> io::Result<()> {
    let mut cqe: *mut io_uring_cqe = null_mut();
    let ret = unsafe { io_uring_wait_cqe(ring, &mut cqe) };

    if ret < 0 {
        return Err(io::Error::last_os_error());
    }

    unsafe {
        println!("NOP completed with result: {}", (*cqe).res);
        println!("User data: 0x{:x}", (*cqe).user_data);
        io_uring_cqe_seen(ring, cqe);
    }

    Ok(())
}

fn main() -> io::Result<()> {
    let queue_depth: u32 = 1;
    let mut ring = setup_io_uring(queue_depth)?;

    println!("Submitting NOP operation");
    submit_noop(&mut ring)?;

    println!("Waiting for completion");
    wait_for_completion(&mut ring)?;

    unsafe { io_uring_queue_exit(&mut ring) };

    Ok(())
}
