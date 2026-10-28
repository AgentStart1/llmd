use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

pub fn supervisor_command(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", test]);
    command.stdout(Stdio::null()).stderr(Stdio::inherit());
    // Keep terminal Ctrl-C directed at the runner; EOF is the supervisor's shutdown signal.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    command
}

struct OwnedCli {
    child: Child,
}

impl Drop for OwnedCli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct DirectoryCleanup(PathBuf);

impl Drop for DirectoryCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn supervise_cli() {
    let directory = PathBuf::from(std::env::var_os("LLMD_TEST_DIRECTORY").unwrap());
    // Declared before OwnedCli so unwinding always reaps the process before removal.
    let _cleanup = DirectoryCleanup(directory.clone());
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("LLMD_TEST_ARGS").unwrap()).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_llmd"))
        .args(args)
        .env("LLMD_MODEL_DIR", directory.join("models"))
        .env("RUST_LOG", "llmd_server=info")
        .env("NO_COLOR", "1")
        // Never give the CLI a copy of the lease pipe.
        .stdin(Stdio::null())
        .stdout(File::create(directory.join("stdout")).unwrap())
        .stderr(File::create(directory.join("stderr")).unwrap())
        .spawn()
        .unwrap();
    let mut owned = OwnedCli { child };
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let _ = std::io::stdin().read_exact(&mut [0]);
        let _ = sender.send(());
    });
    let mut status_published = false;
    loop {
        if !status_published {
            if let Some(status) = owned.child.try_wait().unwrap() {
                let temporary = directory.join("status.tmp");
                fs::write(&temporary, status.code().unwrap_or(-1).to_string()).unwrap();
                fs::rename(temporary, directory.join("status")).unwrap();
                status_published = true;
            }
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => break,
        }
    }
    reader.join().unwrap();
    // OwnedCli is dropped before DirectoryCleanup, including after runner termination.
}
