//! Driving a Jupyter kernel over ZeroMQ.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use jupyter_protocol::connection_info::Transport;
use jupyter_protocol::{
    ConnectionInfo, ExecuteReply, ExecuteRequest, ExecutionState, InterruptRequest,
    JupyterMessage, JupyterMessageContent, ReplyStatus,
};
use jupyter_zmq_client::{
    ClientControlConnection, ClientIoPubConnection, ClientShellConnection, KernelspecDir,
};
use tokio::process::Child;
use uuid::Uuid;

use super::environment::{Environment, FOREIGN};
use super::outputs::Outputs;
use crate::store::Output;

/// How long to wait for the kernel to greet us on iopub before assuming it
/// speaks the older protocol, which has no greeting.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(30);

/// Run on every kernel before the first cell. matplotlib's inline backend
/// sends figures as PNG only, unless asked for SVG, which stays sharp at any
/// size on a slide.
pub const SETUP: &str = "%config InlineBackend.figure_formats = ['svg']\n";

/// A running Jupyter kernel. Cells sent to it share one interpreter, so state
/// carries over from one cell to the next, exactly as in a notebook.
pub struct Kernel {
    /// The name of the kernelspec it was started from.
    pub name: String,
    process: Child,
    shell: ClientShellConnection,
    iopub: ClientIoPubConnection,
    connection_info: ConnectionInfo,
    session_id: String,
    /// Whether it is interrupted by a message, rather than by a signal.
    interrupted_by_message: bool,
    /// Held for as long as the kernel, which reads it as it starts.
    _connection_file: ConnectionFile,
}

/// The file a kernel is told where to listen by, removed once it is no longer
/// needed, however the kernel ends: shut down, failing to start, or dropped
/// as a notebook fails.
struct ConnectionFile(PathBuf);

impl Drop for ConnectionFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

/// A cell that ran, as a notebook shows it.
pub struct Ran {
    pub outputs: Vec<Output>,
    /// Whether it ran without raising.
    pub ok: bool,
    /// The count the kernel gave it, which a notebook shows beside it.
    pub count: usize,
}

/// Interrupts the cell a kernel runs, the way its kernelspec asks to be.
pub struct Interrupter {
    control: ClientControlConnection,
    pid: Option<u32>,
    by_message: bool,
}

impl Interrupter {
    pub async fn interrupt(&mut self) -> Result<()> {
        if self.by_message {
            self.control
                .send(InterruptRequest::default().into())
                .await?;
            self.control.read().await?;
        } else if let Some(pid) = self.pid {
            tokio::process::Command::new("kill")
                .args(["-INT", &pid.to_string()])
                .status()
                .await?;
        }
        Ok(())
    }
}

/// Where to look for kernelspecs: in the environment the cells run in, if
/// they have one, and nowhere else, so that a kernel of another environment
/// never runs them.
///
/// Without one, the crate searches the Jupyter data directories and asks the `jupyter`
/// command for the rest. xeus-python installs no `jupyter` command, though, so
/// under pixi or conda that leaves the environment's own kernels invisible;
/// look under the prefix ourselves.
async fn kernelspec_dirs(environment: Option<&Environment>) -> Vec<PathBuf> {
    if let Some(environment) = environment {
        return vec![environment.prefix.join("share").join("jupyter")];
    }
    let mut dirs = Vec::new();
    if let Some(prefix) = std::env::var_os("CONDA_PREFIX") {
        dirs.push(PathBuf::from(prefix).join("share").join("jupyter"));
    }
    dirs.extend(jupyter_zmq_client::dirs::data_dirs_with_jupyter_paths().await);
    dirs
}

/// The first of `names` that is installed, so a document can run on whichever
/// kernel the environment provides.
async fn find_kernelspec(
    names: &[&str],
    environment: Option<&Environment>,
) -> Result<KernelspecDir> {
    let mut installed = Vec::new();
    for dir in kernelspec_dirs(environment).await {
        installed.extend(jupyter_zmq_client::read_kernelspec_jsons(&dir).await);
    }
    names
        .iter()
        .find_map(|name| installed.iter().find(|spec| spec.kernel_name == *name))
        .cloned()
        .with_context(|| {
            let names: Vec<&str> = installed.iter().map(|s| s.kernel_name.as_str()).collect();
            match environment {
                Some(environment) => format!(
                    "the kernels installed in {} are {names:?}",
                    environment.prefix.display()
                ),
                None => format!("the kernels installed here are {names:?}"),
            }
        })
}

/// How often to check whether a starting kernel listens on its ports.
const PORT_POLL: Duration = Duration::from_millis(5);

/// Waits until the kernel listens on each of `ports`, or fails if it exits
/// first. zeromq retries a refused connection only after more than a second,
/// which a kernel that is still starting up would always cost.
async fn wait_for_ports(process: &mut Child, name: &str, ip: IpAddr, ports: &[u16]) -> Result<()> {
    for &port in ports {
        while tokio::net::TcpStream::connect((ip, port)).await.is_err() {
            if let Some(status) = process.try_wait()? {
                anyhow::bail!("the `{name}` kernel exited on startup with {status}");
            }
            tokio::time::sleep(PORT_POLL).await;
        }
    }
    Ok(())
}

impl Kernel {
    /// Start the first of these kernels that is installed, most preferred
    /// first, in the directory `dir` and in `environment`, and connect to it.
    pub async fn start(
        kernel_names: &[&str],
        dir: &Path,
        environment: Option<&Environment>,
    ) -> Result<Kernel> {
        let kernelspec = find_kernelspec(kernel_names, environment)
            .await
            .with_context(|| format!("could not find any of the {kernel_names:?} kernels"))?;
        let kernel_name = kernelspec.kernel_name.clone();
        let interrupted_by_message =
            kernelspec.kernelspec.interrupt_mode.as_deref() == Some("message");

        // The kernel binds these ports; we only pick ones that are free now.
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let ports = jupyter_zmq_client::peek_ports(ip, 5).await?;
        let connection_info = ConnectionInfo {
            transport: Transport::TCP,
            ip: ip.to_string(),
            stdin_port: ports[0],
            control_port: ports[1],
            hb_port: ports[2],
            shell_port: ports[3],
            iopub_port: ports[4],
            signature_scheme: "hmac-sha256".to_string(),
            key: Uuid::new_v4().to_string(),
            kernel_name: Some(kernel_name.to_string()),
        };

        let runtime_dir = jupyter_zmq_client::dirs::runtime_dir();
        tokio::fs::create_dir_all(&runtime_dir)
            .await
            .with_context(|| format!("could not create {}", runtime_dir.display()))?;
        let connection_file = runtime_dir.join(format!("kernel-{}.json", Uuid::new_v4()));
        tokio::fs::write(&connection_file, serde_json::to_vec(&connection_info)?)
            .await
            .with_context(|| format!("could not write {}", connection_file.display()))?;
        let connection_file = ConnectionFile(connection_file);

        let mut command = kernelspec.command(&connection_file.0, None, None)?;
        if let Some(environment) = environment {
            for name in FOREIGN {
                command.env_remove(name);
            }
            command.envs(&environment.vars);
        }
        // What the kernel prints of its own goes to stderr, leaving stdout to
        // whoever runs us: the extension reads its answers there.
        let mut process = command
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(std::io::stderr())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not start the `{kernel_name}` kernel"))?;

        let listening = [connection_info.shell_port, connection_info.iopub_port];
        wait_for_ports(&mut process, &kernel_name, ip, &listening).await?;

        let session_id = Uuid::new_v4().to_string();
        let mut iopub =
            jupyter_zmq_client::create_client_iopub_connection(&connection_info, "", &session_id)
                .await?;
        let identity = jupyter_zmq_client::peer_identity_for_session(&session_id)?;
        let shell = jupyter_zmq_client::create_client_shell_connection_with_identity(
            &connection_info,
            &session_id,
            identity,
        )
        .await?;

        // iopub is a subscription, so anything the kernel publishes before it
        // sees our subscription is lost. Waiting for its welcome means the
        // first cell's output is not the one that goes missing.
        jupyter_zmq_client::wait_for_iopub_welcome(&mut iopub, WELCOME_TIMEOUT).await?;

        let mut kernel = Kernel {
            name: kernel_name,
            process,
            shell,
            iopub,
            connection_info,
            session_id,
            interrupted_by_message,
            _connection_file: connection_file,
        };
        // The setup belongs to no cell, so it is left out of the count, and
        // what it prints is not kept.
        kernel.run_silent(SETUP).await?;
        Ok(kernel)
    }

    /// Run one cell and collect everything the kernel publishes for it.
    pub async fn run(&mut self, code: &str) -> Result<Vec<Output>> {
        let (outputs, _) = self.execute(ExecuteRequest::new(code.to_string())).await?;
        Ok(outputs)
    }

    /// Run one cell as a notebook shows it: its outputs, along with whether it
    /// raised and the count the kernel gave it.
    pub async fn run_cell(&mut self, code: &str) -> Result<Ran> {
        let (outputs, reply) = self.execute(ExecuteRequest::new(code.to_string())).await?;
        let JupyterMessageContent::ExecuteReply(reply) = reply.content else {
            anyhow::bail!("unexpected reply {:?}", reply.content);
        };
        Ok(Ran {
            outputs,
            ok: reply.status == ReplyStatus::Ok,
            count: reply.execution_count.0,
        })
    }

    /// What interrupts the cell the kernel runs, from beside whatever waits for
    /// its outputs.
    pub async fn interrupter(&self) -> Result<Interrupter> {
        let control = jupyter_zmq_client::create_client_control_connection(
            &self.connection_info,
            &self.session_id,
        )
        .await?;
        Ok(Interrupter {
            control,
            pid: self.process.id(),
            by_message: self.interrupted_by_message,
        })
    }

    /// Run code that belongs to no cell: it is left out of the history and
    /// the execution count, and fails if the code raises.
    pub async fn run_silent(&mut self, code: &str) -> Result<()> {
        let mut request = ExecuteRequest::new(code.to_string());
        request.silent = true;
        request.store_history = false;
        let (_, reply) = self.execute(request).await?;
        match reply.content {
            JupyterMessageContent::ExecuteReply(ExecuteReply {
                status: ReplyStatus::Ok,
                ..
            }) => Ok(()),
            JupyterMessageContent::ExecuteReply(ExecuteReply {
                error: Some(error), ..
            }) => anyhow::bail!("{}: {}", error.ename, error.evalue),
            other => anyhow::bail!("unexpected reply {other:?}"),
        }
    }

    /// Send an execute request, and collect everything the kernel publishes
    /// for it along with its reply.
    async fn execute(&mut self, request: ExecuteRequest) -> Result<(Vec<Output>, JupyterMessage)> {
        let request: JupyterMessage = request.into();
        let request_id = request.header.msg_id.clone();
        self.shell.send(request).await?;

        let mut outputs = Outputs::default();
        loop {
            // A kernel that exits, as xeus-python does when interrupted, would
            // leave us waiting for the rest of the cell's outputs forever.
            let message = tokio::select! {
                message = self.iopub.read() => message?,
                status = self.process.wait() => {
                    anyhow::bail!("the `{}` kernel exited with {}", self.name, status?)
                }
            };
            // The kernel also publishes messages of its own, and replies to
            // whatever else is on the wire; take only this cell's.
            let parent = message.parent_header.as_ref().map(|h| h.msg_id.as_str());
            if parent != Some(request_id.as_str()) {
                continue;
            }
            match &message.content {
                // The kernel goes idle once it has published everything.
                JupyterMessageContent::Status(status)
                    if status.execution_state == ExecutionState::Idle =>
                {
                    break;
                }
                JupyterMessageContent::StreamContent(stream) => outputs.push_stream(stream),
                JupyterMessageContent::ExecuteResult(result) => {
                    outputs.push_media(&result.data, None)?
                }
                JupyterMessageContent::DisplayData(display) => {
                    let id = display.transient.as_ref();
                    let id = id.and_then(|transient| transient.display_id.as_deref());
                    outputs.push_media(&display.data, id)?
                }
                JupyterMessageContent::UpdateDisplayData(update) => {
                    if let Some(id) = &update.transient.display_id {
                        outputs.update_media(&update.data, id)?
                    }
                }
                JupyterMessageContent::ClearOutput(clear) => outputs.clear(clear.wait),
                JupyterMessageContent::ErrorOutput(error) => outputs.push_error(error),
                _ => {}
            }
        }

        // The reply reaches shell before that idle status, so it is waiting for
        // us now. Read it so it does not sit in front of the next cell's.
        let reply = self.shell.read().await?;
        Ok((outputs.into_vec(), reply))
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.process.start_kill()?;
        self.process.wait().await?;
        Ok(())
    }
}
