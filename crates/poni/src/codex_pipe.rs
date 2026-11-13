//! Codex 的 Windows 本机传输。仅当前用户可连接，所有等待由内核事件唤醒。

use std::cell::Cell;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::Security::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Pipes::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::IO::*;

// HANDLE由内核跨线程使用；关闭只发生在最后一个拥有者释放后。
struct Handle(HANDLE);
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn native(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error((error.code().0 & 0xffff) as i32)
}

fn user_sid(process: HANDLE) -> io::Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(native)?;
    let token = Handle(token);
    let mut size = 0;
    unsafe {
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut size);
    }
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    // TOKEN_USER 包含指针，使用 usize 保证存储对齐。
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(native)?;
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(native)?;
    let sid = unsafe { text.to_string() };
    unsafe {
        let _ = LocalFree(HLOCAL(text.0.cast()));
    }
    sid.map_err(|_| io::ErrorKind::InvalidData.into())
}

pub fn path() -> io::Result<PathBuf> {
    let sid = user_sid(unsafe { GetCurrentProcess() })?;
    let mut name = format!(r"\\.\pipe\Poni.Codex.{sid}");
    if let Some(home) = std::env::var_os("PONI_HOME") {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        home.hash(&mut hash);
        name.push_str(&format!(".{:016x}", hash.finish()));
    }
    Ok(PathBuf::from(name))
}

struct Pipe {
    handle: Handle,
    closed: AtomicBool,
    // 开始I/O与关闭串行，防止取消先于ReadFile发生而留下等待。
    starting: Mutex<()>,
}

pub struct Stream {
    pipe: Arc<Pipe>,
    read_wait: Cell<Option<Duration>>,
    write_wait: Cell<Option<Duration>>,
}

impl Stream {
    fn new(handle: HANDLE) -> Self {
        Self {
            pipe: Arc::new(Pipe {
                handle: Handle(handle),
                closed: AtomicBool::new(false),
                starting: Mutex::new(()),
            }),
            read_wait: Cell::new(None),
            write_wait: Cell::new(None),
        }
    }

    pub fn connect(path: &Path) -> io::Result<Self> {
        let name = HSTRING::from(path.as_os_str());
        let deadline = Instant::now() + Duration::from_secs(3);
        let handle = loop {
            match unsafe {
                CreateFileW(
                    &name,
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    None,
                )
            } {
                Ok(handle) => break handle,
                Err(error)
                    if error.code() == windows::core::HRESULT::from_win32(ERROR_PIPE_BUSY.0) =>
                {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                    // 等待空闲实例由内核唤醒，不使用周期性重连。
                    if !unsafe { WaitNamedPipeW(&name, remaining.as_millis().max(1) as u32) }
                        .as_bool()
                    {
                        return Err(io::Error::last_os_error());
                    }
                }
                Err(error) => return Err(native(error)),
            }
        };
        let stream = Self::new(handle);
        // 验证服务端所属用户，拒绝其他用户预先创建的同名管道。
        let mut pid = 0;
        unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }.map_err(native)?;
        let process = Handle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .map_err(native)?,
        );
        if user_sid(process.0)? != user_sid(unsafe { GetCurrentProcess() })? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(stream)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            pipe: self.pipe.clone(),
            read_wait: Cell::new(self.read_wait.get()),
            write_wait: Cell::new(self.write_wait.get()),
        })
    }
    pub fn set_read_timeout(&self, wait: Option<Duration>) -> io::Result<()> {
        self.read_wait.set(wait);
        Ok(())
    }
    pub fn set_write_timeout(&self, wait: Option<Duration>) -> io::Result<()> {
        self.write_wait.set(wait);
        Ok(())
    }
    pub fn shutdown(&self, _: Shutdown) -> io::Result<()> {
        let _starting = self.pipe.starting.lock().unwrap();
        self.pipe.closed.store(true, Ordering::Release);
        unsafe {
            let _ = CancelIoEx(self.pipe.handle.0, None);
        }
        Ok(())
    }

    fn transfer(
        &self,
        wait: Option<Duration>,
        start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>,
    ) -> io::Result<usize> {
        let event = Handle(unsafe { CreateEventW(None, true, false, None) }.map_err(native)?);
        let mut operation = OVERLAPPED {
            hEvent: event.0,
            ..Default::default()
        };
        let result = {
            let _starting = self.pipe.starting.lock().unwrap();
            if self.pipe.closed.load(Ordering::Acquire) {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            start(&mut operation)
        };
        complete(self.pipe.handle.0, &mut operation, result, wait)
    }
}

fn complete(
    handle: HANDLE,
    operation: &mut OVERLAPPED,
    result: windows::core::Result<()>,
    wait: Option<Duration>,
) -> io::Result<usize> {
    if let Err(error) = result {
        if error.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0) {
            return Err(native(error));
        }
        let timeout = wait.map_or(INFINITE, |wait| {
            wait.as_millis().min((INFINITE - 1) as u128) as u32
        });
        let status = unsafe { WaitForSingleObject(operation.hEvent, timeout) };
        if status != WAIT_OBJECT_0 {
            unsafe {
                let _ = CancelIoEx(handle, Some(operation));
                // OVERLAPPED及缓冲区只能在取消真正结束后释放。
                let mut count = 0;
                let _ = GetOverlappedResult(handle, operation, &mut count, true);
            }
            return Err(if status == WAIT_TIMEOUT {
                io::ErrorKind::TimedOut.into()
            } else {
                io::Error::last_os_error()
            });
        }
    }
    let mut count = 0;
    unsafe { GetOverlappedResult(handle, operation, &mut count, false) }.map_err(native)?;
    Ok(count as usize)
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.transfer(self.read_wait.get(), |operation| unsafe {
            ReadFile(self.pipe.handle.0, Some(buffer), None, Some(operation))
        }) {
            Err(error) if error.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32) => Ok(0),
            result => result,
        }
    }
}
impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.transfer(self.write_wait.get(), |operation| unsafe {
            WriteFile(self.pipe.handle.0, Some(buffer), None, Some(operation))
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Listener {
    path: PathBuf,
    pending: Handle,
}

impl Listener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            pending: create_pipe(path, true)?,
        })
    }

    pub fn incoming(mut self) -> impl Iterator<Item = io::Result<Stream>> {
        std::iter::from_fn(move || {
            let accept = (|| {
                let event =
                    Handle(unsafe { CreateEventW(None, true, false, None) }.map_err(native)?);
                let mut operation = OVERLAPPED {
                    hEvent: event.0,
                    ..Default::default()
                };
                let result = unsafe { ConnectNamedPipe(self.pending.0, Some(&mut operation)) };
                if !result.as_ref().is_err_and(|error| {
                    error.code() == windows::core::HRESULT::from_win32(ERROR_PIPE_CONNECTED.0)
                }) {
                    complete(self.pending.0, &mut operation, result, None)?;
                }
                let next = create_pipe(&self.path, false)?;
                let connected = std::mem::replace(&mut self.pending, next);
                let handle = connected.0;
                std::mem::forget(connected);
                Ok(Stream::new(handle))
            })();
            Some(accept)
        })
    }
}

fn create_pipe(path: &Path, first: bool) -> io::Result<Handle> {
    let sid = user_sid(unsafe { GetCurrentProcess() })?;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(format!("D:P(A;;GA;;;{sid})")),
            1,
            &mut descriptor,
            None,
        )
    }
    .map_err(native)?;
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let mut flags = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let handle = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(path.as_os_str()),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            16,
            8192,
            8192,
            0,
            Some(&security),
        )
    };
    let error = io::Error::last_os_error();
    unsafe {
        let _ = LocalFree(HLOCAL(descriptor.0));
    }
    if handle == INVALID_HANDLE_VALUE {
        return Err(error);
    }
    Ok(Handle(handle))
}
