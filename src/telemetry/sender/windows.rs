use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, CREATE_NEW_PROCESS_GROUP, CREATE_UNICODE_ENVIRONMENT, DETACHED_PROCESS,
    PROCESS_INFORMATION, STARTUPINFOW,
};

pub(super) fn spawn(sender_arg: &str) -> Option<()> {
    let exe = env::current_exe().ok()?;
    let application_name = encode_wide(exe.as_os_str());
    let mut command_line = build_command_line(exe.as_os_str(), sender_arg);
    let startup_info = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();
    let creation_flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT;

    // SAFETY: all pointers are valid for the duration of the call, the UTF-16
    // buffers are mutable and NUL-terminated, and null security/env/cwd means
    // inherit current process settings without inheriting handles.
    let created = unsafe {
        CreateProcessW(
            application_name.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            false.into(),
            creation_flags,
            null(),
            null(),
            &startup_info,
            &mut process_info,
        )
    };
    if created == 0 {
        return None;
    }

    // SAFETY: successful CreateProcessW returns live handles which must be closed.
    unsafe {
        CloseHandle(process_info.hThread);
        CloseHandle(process_info.hProcess);
    }
    Some(())
}

fn encode_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn build_command_line(exe: &OsStr, sender_arg: &str) -> Vec<u16> {
    let mut command = Vec::new();
    command.push(b'"' as u16);
    command.extend(exe.encode_wide());
    command.push(b'"' as u16);
    command.push(b' ' as u16);
    command.extend(OsStr::new(sender_arg).encode_wide());
    command.push(0);
    command
}
