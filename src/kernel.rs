//! Driving a Jupyter kernel over ZeroMQ.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use jupyter_protocol::connection_info::Transport;
use jupyter_protocol::{
    ConnectionInfo, ExecuteRequest, ExecutionState, JupyterMessage, JupyterMessageContent,
};
use jupyter_zmq_client::{ClientIoPubConnection, ClientShellConnection};
use tokio::process::Child;
use uuid::Uuid;

use crate::output::{Output, Outputs};

/// How long to wait for the kernel to greet us on iopub before assuming it
/// speaks the older protocol, which has no greeting.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(30);

/// A running Jupyter kernel. Cells sent to it share one interpreter, so state
/// carries over from one cell to the next, exactly as in a notebook.
pub struct Kernel {
    process: Child,
    shell: ClientShellConnection,
    iopub: ClientIoPubConnection,
    connection_file: PathBuf,
}

impl Kernel {
    /// Start the kernel of the given kernelspec name (e.g. `python3`) and
    /// connect to it.
    pub async fn start(kernel_name: &str) -> Result<Kernel> {
        let kernelspec = jupyter_zmq_client::find_kernelspec_with_jupyter_paths(kernel_name)
            .await
            .with_context(|| format!("could not find the `{kernel_name}` kernel"))?;

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

        Ok(Kernel {
            process,
            shell,
            iopub,
            connection_file,
        })
    }

    /// Run one cell and collect everything the kernel publishes for it.
    pub async fn run(&mut self, code: &str) -> Result<Vec<Output>> {
        let request: JupyterMessage = ExecuteRequest::new(code.to_string()).into();
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
        self.shell.read().await?;
        Ok(outputs.into_vec())
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.process.start_kill()?;
        self.process.wait().await?;
        tokio::fs::remove_file(&self.connection_file).await.ok();
        Ok(())
    }
}
