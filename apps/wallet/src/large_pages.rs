//! Large pages: RandomX mines faster, often 10 to 30 percent, with its 2 GB
//! dataset in them. The miner asks for them every time it gets RandomX
//! ready and quietly does without when the system says no; this sets the
//! system up, once, so it says yes.
//!
//! Windows gives them only to accounts with the "Lock pages in memory"
//! right: granting it takes an administrator's approval, and it takes
//! effect at the next sign-in. Linux gives them from a pool set aside by
//! the administrator. Macs cannot use them for RandomX.

use serde::Serialize;

/// Where this computer stands.
#[derive(Clone, Copy, Default, Serialize)]
pub struct State {
    /// This system can mine in large pages.
    pub supported: bool,
    /// Windows, which asks for an administrator and a sign-out; Linux asks
    /// for the password.
    pub windows: bool,
    /// Set up: mining gets large pages from its next start.
    pub ready: bool,
    /// Windows: the right is granted, and works once the person signs out
    /// of Windows and back in.
    pub sign_out: bool,
}

/// Where this computer stands; `granted` is whether this app granted the
/// Windows right before.
pub fn state(granted: bool) -> State {
    #[cfg(windows)]
    {
        let ready = windows::has_right();
        State { supported: true, windows: true, ready, sign_out: granted && !ready }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = granted;
        State { supported: linux::page_kb() == Some(2048), windows: false, ready: linux::set_aside() >= linux::needed(), sign_out: false }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = granted;
        State::default()
    }
}

/// Sets the system up, asking for an administrator's approval. Blocks
/// until the person answers. Err says why it did not happen.
pub fn set_up() -> Result<(), String> {
    #[cfg(windows)]
    return windows::request();
    #[cfg(target_os = "linux")]
    return linux::request();
    #[cfg(not(any(windows, target_os = "linux")))]
    Err("This computer cannot mine in large pages.".into())
}

/// The argument the app is started with, as administrator, to grant the
/// right to the account named after it.
pub const GRANT_ARG: &str = "--grant-large-pages";

/// When the app was started to grant the right, does that and exits with
/// what happened, before anything else starts.
pub fn grant_if_asked() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some(GRANT_ARG) {
        return;
    }
    #[cfg(windows)]
    let code = args.next().map_or(87, |sid| windows::grant(&sid));
    #[cfg(not(windows))]
    let code = 1;
    std::process::exit(code);
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID, LocalFree};
    use windows_sys::Win32::Security::Authentication::Identity::{
        LSA_OBJECT_ATTRIBUTES, LSA_UNICODE_STRING, LsaAddAccountRights, LsaClose, LsaNtStatusToWinError, LsaOpenPolicy,
        POLICY_CREATE_ACCOUNT, POLICY_LOOKUP_NAMES,
    };
    use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
    use windows_sys::Win32::Security::{
        GetTokenInformation, LookupPrivilegeValueW, PSID, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenPrivileges, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    const RIGHT: &str = "SeLockMemoryPrivilege";

    fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
        text.as_ref().encode_wide().chain(Some(0)).collect()
    }

    /// This process's token information of `class`, in a buffer aligned
    /// for the structures it holds.
    fn token_info(class: i32) -> Option<Vec<u64>> {
        // SAFETY: the token is opened, read into a buffer as long as
        // Windows asks for, and closed.
        unsafe {
            let mut token: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut len = 0u32;
            GetTokenInformation(token, class, null_mut(), 0, &mut len);
            let mut buffer = vec![0u64; (len as usize).div_ceil(8).max(1)];
            let ok = GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), len, &mut len) != 0;
            CloseHandle(token);
            ok.then_some(buffer)
        }
    }

    /// Whether this sign-in has the right: RandomX turns it on itself.
    pub fn has_right() -> bool {
        let name = wide(RIGHT);
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        // SAFETY: plain calls with valid pointers; the token buffer holds
        // a TOKEN_PRIVILEGES followed by PrivilegeCount entries.
        unsafe {
            if LookupPrivilegeValueW(null(), name.as_ptr(), &mut luid) == 0 {
                return false;
            }
            let Some(buffer) = token_info(TokenPrivileges) else { return false };
            let privileges = buffer.as_ptr() as *const TOKEN_PRIVILEGES;
            let first = (*privileges).Privileges.as_ptr();
            (0..(*privileges).PrivilegeCount as usize).any(|i| {
                let held = std::ptr::read_unaligned(first.add(i)).Luid;
                held.LowPart == luid.LowPart && held.HighPart == luid.HighPart
            })
        }
    }

    /// The signed-in account, as a SID string.
    fn account() -> Option<String> {
        let buffer = token_info(TokenUser)?;
        // SAFETY: the buffer holds a TOKEN_USER; the string Windows makes
        // is read up to its end and freed.
        unsafe {
            let sid = (*(buffer.as_ptr() as *const TOKEN_USER)).User.Sid;
            let mut text: *mut u16 = null_mut();
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return None;
            }
            let len = (0..).take_while(|&i| *text.add(i) != 0).count();
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            Some(sid)
        }
    }

    /// Starts this app again as administrator to grant the right to this
    /// account, and waits for it.
    pub fn request() -> Result<(), String> {
        let sid = account().ok_or("Windows did not say which account this is.")?;
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let (verb, file, params) = (wide("runas"), wide(&exe), wide(format!("{} {sid}", super::GRANT_ARG)));
        // SAFETY: the strings outlive the call; the process handle it
        // returns is waited on and closed.
        unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = params.as_ptr();
            info.nShow = SW_HIDE;
            if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
                return Err("Windows did not get an administrator's approval.".into());
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            if code == 0 { Ok(()) } else { Err(format!("Windows could not grant the right (error {code}).")) }
        }
    }

    /// Grants the right to the account `sid`, as administrator: 0, or the
    /// Windows error.
    pub fn grant(sid: &str) -> i32 {
        let sid_text = wide(sid);
        let mut name: Vec<u16> = RIGHT.encode_utf16().collect();
        // SAFETY: every pointer is valid for the call it is given to; the
        // policy and the SID are freed after.
        unsafe {
            let mut account: PSID = null_mut();
            if ConvertStringSidToSidW(sid_text.as_ptr(), &mut account) == 0 {
                return 87;
            }
            let attributes: LSA_OBJECT_ATTRIBUTES = std::mem::zeroed();
            let mut policy = std::mem::zeroed();
            let status = LsaOpenPolicy(null(), &attributes, (POLICY_CREATE_ACCOUNT | POLICY_LOOKUP_NAMES) as u32, &mut policy);
            if status != 0 {
                LocalFree(account.cast());
                return LsaNtStatusToWinError(status) as i32;
            }
            let bytes = (name.len() * 2) as u16;
            let right = LSA_UNICODE_STRING { Length: bytes, MaximumLength: bytes, Buffer: name.as_mut_ptr() };
            let status = LsaAddAccountRights(policy, account, &right, 1);
            LsaClose(policy);
            LocalFree(account.cast());
            if status == 0 { 0 } else { LsaNtStatusToWinError(status) as i32 }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    /// The large page size, in kB.
    pub fn page_kb() -> Option<u64> {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        info.lines().find_map(|l| l.strip_prefix("Hugepagesize:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
    }

    /// Large pages set aside now.
    pub fn set_aside() -> u64 {
        std::fs::read_to_string("/proc/sys/vm/nr_hugepages").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }

    /// Large pages of 2 MB that mining needs: 1040 for the dataset, 128
    /// for the cache, and one scratchpad for each thread.
    pub fn needed() -> u64 {
        1168 + orien_net::miner::cores() as u64 + 8
    }

    /// Sets the pages aside now and at every start, with the person's
    /// password.
    pub fn request() -> Result<(), String> {
        let n = needed().max(set_aside());
        let script = format!("sysctl -w vm.nr_hugepages={n} && echo vm.nr_hugepages={n} > /etc/sysctl.d/60-orien-mining.conf");
        let status = Command::new("pkexec")
            .args(["sh", "-c", &script])
            .status()
            .map_err(|_| "This Linux has no pkexec to ask for the password.".to_string())?;
        if !status.success() {
            return Err("Linux did not get the password.".into());
        }
        if set_aside() < needed() {
            return Err("Linux could set aside only part of the memory now. Restart the computer to finish.".into());
        }
        Ok(())
    }
}
