//! The Windows side: processes made with CreateProcessW so a child inherits only the handles
//! named, and nodes kept in named job objects, asked to stop with Ctrl-Break or WM_CLOSE.

use std::ffi::{OsStr, OsString, c_void};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, FILETIME, GENERIC_READ, GENERIC_WRITE,
    GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LocalFree, STILL_ACTIVE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, CTRL_BREAK_EVENT, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleWindow, GetStdHandle, STD_ERROR_HANDLE,
    STD_HANDLE, STD_INPUT_HANDLE, SetConsoleCtrlHandler,
};
use windows_sys::Win32::System::Environment::{FreeEnvironmentStringsW, GetEnvironmentStringsW};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, OpenJobObjectW,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows_sys::Win32::System::SystemServices::{JOB_OBJECT_QUERY, JOB_OBJECT_TERMINATE};
use windows_sys::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DETACHED_PROCESS, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetCurrentProcessId,
    GetExitCodeProcess, GetProcessTimes, INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
    OpenProcessToken, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, PostMessageW, WM_CLOSE};

use crate::{Command, Result, Stopped, Tracking, environment, error, quote, require_program, wait_until};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

/// An owned handle, None for the two values Windows uses for none.
fn owned(h: HANDLE) -> Option<OwnedHandle> {
    // SAFETY: a valid handle a call just gave this process, owned by nothing else.
    (!h.is_null() && h != INVALID_HANDLE_VALUE).then(|| unsafe { OwnedHandle::from_raw_handle(h) })
}

fn raw(h: &OwnedHandle) -> HANDLE {
    h.as_raw_handle()
}

pub(crate) fn runnable(p: &Path) -> Option<PathBuf> {
    let exts = std::env::var("PATHEXT").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
    let exts: Vec<String> = exts.split(';').filter(|e| !e.is_empty()).map(str::to_lowercase).collect();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase()));
    if ext.is_some_and(|e| exts.contains(&e)) && p.is_file() {
        return Some(p.to_path_buf());
    }
    exts.iter()
        .map(|e| {
            let mut name = p.as_os_str().to_owned();
            name.push(e);
            PathBuf::from(name)
        })
        .find(|c| c.is_file())
}

/// Every entry, those such as `=C:=C:\x` that cmd.exe keeps each drive's folder in too.
pub(crate) fn inherited_environment() -> Vec<String> {
    let mut out = Vec::new();
    // SAFETY: the block is a run of NUL ended strings ending with an empty one, freed once read.
    unsafe {
        let block = GetEnvironmentStringsW();
        let mut at = block;
        while !at.is_null() && *at != 0 {
            let len = (0..).take_while(|&i| *at.add(i) != 0).count();
            out.push(String::from_utf16_lossy(std::slice::from_raw_parts(at, len)));
            at = at.add(len + 1);
        }
        if !block.is_null() {
            FreeEnvironmentStringsW(block);
        }
    }
    out
}

fn system_dir() -> PathBuf {
    let mut buf = [0u16; 512];
    // SAFETY: the length passed is the buffer's.
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    PathBuf::from(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
}

/// What CreateProcessW takes: the program and the one command line. A batch file runs
/// through cmd.exe with arguments it passes on as data.
fn launch_for(c: &Command) -> Result<(Vec<u16>, Vec<u16>)> {
    let program = require_program(c)?;
    let ext = program.extension().map(|e| e.to_string_lossy().to_lowercase());
    if matches!(ext.as_deref(), Some("bat" | "cmd")) {
        let line = quote::batch_command_line(&program.to_string_lossy(), &c.argv[1..])
            .map_err(|e| error(format!("cannot run {}: {e}", program.display())))?;
        return Ok((wide(system_dir().join("cmd.exe")), wide(line)));
    }
    let mut argv = c.argv.clone();
    argv[0] = program.to_string_lossy().into_owned();
    Ok((wide(&program), wide(quote::windows_command_line(&argv))))
}

/// A double NUL ended block, sorted as CreateProcessW expects.
fn env_block(c: &Command) -> Vec<u16> {
    let mut block: Vec<u16> = environment(c).iter().flat_map(|e| e.encode_utf16().chain(Some(0))).collect();
    block.push(0);
    block
}

fn inheritable() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: null_mut(), bInheritHandle: 1 }
}

fn open_file(path: &Path, access: u32, share: u32, disposition: u32) -> Result<OwnedHandle> {
    let sa = inheritable();
    // SAFETY: the name is NUL ended and sa outlives the call.
    let h = unsafe { CreateFileW(wide(path).as_ptr(), access, share, &sa, disposition, FILE_ATTRIBUTE_NORMAL, null_mut()) };
    owned(h).ok_or_else(|| error(format!("cannot open {}: {}", path.display(), last_error())))
}

fn open_null(access: u32) -> Result<OwnedHandle> {
    open_file(Path::new("NUL"), access, FILE_SHARE_READ | FILE_SHARE_WRITE, OPEN_EXISTING)
}

/// An inheritable copy of a standard handle, or the null device when there is none.
fn std_copy(which: STD_HANDLE, null_access: u32) -> Result<OwnedHandle> {
    // SAFETY: the copy goes into dup, owned from then on.
    unsafe {
        let h = GetStdHandle(which);
        let mut dup: HANDLE = null_mut();
        let me = GetCurrentProcess();
        let valid = !h.is_null() && h != INVALID_HANDLE_VALUE;
        if valid && DuplicateHandle(me, h, me, &mut dup, 0, 1, DUPLICATE_SAME_ACCESS) != 0 {
            return owned(dup).ok_or_else(|| error("cannot copy a standard handle"));
        }
    }
    open_null(null_access)
}

/// A started process: its handles and id.
struct Started {
    process: OwnedHandle,
    thread: OwnedHandle,
    pid: u32,
}

/// CreateProcessW where the child inherits exactly the handles given, never a lock file or
/// socket rant holds. Each handle must be inheritable.
fn create(c: &Command, flags: u32, stdio: [HANDLE; 3], extra: &[HANDLE]) -> Result<Started> {
    let (app, mut cmdline) = launch_for(c)?;
    let env = env_block(c);
    let cwd = wide(&c.cwd);
    let cwd_ptr = if c.cwd.as_os_str().is_empty() { null() } else { cwd.as_ptr() };
    let mut list: Vec<HANDLE> = stdio.iter().chain(extra).copied().collect();
    list.sort();
    list.dedup();

    // SAFETY: the attribute list lives in mem, aligned for pointers, until it is deleted, and
    // every pointer CreateProcessW takes outlives the call.
    unsafe {
        let mut size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut mem = vec![0usize; size.div_ceil(size_of::<usize>())];
        let attrs = mem.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) == 0 {
            return Err(error(format!("cannot set up the process: {}", last_error())));
        }
        let listed = UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            list.as_ptr() as *const c_void,
            list.len() * size_of::<HANDLE>(),
            null_mut(),
            null(),
        );
        if listed == 0 {
            let why = last_error();
            DeleteProcThreadAttributeList(attrs);
            return Err(error(format!("cannot set up the process: {why}")));
        }
        let mut si = STARTUPINFOEXW::default();
        si.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        [si.StartupInfo.hStdInput, si.StartupInfo.hStdOutput, si.StartupInfo.hStdError] = stdio;
        si.lpAttributeList = attrs;
        let mut pi = PROCESS_INFORMATION::default();
        let all = flags | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;
        let mut call = |flags| {
            let env = env.as_ptr() as *const c_void;
            CreateProcessW(app.as_ptr(), cmdline.as_mut_ptr(), null(), null(), 1, flags, env, cwd_ptr, &si.StartupInfo, &mut pi)
        };
        let mut ok = call(all);
        // a terminal may run rant in a job that kills its nodes as it closes, so a detached
        // start tries to leave it, and stays when the job forbids leaving
        if ok == 0 && flags & CREATE_BREAKAWAY_FROM_JOB != 0 && GetLastError() == ERROR_ACCESS_DENIED {
            ok = call(all & !CREATE_BREAKAWAY_FROM_JOB);
        }
        let why = last_error();
        DeleteProcThreadAttributeList(attrs);
        if ok == 0 {
            return Err(error(format!("cannot run {}: {why}", c.argv[0])));
        }
        match (owned(pi.hProcess), owned(pi.hThread)) {
            (Some(process), Some(thread)) => Ok(Started { process, thread, pid: pi.dwProcessId }),
            _ => Err(error(format!("cannot run {}: no process handle", c.argv[0]))),
        }
    }
}

fn exit_code(process: &OwnedHandle) -> Option<u32> {
    let mut code = 0u32;
    // SAFETY: a live process handle.
    (unsafe { GetExitCodeProcess(raw(process), &mut code) } != 0 && code != STILL_ACTIVE as u32).then_some(code)
}

/// A rant without a console window, as the service process is, never opens one for a child.
fn console_flags() -> u32 {
    // SAFETY: no arguments.
    if unsafe { GetConsoleWindow() }.is_null() { CREATE_NO_WINDOW } else { 0 }
}

fn wait(p: &Started) -> i32 {
    // SAFETY: a live process handle.
    unsafe { WaitForSingleObject(raw(&p.process), INFINITE) };
    exit_code(&p.process).unwrap_or(1) as i32
}

pub(crate) fn run(c: &Command) -> Result<i32> {
    let input = std_copy(STD_INPUT_HANDLE, GENERIC_READ)?;
    let err = std_copy(STD_ERROR_HANDLE, GENERIC_WRITE)?;
    let p = create(c, console_flags(), [raw(&input), raw(&err), raw(&err)], &[])?;
    Ok(wait(&p))
}

pub(crate) fn output(c: &Command) -> Option<String> {
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    let sa = inheritable();
    // SAFETY: both ends land in read and write, owned from then on.
    if unsafe { CreatePipe(&mut read, &mut write, &sa, 0) } == 0 {
        return None;
    }
    let (read, write) = (owned(read)?, owned(write)?);
    let input = open_null(GENERIC_READ).ok()?;
    let null_err = open_null(GENERIC_WRITE).ok()?;
    let started = create(c, console_flags(), [raw(&input), raw(&write), raw(&null_err)], &[]);
    // the child holds the only write end left, so reading ends when it exits
    drop(write);
    let p = started.ok()?;
    let mut text = Vec::new();
    std::fs::File::from(read).read_to_end(&mut text).ok()?;
    (wait(&p) == 0).then(|| String::from_utf8_lossy(&text).into_owned())
}

fn creation_time(process: &OwnedHandle) -> u64 {
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: a live process handle and four times to write.
    if unsafe { GetProcessTimes(raw(process), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return 0;
    }
    ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64
}

/// A security descriptor that lets this user and the system open an object from any session,
/// since the boot task runs rant in another session with other defaults.
fn user_only_security() -> *mut c_void {
    static SD: OnceLock<usize> = OnceLock::new();
    // SAFETY: the token buffer is aligned for TOKEN_USER and every string is freed once used.
    *SD.get_or_init(|| unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return 0;
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0u32;
        GetTokenInformation(raw(&token), TokenUser, null_mut(), 0, &mut size);
        let mut buf = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(raw(&token), TokenUser, buf.as_mut_ptr().cast(), size, &mut size) == 0 {
            return 0;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return 0;
        }
        let len = (0..).take_while(|&i| *sid.add(i) != 0).count();
        let text = format!("D:(A;;GA;;;{})(A;;GA;;;SY)", String::from_utf16_lossy(std::slice::from_raw_parts(sid, len)));
        LocalFree(sid.cast());
        let mut sd = null_mut();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(text).as_ptr(), SDDL_REVISION_1, &mut sd, null_mut());
        sd as usize
    }) as *mut c_void
}

/// A detached process this one started and stays the parent of, so it learns its exit code.
pub struct Child {
    tracking: Tracking,
    process: OwnedHandle,
    code: Option<i32>,
}

impl Child {
    pub fn tracking(&self) -> &Tracking {
        &self.tracking
    }

    /// Its exit code once all of its job ended, None while any process of it runs.
    pub fn ended(&mut self) -> Option<i32> {
        if self.code.is_none() && !alive(&self.tracking) {
            self.code = exit_code(&self.process).map(|c| c as i32);
        }
        self.code
    }
}

pub fn start_child(c: &Command, log: &Path, job_name: &str) -> Result<Child> {
    let out = open_file(log, FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, OPEN_ALWAYS)?;
    let input = open_null(GENERIC_READ)?;
    // The node inherits a handle to its own job, which keeps the job's name findable after
    // rant exits, since a named object loses its name with its last handle.
    let mut job_sa = inheritable();
    job_sa.lpSecurityDescriptor = user_only_security();
    // SAFETY: sa and the name outlive the call, and the last error is read right after it.
    let (job, existed) = unsafe {
        let h = CreateJobObjectW(&job_sa, wide(job_name).as_ptr());
        (owned(h), GetLastError() == ERROR_ALREADY_EXISTS)
    };
    let job = job.ok_or_else(|| error(format!("cannot create job {job_name}: {}", last_error())))?;
    if existed {
        return Err(error(format!("job {job_name} already exists: an earlier start of this node is still running")));
    }
    // a process inside may leave when it asks, as a rant it runs does for each node it starts
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    let info = (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast();
    // SAFETY: info points at limits, of the size given.
    if unsafe { SetInformationJobObject(raw(&job), JobObjectExtendedLimitInformation, info, size_of_val(&limits) as u32) } == 0 {
        return Err(error(format!("cannot set up job {job_name}: {}", last_error())));
    }
    let flags = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB;
    let p = create(c, flags, [raw(&input), raw(&out), raw(&out)], &[raw(&job)])?;
    // SAFETY: the job and the suspended process are live.
    unsafe {
        if AssignProcessToJobObject(raw(&job), raw(&p.process)) == 0 {
            let why = last_error();
            TerminateProcess(raw(&p.process), 1);
            return Err(error(format!("cannot track {} in a job: {why}", c.argv[0])));
        }
        ResumeThread(raw(&p.thread));
    }
    let tracking = Tracking { pid: p.pid as u64, start_time: creation_time(&p.process), job: job_name.to_string() };
    Ok(Child { tracking, process: p.process, code: None })
}

fn open_job(t: &Tracking, access: u32) -> Option<OwnedHandle> {
    if t.job.is_empty() {
        return None;
    }
    // SAFETY: the name is NUL ended.
    owned(unsafe { OpenJobObjectW(access, 0, wide(&t.job).as_ptr()) })
}

fn active_processes(job: &OwnedHandle) -> u32 {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size = size_of_val(&info) as u32;
    // SAFETY: info is of the size given.
    let ok = unsafe { QueryInformationJobObject(raw(job), JobObjectBasicAccountingInformation, &mut info as *mut _ as *mut c_void, size, null_mut()) };
    if ok == 0 { 0 } else { info.ActiveProcesses }
}

pub fn alive(t: &Tracking) -> bool {
    open_job(t, JOB_OBJECT_QUERY).is_some_and(|job| active_processes(&job) > 0)
}

fn open_process(pid: u64, access: u32) -> Option<OwnedHandle> {
    // SAFETY: no pointers.
    owned(unsafe { OpenProcess(access, 0, pid as u32) })
}

pub fn owns(t: &Tracking, pid: u64) -> bool {
    let (Some(job), Some(process)) = (open_job(t, JOB_OBJECT_QUERY), open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)) else {
        return false;
    };
    let mut inside = 0;
    // SAFETY: two live handles.
    unsafe { IsProcessInJob(raw(&process), raw(&job), &mut inside) != 0 && inside != 0 }
}

pub fn pid_alive(pid: u64) -> bool {
    open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION).is_some_and(|p| exit_code(&p).is_none())
}

/// The ids of the processes in the job, for a console to attach to once the first is gone.
fn job_pids(job: &OwnedHandle) -> Vec<u32> {
    // room for 64 ids after the two counts
    let mut buf = vec![0usize; 1 + 64];
    let list = buf.as_mut_ptr() as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST;
    let size = (buf.len() * size_of::<usize>()) as u32;
    // SAFETY: the buffer holds the size given, and only the ids it counts are read.
    unsafe {
        if QueryInformationJobObject(raw(job), JobObjectBasicProcessIdList, list.cast(), size, null_mut()) == 0 {
            return Vec::new();
        }
        let ids = (&raw const (*list).ProcessIdList) as *const usize;
        (0..(*list).NumberOfProcessIdsInList as usize).map(|i| *ids.add(i) as u32).collect()
    }
}

const HELPER_FLAG: &str = "--rant-ctrl-break";

/// Runs this executable as the Ctrl-Break helper, with no console of its own, and waits.
/// True when the event went out.
fn send_ctrl_break(group: u64, attach: u32) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let mut cmdline = wide(format!("\"{}\" {HELPER_FLAG} {group} {attach}", me.display()));
    let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: every pointer outlives the call, and the handles it gives are closed here.
    unsafe {
        if CreateProcessW(wide(&me).as_ptr(), cmdline.as_mut_ptr(), null(), null(), 0, DETACHED_PROCESS, null(), null(), &si, &mut pi) == 0 {
            return false;
        }
        CloseHandle(pi.hThread);
        let process = OwnedHandle::from_raw_handle(pi.hProcess);
        WaitForSingleObject(raw(&process), 3000) == WAIT_OBJECT_0 && exit_code(&process) == Some(0)
    }
}

/// `rant --rant-ctrl-break GROUP PID`, which stop runs to join the node's console and send
/// Ctrl-Break, so rant never leaves its own console. Its exit code, None for any other argv.
pub fn helper(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let args: Vec<OsString> = args.into_iter().collect();
    if args.len() != 4 || args[1] != HELPER_FLAG {
        return None;
    }
    let number = |a: &OsString| a.to_str().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let (group, attach) = (number(&args[2]), number(&args[3]));
    // SAFETY: console calls on this process alone.
    unsafe {
        FreeConsole();
        if AttachConsole(attach) == 0 {
            return Some(1);
        }
        // the event must not stop this helper first
        SetConsoleCtrlHandler(None, 1);
        let sent = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group);
        FreeConsole();
        Some(if sent != 0 { 0 } else { 1 })
    }
}

/// A program of the Windows GUI subsystem, such as the explorer, gets no console, so Ctrl-Break
/// never reaches it. Read from the subsystem field of its exe.
fn gui_program(pid: u32) -> bool {
    let Some(process) = open_process(pid as u64, PROCESS_QUERY_LIMITED_INFORMATION) else { return false };
    let mut path = vec![0u16; 32768];
    let mut n = path.len() as u32;
    // SAFETY: n is the buffer's length.
    if unsafe { QueryFullProcessImageNameW(raw(&process), 0, path.as_mut_ptr(), &mut n) } == 0 {
        return false;
    }
    let Ok(mut exe) = std::fs::File::open(String::from_utf16_lossy(&path[..n as usize])) else { return false };
    let mut dos = [0u8; 64];
    if exe.read_exact(&mut dos).is_err() || &dos[..2] != b"MZ" {
        return false;
    }
    // past the PE signature and file header, the field sits at one offset in 32 and 64 bit images
    let pe = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    let mut subsystem = [0u8; 2];
    const GUI: u16 = 2;
    exe.seek(SeekFrom::Start(pe + 4 + 20 + 68)).is_ok() && exe.read_exact(&mut subsystem).is_ok() && u16::from_le_bytes(subsystem) == GUI
}

/// Posts WM_CLOSE to every top level window of the processes, hidden ones too, which is how
/// taskkill asks a program with no console to stop. True when there was one.
fn close_windows(pids: &[u32]) -> bool {
    struct Search<'a> {
        pids: &'a [u32],
        found: bool,
    }
    unsafe extern "system" fn each(w: HWND, arg: LPARAM) -> i32 {
        // SAFETY: arg is the Search close_windows passed, alive for the whole enumeration.
        unsafe {
            let s = &mut *(arg as *mut Search);
            let mut pid = 0;
            GetWindowThreadProcessId(w, &mut pid);
            if s.pids.contains(&pid) && GetWindow(w, GW_OWNER).is_null() {
                PostMessageW(w, WM_CLOSE, 0, 0);
                s.found = true;
            }
        }
        1
    }
    let mut search = Search { pids, found: false };
    // SAFETY: search outlives the enumeration.
    unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
    search.found
}

/// Sends Ctrl-Break, or WM_CLOSE to a GUI program, until it goes out, since one just started
/// may have no console or window yet. Then waits for the job to empty, all within grace.
fn ask_to_stop(t: &Tracking, job: &OwnedHandle, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    let mut sent = false;
    while alive(t) {
        if Instant::now() >= deadline {
            return false;
        }
        if !sent {
            let pids = job_pids(job);
            let first = pids.contains(&(t.pid as u32));
            let target = if first || pids.is_empty() { t.pid as u32 } else { pids[0] };
            let own = if pids.is_empty() { vec![target] } else { pids };
            sent = if gui_program(target) { close_windows(&own) } else { send_ctrl_break(t.pid, target) };
        }
        std::thread::sleep(Duration::from_millis(if sent { 20 } else { 50 }));
    }
    true
}

/// Asks the whole job to stop, waits up to grace, then ends what is left.
pub fn stop(t: &Tracking, grace: Duration) -> Stopped {
    let Some(job) = open_job(t, JOB_OBJECT_QUERY | JOB_OBJECT_TERMINATE).filter(|j| active_processes(j) > 0) else {
        return Stopped::AlreadyGone;
    };
    if ask_to_stop(t, &job, grace) {
        return Stopped::Gracefully;
    }
    // SAFETY: a live job handle.
    unsafe { TerminateJobObject(raw(&job), 1) };
    wait_until(Duration::from_secs(2), || !alive(t));
    Stopped::Killed
}

/// Stops a process rant did not start, by its id alone. Windows offers no way to ask such a
/// process, so it is ended at once.
pub fn stop_pid(pid: u64, _grace: Duration) -> Result<Stopped> {
    // SAFETY: no pointers.
    if pid == 0 || pid == unsafe { GetCurrentProcessId() } as u64 {
        return Ok(Stopped::AlreadyGone);
    }
    let Some(process) = open_process(pid, PROCESS_TERMINATE | PROCESS_SYNCHRONIZE) else {
        // SAFETY: no pointers.
        if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
            return Err(error(format!("process {pid} belongs to another user")));
        }
        return Ok(Stopped::AlreadyGone);
    };
    // SAFETY: a live process handle.
    unsafe {
        if TerminateProcess(raw(&process), 1) == 0 {
            return Ok(Stopped::AlreadyGone);
        }
        WaitForSingleObject(raw(&process), 2000);
    }
    Ok(Stopped::KilledAtOnce)
}
