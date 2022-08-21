#[allow(clippy::all, dead_code)]
mod bearclaw_capnp {
    include!(concat!(env!("OUT_DIR"), "/bearclaw_capnp.rs"));
}
mod bootstrap_proxy;
mod rpc;

use std::{ops::Deref, sync::Arc};

use clap::Parser;
use futures::AsyncReadExt;

/// Maximum number of concurrent RPC connections allowed. Connections that exceed this limit will not
/// be accepted until an existing connection is closed and could result in new connections being
/// silently dropped.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_CONCURRENT_RPC_CONNECTIONS: usize = 64;

/// Maximum number of messages that can be queued in the proxy intercepted message notification
/// channel. If a receiver cannot process messages fast enough it will receive an error and fail.
/// TODO: Value chosen arbitrarily.
const PROXY_BROADCAST_CHANNEL_CAPACITY: usize = 16;

/// Maximum number of messages that can be queued in the proxy command channel. If the channel is
/// full, senders will block until the receiver catches up.
/// TODO: Value chosen arbitrarily.
const PROXY_COMMAND_CHANNEL_CAPACITY: usize = 2;

#[tokio::main]
async fn main() -> Result<()> {
    console_subscriber::init();
    trace_version();

    let args = Args::parse();

    tracing::info!(
        "Bootstrap proxy plugin endpoint: {}",
        &args.bootstrap_proxy_endpoint
    );
    tracing::info!("RPC endpoint: {}", &args.rpc_endpoint);

    tracing::info!("Connecting to Bootstrap Proxy");
    let interceptor =
        bootstrap_proxy::Interceptor::connect(args.bootstrap_proxy_endpoint.as_ref()).await?;

    let (death_notification_tx, mut death_notification_rx) = tokio::sync::mpsc::channel(1);
    let (shutdown_notification_tx, shutdown_notification_rx) =
        tokio::sync::watch::channel::<()>(());

    tracing::info!("Running proxy interceptor");
    let (interceptor_tx, interceptor_rx) =
        tokio::sync::broadcast::channel(PROXY_BROADCAST_CHANNEL_CAPACITY);
    // subscriptions will be created from the tx side
    drop(interceptor_rx);

    tokio::task::Builder::new()
        .name("proxy-inteceptor")
        .spawn(proxy(
            interceptor,
            interceptor_tx.clone(),
            shutdown_notification_rx.clone(),
            death_notification_tx.clone(),
        ));

    let (proxy_command_tx, proxy_command_rx) =
        tokio::sync::mpsc::channel(PROXY_COMMAND_CHANNEL_CAPACITY);

    tokio::task::Builder::new()
        .name("proxy-command")
        .spawn(proxy_command(
            interceptor_tx,
            proxy_command_rx,
            shutdown_notification_rx.clone(),
            death_notification_tx.clone(),
        ));

    let (shutdown_command_tx, mut shutdown_command_rx) = tokio::sync::mpsc::channel::<()>(1);

    tracing::trace!("Creating thread pool to run Cap'n Proto vats");
    let rpc_spawner = RpcSpawner::new(
        Arc::new(args.bootstrap_proxy_endpoint),
        Arc::new(proxy_command_tx),
        Arc::new(shutdown_command_tx),
        shutdown_notification_rx.clone(),
        death_notification_tx.clone(),
    );

    tracing::info!("Listening on RPC Endpoint");
    let rpc_limiter = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_RPC_CONNECTIONS));
    let rpc_listener = tokio::net::TcpListener::bind(args.rpc_endpoint).await?;

    tokio::task::Builder::new()
        .name("rpc-socket-listener")
        .spawn(rpc(
            rpc_listener,
            rpc_limiter,
            rpc_spawner,
            shutdown_notification_rx,
            death_notification_tx,
        ));

    tracing::trace!("Waiting for shutdown command");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown command received from CTRL+C");
        },
        _ = shutdown_command_rx.recv() => {
            tracing::info!("Shutdown command received from RPC client");
        },
    }

    tracing::trace!("telling all tasks to shut down");
    shutdown_notification_tx.send(())?;

    tracing::trace!("waiting for all tasks to shut down");
    let _ = death_notification_rx.recv().await;

    tracing::trace!("Shutdown complete, goodbye world o7");
    Ok(())
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Error {
    BootstrapProxy(bootstrap_proxy::Error),
    Channel,
    IOFailure(std::io::Error),
}

impl From<bootstrap_proxy::Error> for Error {
    fn from(e: bootstrap_proxy::Error) -> Self {
        Self::BootstrapProxy(e)
    }
}

impl<T> From<tokio::sync::broadcast::error::SendError<T>> for Error {
    fn from(_: tokio::sync::broadcast::error::SendError<T>) -> Self {
        Self::Channel
    }
}

impl From<tokio::sync::broadcast::error::RecvError> for Error {
    fn from(_: tokio::sync::broadcast::error::RecvError) -> Self {
        Self::Channel
    }
}

impl<T> From<tokio::sync::watch::error::SendError<T>> for Error {
    fn from(_: tokio::sync::watch::error::SendError<T>) -> Self {
        Self::Channel
    }
}

impl<T> From<async_channel::SendError<T>> for Error {
    fn from(_: async_channel::SendError<T>) -> Self {
        Self::Channel
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::IOFailure(e)
    }
}

fn trace_version() {
    let dirty_build = git_version::git_version!().contains("-modified");

    tracing::info!(
        "bearclaw-proxy {}{}",
        env!("VERGEN_BUILD_SEMVER"),
        if dirty_build { "-dirty" } else { "" }
    );
    tracing::info!("{} build", env!("VERGEN_CARGO_PROFILE"));
    tracing::info!(
        "Built from {}{} branch commit {} from {}",
        if dirty_build {
            "**UNCOMMITTED CHANGES** to "
        } else {
            ""
        },
        option_env!("VERGEN_GIT_BRANCH").unwrap_or("(not on a branch)"),
        option_env!("VERGEN_GIT_SHA").unwrap_or("(uncommitted)"),
        option_env!("VERGEN_GIT_COMMIT_TIMESTAMP").unwrap_or("(uncommitted)"),
    );
    tracing::info!(
        "Built on {} with rustc {} for {}",
        env!("VERGEN_BUILD_TIMESTAMP"),
        env!("VERGEN_RUSTC_SEMVER"),
        env!("VERGEN_CARGO_TARGET_TRIPLE"),
    );
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Bootstrap proxy plugin endpoint to connect to. THIS IS NOT ENCRYPTED!
    #[clap(short, long)]
    bootstrap_proxy_endpoint: String,

    /// RPC endpoint to listen on. THIS IS NOT ENCRYPTED!
    #[clap(short, long, default_value = "localhost:3092")]
    rpc_endpoint: String,
}

async fn proxy(
    mut interceptor: bootstrap_proxy::Interceptor,
    broadcast_tx: tokio::sync::broadcast::Sender<bootstrap_proxy::InterceptedMessage>,
    mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
    _death_notification_tx: tokio::sync::mpsc::Sender<()>,
) -> Result<()> {
    loop {
        tokio::select! {
            message = interceptor.intercept() => {
                let message = message?;
                // It's OK for this to return an error if there are no subscribers
                let _ = broadcast_tx.send(message);
            }
            _ = shutdown_notification_rx.changed() => {
                tracing::trace!("shutting down");
                return Ok(());
            }
        }
    }
}

async fn proxy_command(
    broadcast_tx: tokio::sync::broadcast::Sender<bootstrap_proxy::InterceptedMessage>,
    mut command_rx: tokio::sync::mpsc::Receiver<ProxyCommand>,
    mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
    _death_notification_tx: tokio::sync::mpsc::Sender<()>,
) {
    loop {
        tokio::select! {
            msg = command_rx.recv() => {
                match msg {
                    Some(ProxyCommand::Subscribe(reply_tx)) => {
                        let _ = reply_tx.send(broadcast_tx.subscribe());
                    }
                    None => {
                        tracing::debug!("exiting due to command channel receive failure");
                        return;
                    }
                }
            }
            _ = shutdown_notification_rx.changed() => {
                tracing::trace!("shutting down");
                return;
            }
        }
    }
}

#[derive(Debug)]
enum ProxyCommand {
    Subscribe(
        tokio::sync::oneshot::Sender<
            tokio::sync::broadcast::Receiver<bootstrap_proxy::InterceptedMessage>,
        >,
    ),
}

struct RpcSpawner {
    sender: async_channel::Sender<SpawnerPayload>,
}

type SpawnerPayload = (
    usize,
    tokio::sync::OwnedSemaphorePermit,
    tokio::net::TcpStream,
);

impl RpcSpawner {
    fn new(
        bootstrap_proxy_endpoint: Arc<String>,
        proxy_command_tx: Arc<tokio::sync::mpsc::Sender<ProxyCommand>>,
        shutdown_command_tx: Arc<tokio::sync::mpsc::Sender<()>>,
        shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        let (send, recv) = async_channel::bounded(MAX_CONCURRENT_RPC_CONNECTIONS);

        for thread_id in 0..num_cpus::get_physical() {
            let thread_id = thread_id + 1;
            let recv: async_channel::Receiver<SpawnerPayload> = recv.clone();
            let bootstrap_proxy_endpoint = bootstrap_proxy_endpoint.clone();
            let proxy_command_tx = proxy_command_tx.clone();
            let shutdown_command_tx = shutdown_command_tx.clone();
            let mut shutdown_notification_rx = shutdown_notification_rx.clone();
            let death_notification_tx = death_notification_tx.clone();

            tracing::trace!("creating rpc thread {}", thread_id);

            // Why can't this just spawn local tasks onto the existing tokio thread pool threads
            // instead of creating its own separate threads? This is based on the example in the
            // documentation for tokio::task::LocalSet.
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let local = tokio::task::LocalSet::new();

                tokio::task::Builder::new()
                    .name(&format!("rpc-spawner-tid{}", thread_id))
                    .spawn_local_on(
                        async move {
                            tracing::trace!("waiting for capnp rpc spawn request");

                            while let Ok((client_id, permit, stream)) = tokio::select! {
                                result = recv.recv() => { result }
                                _ = shutdown_notification_rx.changed() => {
                                    tracing::trace!("shutting down");
                                    return;
                                }
                            } {
                                tracing::trace!(
                                    "creating capnp rpc system for client {}",
                                    client_id
                                );

                                let (reader, writer) =
                                    tokio_util::compat::TokioAsyncReadCompatExt::compat(stream)
                                        .split();
                                let network = capnp_rpc::twoparty::VatNetwork::new(
                                    reader,
                                    writer,
                                    capnp_rpc::rpc_twoparty_capnp::Side::Server,
                                    Default::default(),
                                );
                                let initial_object: bearclaw_capnp::bearclaw::Client =
                                    capnp_rpc::new_client(rpc::BearclawImpl::new(
                                        bootstrap_proxy_endpoint.clone(),
                                        proxy_command_tx.deref().clone(),
                                        shutdown_command_tx.deref().clone(),
                                        death_notification_tx.clone(),
                                        thread_id,
                                        client_id,
                                    ));
                                let rpc_system = capnp_rpc::RpcSystem::new(
                                    Box::new(network),
                                    Some(initial_object.client),
                                );

                                let mut shutdown_notification_rx = shutdown_notification_rx.clone();

                                tokio::task::Builder::new()
                                    .name(&format!("rpc-tid{}-cid{}", thread_id, client_id))
                                    .spawn_local(async move {
                                        tracing::trace!("executing capnp rpc system");
                                        let result = tokio::select! {
                                            result = rpc_system => { result }
                                            _ = shutdown_notification_rx.changed() => {
                                                // TODO: Is there a way to gracefully shut down
                                                // the rpc_system?
                                                tracing::trace!("shutting down");
                                                return Ok(());
                                            }
                                        };
                                        drop(permit);

                                        tracing::trace!(
                                            "capnp rpc system exited with result: {:?}",
                                            result
                                        );
                                        tracing::debug!("rpc client {} disconnected", client_id);

                                        result
                                    });
                            }
                        },
                        &local,
                    );

                rt.block_on(local);
            });
        }

        Self { sender: send }
    }

    async fn spawn(&self, args: SpawnerPayload) -> Result<()> {
        // Use async_channel to randomly assign this to one of the threads in the thread pool
        self.sender.send(args).await?;
        Ok(())
    }
}

async fn rpc(
    listener: tokio::net::TcpListener,
    limiter: Arc<tokio::sync::Semaphore>,
    spawner: RpcSpawner,
    mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
    _death_notification_tx: tokio::sync::mpsc::Sender<()>,
) -> Result<()> {
    let mut client_id = 1;

    loop {
        if limiter.available_permits() == 0 {
            tracing::warn!(
                "Maximum number of RPC client connections ({}) reached. New RPC connections will be ignored until an existing client disconnects.",
                MAX_CONCURRENT_RPC_CONNECTIONS
            );
        }

        tracing::trace!("waiting for permit to accept a new rpc client");
        let permit = tokio::select! {
            permit = limiter.clone().acquire_owned() => {
                permit.unwrap()
            }
            _ = shutdown_notification_rx.changed() => {
                tracing::trace!("shutting down");
                return Ok(());
            }
        };

        tracing::trace!("listening for a new rpc client");
        let (stream, _) = tokio::select! {
            result = listener.accept() => { result? }
            _ = shutdown_notification_rx.changed() => {
                tracing::trace!("shutting down");
                return Ok(());
            }
        };

        tracing::debug!("new rpc client {} connected", client_id);
        stream.set_nodelay(true)?;
        spawner.spawn((client_id, permit, stream)).await?;

        client_id += 1;
    }
}