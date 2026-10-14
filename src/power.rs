//! Deliver system sleep notifications on the application's control thread.

use std::{cell::Cell, ffi::c_void, ptr};

const CAN_SLEEP: u32 = 0xe000_0270;
const WILL_SLEEP: u32 = 0xe000_0280;
const HANDLED_SOURCE: i32 = 4;

type CfRef = *const c_void;
type NotificationPort = *mut c_void;
type Callback = unsafe extern "C" fn(*mut c_void, u32, u32, *mut c_void);

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        context: *mut c_void,
        port: *mut NotificationPort,
        callback: Callback,
        notifier: *mut u32,
    ) -> u32;
    fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
    fn IOAllowPowerChange(connection: u32, token: isize) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IONotificationPortGetRunLoopSource(port: NotificationPort) -> CfRef;
    fn IONotificationPortDestroy(port: NotificationPort);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopDefaultMode: CfRef;
    fn CFRunLoopGetCurrent() -> CfRef;
    fn CFRunLoopAddSource(run_loop: CfRef, source: CfRef, mode: CfRef);
    fn CFRunLoopRemoveSource(run_loop: CfRef, source: CfRef, mode: CfRef);
    fn CFRunLoopRunInMode(mode: CfRef, seconds: f64, return_after_source: u8) -> i32;
}

#[derive(Default)]
struct State {
    connection: Cell<u32>,
    pending: Cell<Option<isize>>,
}

impl State {
    fn message(&self, message: u32, token: isize) -> Option<isize> {
        match message {
            CAN_SLEEP => Some(token),
            WILL_SLEEP => {
                self.pending.set(Some(token));
                None
            }
            _ => None,
        }
    }
}

unsafe extern "C" fn power_changed(
    context: *mut c_void,
    _service: u32,
    message: u32,
    argument: *mut c_void,
) {
    // The port only dispatches on the owning run loop; Cell avoids aliasing a live &mut PowerWatch.
    let state = unsafe { &*context.cast::<State>() };
    if let Some(token) = state.message(message, argument as isize) {
        let result = unsafe { IOAllowPowerChange(state.connection.get(), token) };
        if result != 0 {
            state.pending.set(Some(token));
        }
    }
}

/// Keep this on its creating thread, and construct it before the owned audio engine.
pub struct PowerWatch {
    state: Box<State>,
    port: NotificationPort,
    notifier: u32,
    run_loop: CfRef,
    source: CfRef,
}

impl PowerWatch {
    pub fn new() -> Result<Self, String> {
        let mut watch = Self {
            state: Box::default(),
            port: ptr::null_mut(),
            notifier: 0,
            run_loop: unsafe { CFRunLoopGetCurrent() },
            source: ptr::null(),
        };
        let connection = unsafe {
            IORegisterForSystemPower(
                ptr::from_ref(watch.state.as_ref()).cast_mut().cast(),
                &mut watch.port,
                power_changed,
                &mut watch.notifier,
            )
        };
        watch.state.connection.set(connection);
        if connection == 0 || watch.port.is_null() || watch.notifier == 0 {
            return Err("IORegisterForSystemPower failed to register sleep notifications".into());
        }
        watch.source = unsafe { IONotificationPortGetRunLoopSource(watch.port) };
        if watch.source.is_null() || watch.run_loop.is_null() {
            return Err("IOKit did not provide a power notification run-loop source".into());
        }
        unsafe { CFRunLoopAddSource(watch.run_loop, watch.source, kCFRunLoopDefaultMode) };
        Ok(watch)
    }

    /// Dispatch queued notifications without waiting. True requests audio teardown before sleep.
    pub fn poll(&mut self) -> bool {
        // Bound draining so unrelated run-loop sources cannot starve the application's control loop.
        for _ in 0..32 {
            if self.state.pending.get().is_some() {
                return true;
            }
            let result = unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 1) };
            if result != HANDLED_SOURCE {
                break;
            }
        }
        self.state.pending.get().is_some()
    }

    /// Call after stopping callbacks and destroying the application's aggregate device.
    pub fn allow_sleep(&mut self) -> Result<(), String> {
        if let Some(token) = self.state.pending.get() {
            let result = unsafe { IOAllowPowerChange(self.state.connection.get(), token) };
            if result != 0 {
                return Err(format!("IOAllowPowerChange failed: IOReturn {result}"));
            }
            self.state.pending.set(None);
        }
        Ok(())
    }
}

impl Drop for PowerWatch {
    fn drop(&mut self) {
        if !self.source.is_null() && !self.run_loop.is_null() {
            unsafe { CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopDefaultMode) };
        }
        let _ = self.allow_sleep();
        if self.notifier != 0 {
            unsafe { IODeregisterForSystemPower(&mut self.notifier) };
        }
        // The notification port owns its run-loop source; releasing the source separately is invalid.
        if !self.port.is_null() {
            unsafe { IONotificationPortDestroy(self.port) };
        }
        if self.state.connection.get() != 0 {
            unsafe { IOServiceClose(self.state.connection.get()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_sleep_is_allowed_but_will_sleep_waits_for_teardown() {
        let state = State::default();
        assert_eq!(state.message(CAN_SLEEP, 41), Some(41));
        assert_eq!(state.pending.get(), None);
        assert_eq!(state.message(WILL_SLEEP, 42), None);
        assert_eq!(state.pending.get(), Some(42));
        assert_eq!(state.message(0xe000_0300, 0), None);
        assert_eq!(state.pending.get(), Some(42));
    }

    #[test]
    fn zero_is_a_valid_notification_token() {
        let state = State::default();
        assert_eq!(state.message(WILL_SLEEP, 0), None);
        assert_eq!(state.pending.get(), Some(0));
    }
}
