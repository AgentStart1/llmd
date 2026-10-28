use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize, Deserialize)]
enum Event {
    Started { directory: PathBuf },
    Exited { code: i32 },
    Finished { error: Option<String> },
}

fn publish(stream: &mut TcpStream, event: Event) -> Result<()> {
    serde_json::to_writer(&mut *stream, &event)?;
    stream.write_all(b"\n")?;
    Ok(())
}

pub fn supervisor_command(test: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", test]);
    command.stdout(Stdio::null()).stderr(Stdio::inherit());
    // The runner owns terminal cancellation; the supervisor observes connection EOF.
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

pub struct OwnedProcess(pub Child);

impl OwnedProcess {
    pub fn wait(&mut self) -> Result<ExitStatus> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.0.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!("process did not exit within {TIMEOUT:?}");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn stop(&mut self) -> Result<()> {
        if self.0.try_wait()?.is_none() {
            self.0.kill().context("terminate owned process")?;
        }
        self.wait().context("reap owned process")?;
        Ok(())
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("process cleanup failed: {error:#}");
        }
    }
}

struct Supervisor {
    process: OwnedProcess,
    connection: TcpStream,
    events: Receiver<Result<Event>>,
    reader: Option<JoinHandle<()>>,
}

impl Supervisor {
    fn connect(args: &[&str]) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let mut process = OwnedProcess(
            supervisor_command("supervise_cli")
                .env("LLMD_TEST_CONTROL", listener.local_addr()?.to_string())
                .env("LLMD_TEST_ARGS", serde_json::to_string(args)?)
                .stdin(Stdio::null())
                .spawn()?,
        );
        let deadline = Instant::now() + TIMEOUT;
        let connection = loop {
            match listener.accept() {
                Ok((connection, _)) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
            if process.0.try_wait()?.is_some() || Instant::now() >= deadline {
                bail!("supervisor failed to connect");
            }
            thread::sleep(Duration::from_millis(20));
        };
        // Windows accepted sockets inherit the listener's nonblocking mode.
        connection.set_nonblocking(false)?;
        connection.set_write_timeout(Some(TIMEOUT))?;
        let mut input = BufReader::new(connection.try_clone()?);
        let (sender, events) = mpsc::channel();
        let reader = thread::spawn(move || loop {
            let mut line = String::new();
            let event = match input.read_line(&mut line) {
                Ok(0) => Err(anyhow::anyhow!("supervisor disconnected before completion")),
                Ok(_) => serde_json::from_str::<Event>(&line).map_err(Into::into),
                Err(error) => Err(error.into()),
            };
            let terminal = matches!(event, Ok(Event::Finished { .. }) | Err(_));
            if sender.send(event).is_err() || terminal {
                break;
            }
        });
        let mut supervisor = Self {
            process,
            connection,
            events,
            reader: Some(reader),
        };
        // No CLI resources may be created until this side owns the complete channel.
        supervisor
            .connection
            .write_all(&[1])
            .context("start supervised CLI")?;
        Ok(supervisor)
    }

    fn receive(&self) -> Result<Event> {
        self.events
            .recv_timeout(TIMEOUT)
            .context("wait for supervisor event")?
            .context("receive supervisor event")
    }

    fn finish(&mut self) -> Result<()> {
        // Request shutdown while retaining the read half for the cleanup result.
        self.connection
            .shutdown(Shutdown::Write)
            .context("request supervisor cleanup")?;
        loop {
            match self.receive()? {
                Event::Exited { .. } => {}
                Event::Finished { error: None } => break,
                Event::Finished { error: Some(error) } => {
                    bail!("supervisor cleanup failed: {error}")
                }
                event => bail!("unexpected shutdown event: {event:?}"),
            }
        }
        let status = self.process.wait().context("wait for supervisor exit")?;
        if !status.success() {
            bail!("supervisor exited with {status}");
        }
        Ok(())
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        // Panic fallback only. Successful test scopes must have passed finish().
        let _ = self.connection.shutdown(Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        if let Err(error) = self.process.wait() {
            eprintln!("supervisor fallback cleanup failed: {error:#}");
        }
    }
}

pub struct Session {
    supervisor: Supervisor,
    directory: PathBuf,
}

impl Session {
    fn start(args: &[&str]) -> Result<Self> {
        let supervisor = Supervisor::connect(args)?;
        match supervisor.receive()? {
            Event::Started { directory } => Ok(Self {
                supervisor,
                directory,
            }),
            event => bail!("expected startup event, got {event:?}"),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn supervisor(&mut self) -> &mut OwnedProcess {
        &mut self.supervisor.process
    }
    pub fn stdout(&self) -> String {
        fs::read_to_string(self.directory.join("stdout")).unwrap()
    }
    pub fn stderr(&self) -> String {
        fs::read_to_string(self.directory.join("stderr")).unwrap()
    }

    pub fn wait(&self) -> Result<i32> {
        match self.supervisor.receive()? {
            Event::Exited { code } => Ok(code),
            event => bail!("expected CLI exit event, got {event:?}"),
        }
    }

    fn address(&self) -> Result<String> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Ok(event) = self.supervisor.events.try_recv() {
                bail!(
                    "server exited before readiness: {:?}; {} {}",
                    event?,
                    self.stdout(),
                    self.stderr()
                );
            }
            if let Some(address) = self
                .stdout()
                .split("llmd listening on ")
                .nth(1)
                .and_then(|tail| tail.split_whitespace().next())
            {
                return Ok(address.to_owned());
            }
            if Instant::now() >= deadline {
                bail!(
                    "server readiness timed out: {} {}",
                    self.stdout(),
                    self.stderr()
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn with_cli<R>(args: &[&str], operation: impl FnOnce(&mut Session) -> R) -> Result<R> {
    let mut session = Session::start(args)?;
    let result = operation(&mut session);
    session.supervisor.finish()?;
    Ok(result)
}

pub fn with_server<R>(operation: impl FnOnce(&mut Session, String) -> R) -> Result<R> {
    with_cli(
        &[
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--pool-size",
            "1",
        ],
        |session| {
            let address = session.address().unwrap();
            operation(session, address)
        },
    )
}

pub fn supervise_cli() -> Result<()> {
    let address = std::env::var("LLMD_TEST_CONTROL")?;
    let mut connection = TcpStream::connect_timeout(&address.parse()?, TIMEOUT)?;
    connection.set_write_timeout(Some(TIMEOUT))?;
    connection.set_read_timeout(Some(TIMEOUT))?;
    let mut start = [0];
    connection
        .read_exact(&mut start)
        .context("wait for runner startup request")?;
    if start != [1] {
        bail!("invalid runner startup request");
    }
    let result = run_cli(&mut connection);
    publish(
        &mut connection,
        Event::Finished {
            error: result.as_ref().err().map(|error| format!("{error:#}")),
        },
    )?;
    result
}

fn run_cli(connection: &mut TcpStream) -> Result<()> {
    let directory = tempfile::tempdir()?;
    fs::create_dir(directory.path().join("models"))?;
    let args: Vec<String> = serde_json::from_str(&std::env::var("LLMD_TEST_ARGS")?)?;
    let mut cli = OwnedProcess(
        Command::new(env!("CARGO_BIN_EXE_llmd"))
            .args(args)
            .env("LLMD_MODEL_DIR", directory.path().join("models"))
            .env("RUST_LOG", "llmd_server=info")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(File::create(directory.path().join("stdout"))?)
            .stderr(File::create(directory.path().join("stderr"))?)
            .spawn()?,
    );
    publish(
        connection,
        Event::Started {
            directory: directory.path().to_owned(),
        },
    )?;
    connection.set_read_timeout(Some(Duration::from_millis(20)))?;
    let mut published_exit = false;
    loop {
        if !published_exit {
            if let Some(status) = cli.0.try_wait()? {
                publish(
                    connection,
                    Event::Exited {
                        code: status.code().unwrap_or(-1),
                    },
                )?;
                published_exit = true;
            }
        }
        match connection.peek(&mut [0]) {
            Ok(_) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    cli.stop()?;
    drop(cli);
    directory.close().context("remove fixture directory")?;
    Ok(())
}
