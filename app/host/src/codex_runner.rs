use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "macos")]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream as ControlStream;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
type ControlStream = std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::paths::open_owned_directory_chain;
#[cfg(windows)]
use crate::windows_job::JobObject;
use crate::store::{Job, JobFailureKind};

const OUTPUT_OK: u8 = 0;
const OUTPUT_INVALID: u8 = 1;
const OUTPUT_TOO_LARGE: u8 = 2;
pub const SUPERVISOR_CLI_MISSING_EXIT: i32 = 78;
pub const SUPERVISOR_IO_EXIT: i32 = 79;
#[cfg(windows)]
const WINDOWS_CODEX_MODEL: &str = "gpt-6-sol";

#[derive(Debug, Clone)]
pub struct CodexRunnerConfig {
    pub executable: PathBuf,
    pub supervisor_executable: Option<PathBuf>,
    pub timeout: Duration,
    pub max_stdout_line_bytes: usize,
    pub max_stdout_total_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl Default for CodexRunnerConfig {
    fn default() -> Self {
        Self {
            executable: discover_codex_executable(),
            supervisor_executable: std::env::current_exe().ok(),
            timeout: Duration::from_secs(15 * 60),
            max_stdout_line_bytes: 256 * 1024,
            max_stdout_total_bytes: 4 * 1024 * 1024,
            max_stderr_bytes: 64 * 1024,
        }
    }
}

#[cfg(windows)]
pub(crate) fn discover_codex_executable() -> PathBuf {
    if let Some(configured) = std::env::var_os("EASY_CODEX_CLI") {
        let configured = PathBuf::from(configured);
        if configured.is_absolute() {
            return configured;
        }
    }
    // The desktop launcher redirects LOCALAPPDATA to its private data root
    // when starting Host. USERPROFILE still locates the user's Codex install.
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let app_bin = PathBuf::from(profile).join("AppData/Local/OpenAI/Codex/bin");
        if let Some(binary) = find_codex_in_app_bin(&app_bin) {
            return binary;
        }
    }
    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        let app_bin = PathBuf::from(local_appdata).join("OpenAI/Codex/bin");
        if let Some(binary) = find_codex_in_app_bin(&app_bin) {
            return binary;
        }
    }
    // Desktop releases can ship a newer CLI than an older npm install. The
    // app-owned task store may reject the model selected by that older CLI.
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let binary = directory.join("codex.exe");
            if binary.is_file() {
                return binary;
            }
        }
    }
    let (package, vendor) = if cfg!(target_arch = "aarch64") {
        ("codex-win32-arm64", "aarch64-pc-windows-msvc")
    } else {
        ("codex-win32-x64", "x86_64-pc-windows-msvc")
    };
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let binary = PathBuf::from(appdata)
            .join("npm/node_modules/@openai/codex/node_modules/@openai")
            .join(package)
            .join("vendor")
            .join(vendor)
            .join("bin/codex.exe");
        if binary.is_file() {
            return binary;
        }
    }
    PathBuf::from("codex.exe")
}

#[cfg(windows)]
fn find_codex_in_app_bin(app_bin: &Path) -> Option<PathBuf> {
    let mut candidates = std::fs::read_dir(app_bin)
        .ok()?
        .take(64)
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                path.join("codex.exe")
            } else {
                path
            }
        })
        .filter(|path| {
            path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("codex.exe"))
                && path.is_file()
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|path| path.metadata().and_then(|metadata| metadata.modified()).ok());
    candidates.pop()
}

#[cfg(not(windows))]
pub(crate) fn discover_codex_executable() -> PathBuf {
    if let Some(configured) = std::env::var_os("EASY_CODEX_CLI") {
        let configured = PathBuf::from(configured);
        if configured.is_absolute() {
            return configured;
        }
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return PathBuf::from("codex");
    };
    let node_versions = home.join(".nvm/versions/node");
    let package = if cfg!(target_arch = "aarch64") {
        "codex-darwin-arm64"
    } else {
        "codex-darwin-x64"
    };
    let vendor = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    let Ok(entries) = std::fs::read_dir(node_versions) else {
        return PathBuf::from("codex");
    };
    let mut candidates = entries
        .take(64)
        .filter_map(Result::ok)
        .map(|entry| {
            entry
                .path()
                .join("lib/node_modules/@openai/codex/node_modules/@openai")
                .join(package)
                .join("vendor")
                .join(vendor)
                .join("bin/codex")
        })
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop().unwrap_or_else(|| PathBuf::from("codex"))
}

#[derive(Debug, Clone)]
pub struct CodexRunner {
    config: CodexRunnerConfig,
    #[cfg(windows)]
    desktop_journal: Option<PathBuf>,
}

impl CodexRunner {
    pub fn new(config: CodexRunnerConfig) -> Self {
        Self { config, #[cfg(windows)] desktop_journal: None }
    }

    #[cfg(windows)]
    pub fn with_desktop(mut self, journal: PathBuf) -> Self {
        self.desktop_journal = Some(journal);
        self
    }

    pub fn run(&self, job: &Job) -> Result<(), JobFailureKind> {
        self.run_with_cancel(job, &AtomicBool::new(false))
    }

    pub fn run_with_cancel(&self, job: &Job, cancel: &AtomicBool) -> Result<(), JobFailureKind> {
        #[cfg(windows)]
        if let Some(journal) = &self.desktop_journal {
            return crate::desktop_runner::run(job, cancel, self.config.timeout, journal);
        }
        #[cfg(unix)]
        let cwd = open_owned_directory_chain(&job.cwd, false)
            .map_err(|_| JobFailureKind::UnsafeWorkingDirectory)?;
        #[cfg(unix)]
        let cwd_fd = cwd.as_raw_fd();
        #[cfg(windows)]
        crate::windows_paths::validate_directory(&job.cwd)
            .map_err(|_| JobFailureKind::UnsafeWorkingDirectory)?;
        let supervise = cfg!(target_os = "macos") && self.config.supervisor_executable.is_some();
        #[cfg(unix)]
        let (mut parent_control, child_control) = if supervise {
            let (parent, child) = ControlStream::pair().map_err(|_| JobFailureKind::ProcessIo)?;
            (Some(parent), Some(child))
        } else {
            (None, None)
        };
        #[cfg(windows)]
        let (mut parent_control, child_control): (Option<ControlStream>, Option<ControlStream>) =
            (None, None);
        #[cfg(unix)]
        let control_fd = child_control.as_ref().map(AsRawFd::as_raw_fd);
        #[cfg(windows)]
        let control_fd: Option<i32> = None;
        let mut command = if supervise {
            let mut command = Command::new(self.config.supervisor_executable.as_ref().unwrap());
            command.args([
                "runner-supervisor",
                &std::process::id().to_string(),
                self.config.executable.to_string_lossy().as_ref(),
                job.task_id.as_str(),
                &control_fd.unwrap().to_string(),
            ]);
            command
        } else {
            let mut command = Command::new(&self.config.executable);
            command.arg("exec");
            #[cfg(windows)]
            {
                // Keep Host's CLI model stable when the desktop user's default
                // changes to a model unavailable to ChatGPT-authenticated CLI.
                command.args(["--model", WINDOWS_CODEX_MODEL]);
                // CLI resume does not retain the sandbox selected when the task
                // was created. Permit edits only in the task's validated cwd.
                command.args(["--sandbox", "workspace-write"]);
            }
            command.args(["resume", "--json"]);
            #[cfg(windows)]
            if !job.cwd.ancestors().any(|directory| directory.join(".git").exists()) {
                // Projectless Codex tasks have a real, allowlisted working
                // directory but no Git checkout. The CLI requires this flag.
                command.arg("--skip-git-repo-check");
            }
            command.args([job.task_id.as_str(), "-"]);
            command
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.current_dir(&job.cwd);
        #[cfg(unix)]
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(cwd_fd) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(control_fd) = control_fd
                    && libc::fcntl(control_fd, libc::F_SETFD, 0) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() == 1 {
                        return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                JobFailureKind::CliMissing
            } else {
                JobFailureKind::ProcessIo
            }
        })?;
        drop(child_control);
        if let Some(control) = parent_control.as_mut() {
            control
                .set_nonblocking(true)
                .map_err(|_| JobFailureKind::ProcessIo)?;
        }
        self.drive_child(child, parent_control, job.prompt.as_bytes(), cancel)
    }

    fn drive_child(
        &self,
        mut child: Child,
        mut control: Option<ControlStream>,
        prompt: &[u8],
        cancel: &AtomicBool,
    ) -> Result<(), JobFailureKind> {
        let mut stdin = child.stdin.take().ok_or(JobFailureKind::ProcessIo)?;
        let stdout = child.stdout.take().ok_or(JobFailureKind::ProcessIo)?;
        let stderr = child.stderr.take().ok_or(JobFailureKind::ProcessIo)?;
        let mut guard = ChildGuard::new(child)?;

        let stdin_failed = stdin.write_all(prompt).and_then(|_| stdin.flush()).is_err();
        drop(stdin);

        let output_state = Arc::new(AtomicU8::new(OUTPUT_OK));
        let stdout_state = Arc::clone(&output_state);
        let max_line = self.config.max_stdout_line_bytes;
        let max_total = self.config.max_stdout_total_bytes;
        let stdout_reader =
            thread::spawn(move || read_json_lines(stdout, max_line, max_total, stdout_state));
        let stderr_limit = self.config.max_stderr_bytes;
        let stderr_reader = thread::spawn(move || read_stderr(stderr, stderr_limit));

        let deadline = Instant::now() + self.config.timeout;
        let supervised = control.is_some();
        let (status, supervisor_code) = loop {
            if let Some(control) = control.as_mut() {
                let mut code = [0_u8; 1];
                match control.read(&mut code) {
                    Ok(1) => {
                        guard.kill_and_wait();
                        break (None, Some(code[0]));
                    }
                    Ok(0) => {}
                    Ok(_) => unreachable!(),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        guard.kill_and_wait();
                        let _ = stdout_reader.join();
                        let _ = stderr_reader.join();
                        return Err(JobFailureKind::ProcessIo);
                    }
                }
            }
            match guard.child_mut().try_wait() {
                Ok(Some(status)) => break (Some(status), None),
                Ok(None) => {}
                Err(_) => {
                    guard.kill_and_wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(JobFailureKind::ProcessIo);
                }
            }
            if output_state.load(Ordering::Acquire) == OUTPUT_TOO_LARGE {
                guard.kill_and_wait();
                let output = stdout_reader.join().unwrap_or(OUTPUT_INVALID);
                let _ = stderr_reader.join();
                return Err(output_failure(output));
            }
            if Instant::now() >= deadline {
                guard.kill_and_wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(JobFailureKind::Timeout);
            }
            if cancel.load(Ordering::Acquire) {
                guard.kill_and_wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(JobFailureKind::ProcessIo);
            }
            thread::sleep(Duration::from_millis(10));
        };
        if status.is_some() {
            guard.disarm();
        }

        let output = stdout_reader.join().unwrap_or(OUTPUT_INVALID);
        let stderr = stderr_reader.join().unwrap_or_default();
        if supervisor_code == Some(SUPERVISOR_CLI_MISSING_EXIT as u8)
            || (supervised
                && status.as_ref().and_then(|status| status.code())
                    == Some(SUPERVISOR_CLI_MISSING_EXIT))
        {
            return Err(JobFailureKind::CliMissing);
        }
        if supervisor_code == Some(SUPERVISOR_IO_EXIT as u8)
            || (supervised
                && status.as_ref().and_then(|status| status.code()) == Some(SUPERVISOR_IO_EXIT))
        {
            return Err(JobFailureKind::ProcessIo);
        }
        if output == OUTPUT_TOO_LARGE {
            return Err(JobFailureKind::OutputTooLarge);
        }
        let succeeded = supervisor_code == Some(0)
            || status
                .as_ref()
                .is_some_and(std::process::ExitStatus::success);
        if !succeeded {
            return Err(classify_stderr(&stderr));
        }
        if stdin_failed {
            return Err(JobFailureKind::ProcessIo);
        }
        if output != OUTPUT_OK {
            return Err(output_failure(output));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub fn run_supervisor(
    parent: i32,
    executable: &Path,
    task_id: &str,
    control_fd: i32,
) -> Result<i32, std::io::Error> {
    if parent <= 1
        || control_fd < 0
        || uuid::Uuid::parse_str(task_id).is_err()
        || !executable.is_absolute()
    {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let _queue = unsafe { std::fs::File::from_raw_fd(queue) };
    let mut control = unsafe { UnixStream::from_raw_fd(control_fd) };
    if unsafe { libc::fcntl(control_fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(queue, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let change = libc::kevent {
        ident: parent as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let registered =
        unsafe { libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) };
    if registered != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut child = match Command::new(executable)
        .args(["exec", "resume", "--json", task_id, "-"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return supervisor_hold(queue, &mut control, SUPERVISOR_CLI_MISSING_EXIT as u8);
        }
        Err(_) => return supervisor_hold(queue, &mut control, SUPERVISOR_IO_EXIT as u8),
    };
    loop {
        let mut event = unsafe { std::mem::zeroed::<libc::kevent>() };
        let timeout = libc::timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        };
        let observed = unsafe { libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, &timeout) };
        if observed < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if observed > 0 {
            unsafe {
                libc::kill(-libc::getpgrp(), libc::SIGKILL);
            }
            return Ok(1);
        }
        if let Some(status) = child.try_wait()? {
            return supervisor_hold(queue, &mut control, u8::from(!status.success()));
        }
    }
}

#[cfg(target_os = "macos")]
fn supervisor_hold(queue: i32, control: &mut UnixStream, code: u8) -> Result<i32, std::io::Error> {
    if control.write_all(&[code]).is_err() {
        unsafe {
            libc::kill(-libc::getpgrp(), libc::SIGKILL);
        }
        return Ok(1);
    }
    loop {
        let mut event = unsafe { std::mem::zeroed::<libc::kevent>() };
        let observed =
            unsafe { libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
        if observed > 0 {
            unsafe {
                libc::kill(-libc::getpgrp(), libc::SIGKILL);
            }
            return Ok(1);
        }
        if observed < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn run_supervisor(
    _parent: i32,
    _executable: &Path,
    _task_id: &str,
    _control_fd: i32,
) -> Result<i32, std::io::Error> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn read_json_lines(
    mut reader: impl Read,
    max_line: usize,
    max_total: usize,
    state: Arc<AtomicU8>,
) -> u8 {
    let mut buffer = [0_u8; 8192];
    let mut line = Vec::new();
    let mut total = 0_usize;
    let mut records = 0_usize;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return publish_output_state(&state, OUTPUT_INVALID),
        };
        for byte in &buffer[..read] {
            total = match total.checked_add(1) {
                Some(total) if total <= max_total => total,
                _ => return publish_output_state(&state, OUTPUT_TOO_LARGE),
            };
            if *byte == b'\n' {
                if !line.is_empty() {
                    if !is_json_object(&line) {
                        return publish_output_state(&state, OUTPUT_INVALID);
                    }
                    records += 1;
                    line.clear();
                }
            } else {
                if line.len() >= max_line {
                    return publish_output_state(&state, OUTPUT_TOO_LARGE);
                }
                line.push(*byte);
            }
        }
    }
    if !line.is_empty() {
        if !is_json_object(&line) {
            return publish_output_state(&state, OUTPUT_INVALID);
        }
        records += 1;
    }
    if records == 0 {
        publish_output_state(&state, OUTPUT_INVALID)
    } else {
        OUTPUT_OK
    }
}

fn is_json_object(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes).is_ok_and(|value| value.is_object())
}

fn publish_output_state(state: &AtomicU8, value: u8) -> u8 {
    state.store(value, Ordering::Release);
    value
}

fn read_stderr(mut reader: impl Read, limit: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(limit.min(4096));
    let mut buffer = [0_u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let remaining = limit.saturating_sub(output.len());
                output.extend_from_slice(&buffer[..read.min(remaining)]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    output
}

fn classify_stderr(stderr: &[u8]) -> JobFailureKind {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if text.contains("not logged in")
        || text.contains("authentication")
        || text.contains("unauthorized")
        || text.contains("401")
    {
        JobFailureKind::Authentication
    } else if text.contains("archived") {
        JobFailureKind::TaskArchived
    } else if text.contains("active session")
        || text.contains("already active")
        || text.contains("currently running")
    {
        JobFailureKind::ActiveSession
    } else {
        JobFailureKind::ExitFailure
    }
}

fn output_failure(output: u8) -> JobFailureKind {
    if output == OUTPUT_TOO_LARGE {
        JobFailureKind::OutputTooLarge
    } else {
        JobFailureKind::InvalidOutput
    }
}

struct ChildGuard {
    child: Option<Child>,
    #[cfg(unix)]
    process_group: i32,
    #[cfg(windows)]
    job: Option<JobObject>,
}

impl ChildGuard {
    fn new(mut child: Child) -> Result<Self, JobFailureKind> {
        #[cfg(unix)]
        {
            let process_group = child.id() as i32;
            Ok(Self {
                child: Some(child),
                process_group,
            })
        }
        #[cfg(windows)]
        {
            let job = match JobObject::attach(&child) {
                Ok(job) => job,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(JobFailureKind::ProcessIo);
                }
            };
            Ok(Self {
                child: Some(child),
                job: Some(job),
            })
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child is armed")
    }

    fn kill_and_wait(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-self.process_group, libc::SIGKILL);
            }
            #[cfg(windows)]
            if let Some(job) = self.job.as_ref() {
                job.terminate();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        #[cfg(windows)]
        self.job.take();
    }

    fn disarm(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
        self.child.take();
        #[cfg(windows)]
        self.job.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.kill_and_wait();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use tempfile::tempdir;

    fn fixture_job(cwd: &Path, prompt: &str) -> Job {
        Job {
            request_id: "request".into(),
            task_id: "00000000-0000-4000-8000-000000000001".into(),
            slot: 1,
            generation: 1,
            prompt: prompt.into(),
            cwd: cwd.to_path_buf(),
            recovery_count: 0,
            claim_generation: 1,
        }
    }

    fn script(directory: &Path, body: &str) -> PathBuf {
        let path = directory.join("fake-codex");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn config(executable: PathBuf) -> CodexRunnerConfig {
        CodexRunnerConfig {
            executable,
            supervisor_executable: None,
            timeout: Duration::from_secs(5),
            max_stdout_line_bytes: 1024,
            max_stdout_total_bytes: 4096,
            max_stderr_bytes: 1024,
        }
    }

    #[test]
    fn exact_argv_and_prompt_stdin_are_preserved_without_shell_interpolation() {
        let temp = tempdir().unwrap();
        let args = temp.path().join("args");
        let stdin = temp.path().join("stdin");
        let executable = script(
            temp.path(),
            &format!(
                "printf '%s\\n' \"$@\" > '{}'; cat > '{}'; printf '{{\"type\":\"done\"}}\\n'",
                args.display(),
                stdin.display()
            ),
        );
        let prompt = "line one\n$(touch should-not-exist) ; ' quoted";
        let runner = CodexRunner::new(config(executable));

        assert_eq!(runner.run(&fixture_job(temp.path(), prompt)), Ok(()));
        assert_eq!(
            fs::read_to_string(args).unwrap(),
            "exec\nresume\n--json\n00000000-0000-4000-8000-000000000001\n-\n"
        );
        assert_eq!(fs::read_to_string(stdin).unwrap(), prompt);
        assert!(!temp.path().join("should-not-exist").exists());
    }

    #[test]
    fn missing_cli_timeout_invalid_and_oversized_output_fail_closed() {
        let temp = tempdir().unwrap();
        let job = fixture_job(temp.path(), "prompt");
        let missing = CodexRunner::new(config(temp.path().join("missing")));
        assert_eq!(missing.run(&job), Err(JobFailureKind::CliMissing));

        let sleepy = script(temp.path(), "sleep 5");
        let mut sleepy_config = config(sleepy);
        sleepy_config.timeout = Duration::from_millis(50);
        assert_eq!(
            CodexRunner::new(sleepy_config).run(&job),
            Err(JobFailureKind::Timeout)
        );

        let invalid = script(temp.path(), "cat >/dev/null; printf 'not-json\\n'");
        assert_eq!(
            CodexRunner::new(config(invalid)).run(&job),
            Err(JobFailureKind::InvalidOutput)
        );

        let early_close = script(temp.path(), "printf 'not-json\\n'");
        let mut early_close_job = job.clone();
        early_close_job.prompt = "x".repeat(128 * 1024);
        assert_eq!(
            CodexRunner::new(config(early_close)).run(&early_close_job),
            Err(JobFailureKind::ProcessIo)
        );

        let oversized = script(
            temp.path(),
            "i=0; while [ $i -lt 2000 ]; do printf x; i=$((i+1)); done",
        );
        assert_eq!(
            CodexRunner::new(config(oversized)).run(&job),
            Err(JobFailureKind::OutputTooLarge)
        );
    }

    #[test]
    fn stderr_is_reduced_to_stable_categories() {
        let temp = tempdir().unwrap();
        let job = fixture_job(temp.path(), "prompt");
        for (message, expected) in [
            (
                "not logged in: secret detail",
                JobFailureKind::Authentication,
            ),
            (
                "task is archived: private title",
                JobFailureKind::TaskArchived,
            ),
            (
                "active session already active",
                JobFailureKind::ActiveSession,
            ),
            ("unknown private failure", JobFailureKind::ExitFailure),
        ] {
            let executable = script(
                temp.path(),
                &format!("printf '%s' '{}' >&2; exit 1", message),
            );
            assert_eq!(
                CodexRunner::new(config(executable)).run(&job),
                Err(expected)
            );
        }

        let delayed = script(
            temp.path(),
            "exec 1>&-; printf 'not logged in: private detail' >&2; sleep 0.2; exit 1",
        );
        assert_eq!(
            CodexRunner::new(config(delayed)).run(&job),
            Err(JobFailureKind::Authentication)
        );
    }

    #[test]
    fn child_sigkill_is_a_sanitized_exit_failure() {
        let temp = tempdir().unwrap();
        let executable = script(temp.path(), "printf '{}\\n'; kill -9 $$");
        assert_eq!(
            CodexRunner::new(config(executable)).run(&fixture_job(temp.path(), "prompt")),
            Err(JobFailureKind::ExitFailure)
        );
    }

    #[test]
    fn direct_cli_exit_codes_cannot_spoof_supervisor_categories() {
        let temp = tempdir().unwrap();
        for code in [SUPERVISOR_CLI_MISSING_EXIT, SUPERVISOR_IO_EXIT] {
            let executable = script(temp.path(), &format!("printf '{{}}\\n'; exit {code}"));
            assert_eq!(
                CodexRunner::new(config(executable)).run(&fixture_job(temp.path(), "prompt")),
                Err(JobFailureKind::ExitFailure)
            );
        }
    }

    #[test]
    fn unsafe_cwd_is_rejected_before_spawn() {
        let temp = tempdir().unwrap();
        let executable = script(temp.path(), "printf '{}\\n'");
        let mut job = fixture_job(temp.path(), "prompt");
        job.cwd = temp.path().join("missing");
        assert_eq!(
            CodexRunner::new(config(executable)).run(&job),
            Err(JobFailureKind::UnsafeWorkingDirectory)
        );
    }
}
