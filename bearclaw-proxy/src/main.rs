#[allow(clippy::all, dead_code)]
mod bearclaw_capnp {
    include!(concat!(env!("OUT_DIR"), "/bearclaw_capnp.rs"));
}
mod rpc;
mod storage;

use std::{any::Any, ops::Deref, path::PathBuf, sync::Arc, time::Duration};

use clap::Parser;
use futures::AsyncReadExt;

/// Maximum number of concurrent RPC connections allowed. Connections that exceed this limit will not
/// be accepted until an existing connection is closed and could result in new connections being
/// silently dropped.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_CONCURRENT_RPC_CONNECTIONS: usize = 64;

/// Start sending keepalive probes after this duration of idleness
const RPC_SOCKET_KEEPALIVE_TIME: Duration = Duration::from_secs(60);

/// Send subsequent keepalive probes after this duration
const RPC_SOCKET_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

/// Close the connection after this many keepalive probe failures
const RPC_SOCKET_KEEPALIVE_RETRIES: u32 = 6;

/// Maximum number of messages that can be queued in the storage channel. If the channel is
/// full, senders will block until the receiver catches up.
/// TODO: Value chosen arbitrarily.
const STORAGE_CHANNEL_CAPACITY: usize = 4;

#[tokio::main]
async fn main() -> Result<()> {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("Panic: {:?}", info);
        default_panic(info);
        std::process::exit(1);
    }));

    if std::env::args().any(|x| x == "--contributors") {
        print_contributors();
        return Ok(());
    }

    if std::env::args().any(|x| x == "--third-party-licenses") {
        print_third_party_licenses();
        return Ok(());
    }

    console_subscriber::init();
    trace_version();

    let args = Args::parse();

    // needed for STRICT tables
    if rusqlite::version_number() < 3037000 {
        panic!(
            "Sqlite version 3.37 or later is required. Your version: {}",
            rusqlite::version(),
        );
    }

    tracing::info!("Command Line {args:#?}");

    tracing::info!("Opening Project File");
    let db = storage::Database::open_or_create(&args.project_file)?;
    let (storage_tx, storage_rx) = tokio::sync::mpsc::channel(STORAGE_CHANNEL_CAPACITY);
    let storage_thread = std::thread::Builder::new()
        .name("storage".to_owned())
        .spawn(move || db.run(storage_rx))?;
    let storage_channel = storage::Channel::from_sender(storage_tx);
    let (death_notification_tx, mut death_notification_rx) = tokio::sync::mpsc::channel(1);
    let (shutdown_notification_tx, shutdown_notification_rx) =
        tokio::sync::watch::channel::<()>(());
    let (shutdown_command_tx, mut shutdown_command_rx) = tokio::sync::mpsc::channel::<()>(1);

    tracing::trace!("Creating thread pool to run Cap'n Proto vats");
    let rpc_spawner = RpcSpawner::new(
        storage_channel,
        Arc::new(shutdown_command_tx),
        shutdown_notification_rx.clone(),
        death_notification_tx.clone(),
    );

    tracing::info!("Listening on RPC Endpoint");
    let rpc_limiter = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_RPC_CONNECTIONS));
    let rpc_listener = tokio::net::TcpListener::bind(args.rpc_endpoint).await?;

    tokio::task::Builder::new()
        .name("rpc-socket-listener")
        .spawn(rpc_task(
            rpc_listener,
            rpc_limiter,
            rpc_spawner,
            shutdown_notification_rx,
            death_notification_tx,
        ))?;

    tracing::trace!("Creating storage thread shutdown waiter");
    let mut storage_thread_waiter = tokio::task::Builder::new()
        .name("storage-thread-shutdown-waiter")
        .spawn_blocking(|| storage_thread.join())?;

    tracing::trace!("Waiting for shutdown command");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown command received from CTRL+C");
        },
        _ = shutdown_command_rx.recv() => {
            tracing::info!("Shutdown command received from RPC client");
        },
        result = &mut storage_thread_waiter => {
            result??;
            panic!("Storage thread unexpectedly stopped");
        }
    }

    tracing::trace!("telling all tasks to shut down");
    shutdown_notification_tx.send(())?;

    tracing::trace!("waiting for all tasks to shut down");
    let _ = death_notification_rx.recv().await;

    tracing::trace!("waiting for storage thread to shut down");
    storage_thread_waiter.await??;

    tracing::trace!("Shutdown complete, goodbye world o7");
    Ok(())
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Error {
    Channel,
    IOFailure(std::io::Error),
    OpenProjectFile(storage::OpenError),
    StorageChannel(storage::ChannelError),
    Join(tokio::task::JoinError),
    Other(Box<dyn Any + std::marker::Send>),
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

impl From<storage::OpenError> for Error {
    fn from(e: storage::OpenError) -> Self {
        Self::OpenProjectFile(e)
    }
}

impl From<storage::ChannelError> for Error {
    fn from(e: storage::ChannelError) -> Self {
        Self::StorageChannel(e)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Join(e)
    }
}

impl From<Box<dyn Any + std::marker::Send>> for Error {
    fn from(e: Box<dyn Any + std::marker::Send>) -> Self {
        Self::Other(e)
    }
}

fn print_contributors() {
    println!("{}", include_str!("../../CONTRIBUTORS"));
}

fn print_third_party_licenses() {
    println!(
        "{}",
        include_str!(concat!(env!("OUT_DIR"), "/licenses.txt"))
    );
}

fn trace_version() {
    let dirty_build = git_version::git_version!(fallback = "").contains("-modified");

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
    tracing::info!("Using sqlite version {}", rusqlite::version());
    tracing::info!("Using zstd {}", zstd::zstd_safe::version_string());
}

#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Args {
    /// RPC endpoint to listen on. THIS IS NOT ENCRYPTED!
    #[clap(short, long, default_value = "localhost:3092")]
    rpc_endpoint: String,

    /// Path to project file. A new file is created if it does not exist.
    #[clap(short, long)]
    project_file: PathBuf,

    /// Print contributors to this application
    #[clap(long)]
    contributors: bool,

    /// Print information about third-party code used by this application
    #[clap(long)]
    third_party_licenses: bool,
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
        storage: storage::Channel,
        shutdown_command_tx: Arc<tokio::sync::mpsc::Sender<()>>,
        shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        let (send, recv) = async_channel::bounded(MAX_CONCURRENT_RPC_CONNECTIONS);

        for thread_id in 0..num_cpus::get_physical() {
            let thread_id = thread_id + 1;
            let recv: async_channel::Receiver<SpawnerPayload> = recv.clone();
            let storage = storage.clone();
            let shutdown_command_tx = shutdown_command_tx.clone();
            let mut shutdown_notification_rx = shutdown_notification_rx.clone();
            let death_notification_tx = death_notification_tx.clone();

            tracing::trace!("creating rpc thread {thread_id}");

            // Why can't this just spawn local tasks onto the existing tokio thread pool threads
            // instead of creating its own separate threads? This is based on the example in the
            // documentation for tokio::task::LocalSet.
            std::thread::Builder::new()
                .name(format!("rpc-{thread_id}"))
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    let local = tokio::task::LocalSet::new();
                    let death_notification_tx = death_notification_tx.clone();

                    tokio::task::Builder::new()
                        .name(&format!("rpc-spawner-tid{thread_id}"))
                        .spawn_local_on(
                            async move {
                                let death_notification_tx = death_notification_tx.clone();
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
                                            storage.clone(),
                                            shutdown_command_tx.deref().clone(),
                                            shutdown_notification_rx.clone(),
                                            death_notification_tx.clone(),
                                            thread_id,
                                            client_id,
                                        ));
                                    let rpc_system = capnp_rpc::RpcSystem::new(
                                        Box::new(network),
                                        Some(initial_object.client),
                                    );
                                    let disconnector = rpc_system.get_disconnector();
                                    let mut shutdown_notification_rx =
                                        shutdown_notification_rx.clone();
                                    let death_notification_tx = death_notification_tx.clone();

                                    tokio::task::Builder::new()
                                        .name(&format!("rpc-tid{thread_id}-cid{client_id}"))
                                        .spawn_local(async move {
                                            let _death_notification_tx = death_notification_tx;
                                            tracing::trace!("executing capnp rpc system");
                                            let result = tokio::select! {
                                                result = rpc_system => { result }
                                                _ = shutdown_notification_rx.changed() => {
                                                    tracing::trace!(
                                                        "Shutdown notification, awaiting RPC system disconnector"
                                                    );
                                                    disconnector.await
                                                }
                                            };
                                            drop(permit);

                                            tracing::trace!(
                                                "capnp rpc system exited with result: {result:?}"
                                            );
                                            tracing::debug!("rpc client {client_id} disconnected");

                                            result
                                        })
                                        .unwrap();
                                }
                            },
                            &local,
                        )
                        .unwrap();

                    rt.block_on(local);
                })
                .unwrap();
        }

        Self { sender: send }
    }

    async fn spawn(&self, args: SpawnerPayload) -> Result<()> {
        // Use async_channel to randomly assign this to one of the threads in the thread pool
        self.sender.send(args).await?;
        Ok(())
    }
}

async fn rpc_task(
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
                "Maximum number of RPC client connections ({MAX_CONCURRENT_RPC_CONNECTIONS}) reached. \
                New RPC connections will be ignored until an existing client disconnects.",
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
            result = listener.accept() => {
                match result {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::debug!("RPC socket accept returned error: {e:?}");
                        continue;
                    }
                }
            }
            _ = shutdown_notification_rx.changed() => {
                tracing::trace!("shutting down");
                return Ok(());
            }
        };

        tracing::debug!("new rpc client {client_id} connected");

        let sockref = socket2::SockRef::from(&stream);
        // Don't slow start
        sockref.set_nodelay(true)?;
        // Don't let broken connections linger
        // (See https://blog.cloudflare.com/when-tcp-sockets-refuse-to-die)
        sockref.set_tcp_keepalive(
            &socket2::TcpKeepalive::new()
                .with_time(RPC_SOCKET_KEEPALIVE_TIME)
                .with_interval(RPC_SOCKET_KEEPALIVE_INTERVAL)
                .with_retries(RPC_SOCKET_KEEPALIVE_RETRIES),
        )?;
        let user_timeout = sockref.keepalive_time()?
            + sockref.keepalive_interval()? * sockref.keepalive_retries()?;
        sockref.set_tcp_user_timeout(Some(user_timeout))?;

        spawner.spawn((client_id, permit, stream)).await?;

        client_id += 1;
    }
}
