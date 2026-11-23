//! Single-instance guard and activation signal.
//!
//! The first process owns a named mutex and a named auto-reset event. A second
//! process signals the event and exits; the first process waits on it and shows
//! the main window. No argument forwarding is needed yet.

use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR};

fn instance_names() -> (HSTRING, HSTRING) {
    // Test seam: process-level startup tests must not collide with a running
    // developer Client.
    let suffix = std::env::var("CYNAPSE_TEST_INSTANCE_SUFFIX").unwrap_or_default();
    (
        HSTRING::from(format!("Local\\com.tubumu.cynapseclient.mutex{suffix}")),
        HSTRING::from(format!("Local\\com.tubumu.cynapseclient.activate{suffix}")),
    )
}

pub struct SingleInstance {
    mutex: HANDLE,
    event: HANDLE,
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.mutex);
            let _ = CloseHandle(self.event);
        }
    }
}

/// Why this process may or may not own the guard. The three cases exit
/// differently: only `AlreadyRunning` is a normal exit, and only `Failed` is an
/// error the user has to see.
pub enum Acquire {
    /// This process owns the guard; keep it alive for the whole run.
    Primary(SingleInstance),
    /// Another instance owns the guard and was told to show its window.
    AlreadyRunning,
    /// Win32 refused to set up the guard or to reach the running instance.
    Failed(String),
}

/// Takes the guard. The activation sender receives one message per second
/// launch, so the caller keeps it even though the first launch never uses it.
pub fn acquire(activated: UnboundedSender<()>) -> Acquire {
    // Test seam: the startup test cannot make Win32 fail on demand, so the
    // reporting path would otherwise never run.
    if let Ok(reason) = std::env::var("CYNAPSE_TEST_INSTANCE_FAILURE") {
        return Acquire::Failed(format!("injected single-instance failure: {reason}"));
    }

    let (mutex_name, event_name) = instance_names();
    let mutex = match unsafe { CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())) } {
        Ok(mutex) => mutex,
        Err(error) => return Acquire::Failed(format!("CreateMutexW failed: {error}")),
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return match signal_existing(&event_name) {
            Ok(()) => Acquire::AlreadyRunning,
            Err(error) => Acquire::Failed(error),
        };
    }

    let event = match unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) } {
        Ok(event) => event,
        Err(error) => {
            unsafe {
                let _ = CloseHandle(mutex);
            }
            return Acquire::Failed(format!("CreateEventW failed: {error}"));
        }
    };
    let event_raw = event.0 as isize;
    let spawned = std::thread::Builder::new()
        .name("CynapseActivation".to_string())
        .spawn(move || {
            loop {
                let event = HANDLE(event_raw as *mut _);
                let result = unsafe { WaitForSingleObject(event, INFINITE) };
                if result != windows::Win32::Foundation::WAIT_OBJECT_0 {
                    break;
                }
                if activated.send(()).is_err() {
                    break;
                }
            }
        });
    match spawned {
        Ok(_) => Acquire::Primary(SingleInstance { mutex, event }),
        Err(error) => {
            // The guard never became usable, so release what this attempt took
            // rather than leaving the mutex and event held by a process that
            // is about to exit.
            unsafe {
                let _ = CloseHandle(event);
                let _ = CloseHandle(mutex);
            }
            Acquire::Failed(format!("could not start the activation listener: {error}"))
        }
    }
}

fn signal_existing(event_name: &HSTRING) -> Result<(), String> {
    // The owner creates the event just after the mutex, so a launch that loses
    // the race needs a few retries before it can signal.
    let mut last = String::from("OpenEventW failed: unknown error");
    for _ in 0..20 {
        match unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_name.as_ptr())) } {
            Ok(event) => {
                let set = unsafe { SetEvent(event) };
                unsafe {
                    let _ = CloseHandle(event);
                }
                return set.map_err(|error| format!("SetEvent failed: {error}"));
            }
            Err(error) => last = format!("OpenEventW failed: {error}"),
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(last)
}
