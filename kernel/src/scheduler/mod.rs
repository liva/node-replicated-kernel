//! Scheduling logic

use alloc::sync::Weak;
use core::intrinsics::unlikely;

use crate::error::KError;
use crate::kcb::{self, ArchSpecificKcb};
use crate::nr;
use crate::process::Executor;
use crate::process::ResumeHandle;

use crate::arch::timer;

/// Runs the process allocated to the given core.
pub fn schedule() -> ! {
    let kcb = kcb::get_kcb();

    // Are we the master/first thread in that replica?
    // Then we should set timer to periodically advance the state
    #[cfg(target_os = "none")]
    let is_replica_main_thread = {
        let thread = topology::MACHINE_TOPOLOGY.current_thread();
        thread.node().is_none()
            || thread
                .node()
                .unwrap()
                .threads()
                .next()
                .map(|t| t.id == thread.id)
                .unwrap_or(false)
    };
    #[cfg(not(target_os = "none"))]
    let is_replica_main_thread = false;

    // No process assigned to core? Figure out if there is one now:
    if unlikely(kcb.arch.current_process().is_err()) {
        kcb.replica.as_ref().map(|(replica, token)| {
            loop {
                let response =
                    replica.execute(nr::ReadOps::CurrentExecutor(kcb.arch.hwthread_id()), *token);

                match response {
                    Ok(nr::NodeResult::Executor(e)) => {
                        // We found a process, put it in the KCB
                        let no = kcb::get_kcb()
                            .arch
                            .swap_current_process(Weak::upgrade(&e).unwrap());
                        assert!(no.is_none(), "Handle the case where we replace a process.");
                        break;
                    }
                    Err(KError::NoExecutorForCore) => {
                        // There is no process, set a timer and go to sleep
                        if is_replica_main_thread {
                            for _i in 0..25_000 {
                                core::sync::atomic::spin_loop_hint();
                            }
                            continue;
                        } else {
                            timer::set(timer::DEFAULT_TIMER_DEADLINE + 1_000_000_000);
                        }
                        crate::arch::halt();
                    }
                    other => {
                        unreachable!(
                            "Unexpected return from ReadOps::CurrentExecutor {:?}.",
                            other
                        );
                    }
                };
            }
        });
    }
    debug_assert!(kcb.arch.current_process().is_ok(), "Require executor next.");

    // If we come here, we have a new process, dispatch it:
    unsafe {
        let rh = kcb::get_kcb().arch.current_process().map(|p| p.start());
        rh.unwrap().resume()
    }
}
