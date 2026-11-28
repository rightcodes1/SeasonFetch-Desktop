//! Windows process launcher for the bundled streaming engine.
//!
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST` assigns the suspended child to its private job as part of
//! `CreateProcessW`, so there is no interval in which the child can run outside our ownership.

use seasonfetch_streaming::engine_process::{EngineChild, EngineProcessSpawner, EngineProcessSpec};
use std::ffi::{c_void, OsStr};
use std::io;
use std::mem::{size_of, size_of_val, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

const JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;

#[derive(Debug, Default)]
pub struct WindowsEngineProcessSpawner;

impl EngineProcessSpawner for WindowsEngineProcessSpawner {
    fn spawn(&self, spec: &EngineProcessSpec) -> io::Result<Box<dyn EngineChild>> {
        spawn_engine_process(spec).map(|child| Box::new(child) as Box<dyn EngineChild>)
    }
}

struct OwnedHandle(HANDLE);

// Kernel handles remain valid when moved between threads; ownership is exclusive and Drop closes
// exactly this handle. The process/job APIs used here are safe to call from any thread.
unsafe impl Send for OwnedHandle {}

impl OwnedHandle {
    fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct AttributeList {
    storage: Vec<usize>,
    initialized: bool,
}

impl AttributeList {
    fn new(attribute_count: u32) -> io::Result<Self> {
        let mut required_bytes = 0usize;
        // Windows documents this sizing call as intentionally failing while reporting the size.
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), attribute_count, 0, &mut required_bytes);
        }
        if required_bytes == 0 {
            return Err(io::Error::last_os_error());
        }

        // Vec<usize> provides the pointer alignment required by the opaque native attribute list.
        let word_count = required_bytes
            .checked_add(size_of::<usize>() - 1)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "attribute-list size overflow")
            })?
            / size_of::<usize>();
        let mut list = Self {
            storage: vec![0; word_count],
            initialized: false,
        };
        if unsafe {
            InitializeProcThreadAttributeList(
                list.as_mut_ptr(),
                attribute_count,
                0,
                &mut required_bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        list.initialized = true;
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> windows_sys::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.as_mut_ptr());
            }
        }
    }
}

struct EnvironmentStrings(*mut u16);

impl Drop for EnvironmentStrings {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                FreeEnvironmentStringsW(self.0);
            }
        }
    }
}

struct WindowsEngineChild {
    // Keep the job alive as long as the process handle is owned. Closing the last job handle
    // invokes KILL_ON_JOB_CLOSE and cleans up the child and any descendants it created.
    job: OwnedHandle,
    process: OwnedHandle,
    process_id: u32,
}

impl EngineChild for WindowsEngineChild {
    fn id(&self) -> u32 {
        self.process_id
    }

    fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match unsafe { WaitForSingleObject(self.process.raw(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => self.exit_code().map(Some),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            other => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("WaitForSingleObject returned unexpected status {other:#x}"),
            )),
        }
    }

    fn kill(&mut self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.job.raw(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    fn wait(&mut self) -> io::Result<u32> {
        match unsafe { WaitForSingleObject(self.process.raw(), INFINITE) } {
            WAIT_OBJECT_0 => self.exit_code(),
            WAIT_FAILED => Err(io::Error::last_os_error()),
            other => Err(io::Error::new(
                io::ErrorKind::Other,
                format!("WaitForSingleObject returned unexpected status {other:#x}"),
            )),
        }
    }
}

impl WindowsEngineChild {
    fn exit_code(&self) -> io::Result<u32> {
        let mut code = 0u32;
        if unsafe { GetExitCodeProcess(self.process.raw(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // The process handle is already signaled before this method is called, so every DWORD is
        // a legitimate exit code, including STILL_ACTIVE's numeric value.
        Ok(code)
    }
}

fn spawn_engine_process(spec: &EngineProcessSpec) -> io::Result<WindowsEngineChild> {
    let executable = nul_terminated(spec.executable.as_os_str())?;
    let mut command_line = make_command_line(spec)?;
    let current_directory = spec
        .current_dir
        .as_deref()
        .map(nul_terminated)
        .transpose()?;
    let environment = build_environment_block(&spec.environment)?;

    let job = OwnedHandle::new(unsafe { CreateJobObjectW(null(), null()) });
    if job.raw().is_null() {
        return Err(io::Error::last_os_error());
    }

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.raw(),
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let null_handles = open_null_standard_handles()?;
    let inherited_handles = [
        null_handles[0].raw(),
        null_handles[1].raw(),
        null_handles[2].raw(),
    ];
    let mut attributes = AttributeList::new(2)?;
    let job_handles = [job.raw()];
    if unsafe {
        UpdateProcThreadAttribute(
            attributes.as_mut_ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            job_handles.as_ptr().cast(),
            size_of_val(&job_handles),
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe {
        UpdateProcThreadAttribute(
            attributes.as_mut_ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited_handles.as_ptr().cast(),
            size_of_val(&inherited_handles),
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }

    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESHOWWINDOW | STARTF_USESTDHANDLES;
    startup.StartupInfo.wShowWindow = SW_HIDE as u16;
    startup.StartupInfo.hStdInput = inherited_handles[0];
    startup.StartupInfo.hStdOutput = inherited_handles[1];
    startup.StartupInfo.hStdError = inherited_handles[2];
    startup.lpAttributeList = attributes.as_mut_ptr();

    let mut process_info: PROCESS_INFORMATION = unsafe { zeroed() };
    let creation_flags = CREATE_SUSPENDED
        | EXTENDED_STARTUPINFO_PRESENT
        | CREATE_NO_WINDOW
        | CREATE_UNICODE_ENVIRONMENT;
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            creation_flags,
            environment
                .as_ref()
                .map_or(null(), |block| block.as_ptr().cast::<c_void>()),
            current_directory.as_ref().map_or(null(), |dir| dir.as_ptr()),
            (&startup as *const STARTUPINFOEXW).cast::<STARTUPINFOW>(),
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }

    // Wrap every returned handle immediately. On any later error, dropping `job` closes the
    // KILL_ON_JOB_CLOSE job and guarantees that a still-suspended process cannot escape.
    let process = OwnedHandle::new(process_info.hProcess);
    let thread = OwnedHandle::new(process_info.hThread);
    if process.raw().is_null() || thread.raw().is_null() {
        unsafe {
            TerminateJobObject(job.raw(), 1);
        }
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "CreateProcessW succeeded without returning both process and thread handles",
        ));
    }

    let previous_suspend_count = unsafe { ResumeThread(thread.raw()) };
    if previous_suspend_count == u32::MAX || previous_suspend_count != 1 {
        let error = if previous_suspend_count == u32::MAX {
            io::Error::last_os_error()
        } else {
            io::Error::new(
                io::ErrorKind::Other,
                format!("unexpected initial thread suspend count {previous_suspend_count}"),
            )
        };
        // Explicitly terminate; dropping the last job handle is the fallback if termination
        // itself fails. The process has never run outside its job.
        unsafe {
            TerminateJobObject(job.raw(), 1);
        }
        return Err(error);
    }
    drop(thread);
    drop(null_handles);
    drop(attributes);

    Ok(WindowsEngineChild {
        job,
        process,
        process_id: process_info.dwProcessId,
    })
}

fn open_null_standard_handles() -> io::Result<[OwnedHandle; 3]> {
    let mut path = "NUL".encode_utf16().collect::<Vec<_>>();
    path.push(0);
    let mut security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut handles = Vec::with_capacity(3);
    for _ in 0..3 {
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &mut security,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        handles.push(OwnedHandle::new(handle));
    }
    handles.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::Other,
            "failed to open all three NUL standard handles",
        )
    })
}

fn make_command_line(spec: &EngineProcessSpec) -> io::Result<Vec<u16>> {
    let mut result = quote_windows_argument(spec.executable.as_os_str());
    for arg in &spec.args {
        result.push(b' ' as u16);
        result.extend(quote_windows_argument(arg));
    }
    if result.iter().any(|unit| *unit == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows command lines cannot contain NUL characters",
        ));
    }
    if result.len() + 1 > 32_767 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows command line exceeds the 32,767 UTF-16 code-unit limit",
        ));
    }
    result.push(0);
    Ok(result)
}

fn quote_windows_argument(argument: &OsStr) -> Vec<u16> {
    let value: Vec<u16> = argument.encode_wide().collect();
    let must_quote = value.is_empty()
        || value
            .iter()
            .any(|unit| *unit == b' ' as u16 || *unit == b'\t' as u16 || *unit == b'"' as u16);
    if !must_quote {
        return value;
    }

    let mut quoted = Vec::with_capacity(value.len() + 2);
    quoted.push(b'"' as u16);
    let mut backslashes = 0usize;
    for unit in value {
        if unit == b'\\' as u16 {
            backslashes += 1;
        } else if unit == b'"' as u16 {
            quoted.extend(std::iter::repeat(b'\\' as u16).take(backslashes * 2 + 1));
            quoted.push(unit);
            backslashes = 0;
        } else {
            quoted.extend(std::iter::repeat(b'\\' as u16).take(backslashes));
            quoted.push(unit);
            backslashes = 0;
        }
    }
    quoted.extend(std::iter::repeat(b'\\' as u16).take(backslashes * 2));
    quoted.push(b'"' as u16);
    quoted
}

fn nul_terminated(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows paths cannot contain NUL characters",
        ));
    }
    wide.push(0);
    Ok(wide)
}

fn build_environment_block(
    overrides: &[(std::ffi::OsString, std::ffi::OsString)],
) -> io::Result<Option<Vec<u16>>> {
    if overrides.is_empty() {
        // A null lpEnvironment tells CreateProcessW to inherit the current environment directly.
        return Ok(None);
    }

    let source = unsafe { GetEnvironmentStringsW() };
    if source.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _source_guard = EnvironmentStrings(source);
    let mut entries: Vec<Vec<u16>> = Vec::new();
    unsafe {
        let mut cursor = source;
        while *cursor != 0 {
            let start = cursor;
            while *cursor != 0 {
                cursor = cursor.add(1);
            }
            entries.push(
                std::slice::from_raw_parts(start, cursor.offset_from(start) as usize).to_vec(),
            );
            cursor = cursor.add(1);
        }
    }

    for (name, value) in overrides {
        let name_wide: Vec<u16> = name.encode_wide().collect();
        let value_wide: Vec<u16> = value.encode_wide().collect();
        if name_wide.is_empty()
            || name_wide.contains(&0)
            || name_wide.contains(&(b'=' as u16))
            || value_wide.contains(&0)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Windows environment variable name or value",
            ));
        }

        // Windows environment variable names are case-insensitive. Keep the inherited block
        // (including drive-current-directory entries) and replace only the requested key.
        let lookup = String::from_utf16_lossy(&name_wide).to_lowercase();
        let matching_indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                environment_name_end(entry).and_then(|separator| {
                    (String::from_utf16_lossy(&entry[..separator]).to_lowercase() == lookup)
                        .then_some(index)
                })
            })
            .collect();
        let mut new_entry = name_wide;
        new_entry.push(b'=' as u16);
        new_entry.extend(value_wide);
        if let Some(index) = matching_indices.first().copied() {
            entries[index] = new_entry;
            for duplicate_index in matching_indices.into_iter().skip(1).rev() {
                entries.remove(duplicate_index);
            }
        } else {
            entries.push(new_entry);
        }
    }

    entries.sort_by(|left, right| {
        let left_end = environment_name_end(left).unwrap_or(left.len());
        let right_end = environment_name_end(right).unwrap_or(right.len());
        String::from_utf16_lossy(&left[..left_end])
            .to_lowercase()
            .cmp(&String::from_utf16_lossy(&right[..right_end]).to_lowercase())
    });

    let total_units = entries.iter().map(|entry| entry.len() + 1).sum::<usize>() + 1;
    let mut block = Vec::with_capacity(total_units.max(2));
    for entry in entries {
        block.extend(entry);
        block.push(0);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Ok(Some(block))
}

fn environment_name_end(entry: &[u16]) -> Option<usize> {
    // Environment entries such as `=C:=C:\work` have their separator at the second `=`.
    let start = usize::from(entry.first() == Some(&(b'=' as u16)));
    entry
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, unit)| (*unit == b'=' as u16).then_some(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn to_string(wide: &[u16]) -> String {
        String::from_utf16(wide).unwrap()
    }

    #[test]
    fn command_line_quoting_handles_spaces_quotes_and_trailing_slashes() {
        assert_eq!(to_string(&quote_windows_argument(OsStr::new(""))), "\"\"");
        assert_eq!(
            to_string(&quote_windows_argument(OsStr::new("plain"))),
            "plain"
        );
        assert_eq!(
            to_string(&quote_windows_argument(OsStr::new("two words"))),
            "\"two words\""
        );
        assert_eq!(
            to_string(&quote_windows_argument(OsStr::new("ends with slash\\"))),
            r#""ends with slash\\""#
        );
        assert_eq!(
            to_string(&quote_windows_argument(OsStr::new("a\\\"b"))),
            r#""a\\\"b""#
        );
    }

    fn cmd_spec(command: &str, environment: Vec<(OsString, OsString)>) -> EngineProcessSpec {
        let executable = std::env::var_os("COMSPEC")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        EngineProcessSpec {
            executable,
            args: vec![
                OsString::from("/D"),
                OsString::from("/C"),
                OsString::from(command),
            ],
            current_dir: None,
            environment,
        }
    }

    #[test]
    fn child_is_in_owned_job_and_inherits_environment_overrides() {
        let mut child = spawn_engine_process(&cmd_spec(
            "ping -n 3 127.0.0.1 > NUL & exit %SEASONFETCH_CHILD_EXIT%",
            vec![(
                OsString::from("SEASONFETCH_CHILD_EXIT"),
                OsString::from("37"),
            )],
        ))
        .expect("create suspended child in the configured job");
        assert_ne!(child.process_id, 0);
        let mut in_job = 0;
        assert_eq!(
            unsafe { IsProcessInJob(child.process.raw(), child.job.raw(), &mut in_job) },
            1
        );
        assert_ne!(in_job, 0, "child must be assigned before it is resumed");
        assert_eq!(child.wait().expect("wait for exact child handle"), 37);
    }

    #[test]
    fn kill_terminates_only_the_child_job_and_waits_on_child_handle() {
        let mut child = spawn_engine_process(&cmd_spec("ping -n 30 127.0.0.1 > NUL", Vec::new()))
            .expect("create child process");
        child.kill().expect("terminate the owned job");
        assert_eq!(child.wait().expect("wait on exact process handle"), 1);
    }
}
