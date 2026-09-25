//! Driving a Jupyter kernel over ZeroMQ.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use jupyter_protocol::connection_info::Transport;
use jupyter_protocol::{
    ConnectionInfo, ExecuteReply, ExecuteRequest, ExecutionState, JupyterMessage,
    JupyterMessageContent, ReplyStatus,
};
use jupyter_zmq_client::{ClientIoPubConnection, ClientShellConnection, KernelspecDir};
use tokio::process::Child;
use uuid::Uuid;

use crate::output::{Output, Outputs};

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
    process: Child,
    shell: ClientShellConnection,
    iopub: ClientIoPubConnection,
    connection_file: PathBuf,
}

/// Where to look for kernelspecs.
///
/// The crate searches the Jupyter data directories and asks the `jupyter`
/// command for the rest. xeus-python installs no `jupyter` command, though, so
/// under pixi or conda that leaves the environment's own kernels invisible;
/// look under the prefix ourselves.
async fn kernelspec_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(prefix) = std::env::var_os("CONDA_PREFIX") {
        dirs.push(PathBuf::from(prefix).join("share").join("jupyter"));
    }
    dirs.extend(jupyter_zmq_client::dirs::data_dirs_with_jupyter_paths().await);
    dirs
}

/// The first of `names` that is installed, so a document can run on whichever
/// kernel the environment provides.
async fn find_kernelspec(names: &[&str]) -> Result<KernelspecDir> {
    let mut installed = Vec::new();
    for dir in kernelspec_dirs().await {
        installed.extend(jupyter_zmq_client::read_kernelspec_jsons(&dir).await);
    }
    names
        .iter()
        .find_map(|name| installed.iter().find(|spec| spec.kernel_name == *name))
        .cloned()
        .with_context(|| {
            let names: Vec<&str> = installed.iter().map(|s| s.kernel_name.as_str()).collect();
            format!("the kernels installed here are {names:?}")
        })
}

impl Kernel {
    /// Start the first of these kernels that is installed, most preferred
    /// first, and connect to it.
    pub async fn start(kernel_names: &[&str]) -> Result<Kernel> {
        let kernelspec = find_kernelspec(kernel_names)
            .await
            .with_context(|| format!("could not find any of the {kernel_names:?} kernels"))?;
        let kernel_name = kernelspec.kernel_name.clone();
        println!("running the cells on the `{kernel_name}` kernel");

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

        let process = kernelspec
            .command(&connection_file, None, None)?
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("could not start the `{kernel_name}` kernel"))?;

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
            process,
            shell,
            iopub,
            connection_file,
        };
        // The setup belongs to no cell, so what it prints is not kept.
        kernel.run(SETUP).await?;
        Ok(kernel)
    }

    /// Run one cell and collect everything the kernel publishes for it.
    pub async fn run(&mut self, code: &str) -> Result<Vec<Output>> {
        let (outputs, _) = self.execute(ExecuteRequest::new(code.to_string())).await?;
        Ok(outputs)
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
            let message = self.iopub.read().await?;
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
                JupyterMessageContent::ExecuteResult(result) => outputs.push_media(&result.data)?,
                JupyterMessageContent::DisplayData(display) => outputs.push_media(&display.data)?,
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
        tokio::fs::remove_file(&self.connection_file).await.ok();
        Ok(())
    }
}
