use std::{cell::Cell, rc::Rc, sync::Arc};

use capnp::capability::Promise;
use capnp_rpc::pry;

use crate::bearclaw_capnp;

/// Maximum number of subscriptions (callbacks) to RPC connections that are allowed. Exceeding this
/// limit will create a capnproto error and cause the RPC connection to be closed.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_SUBSCRIPTIONS_PER_RPC_CONNECTION: usize = 128;

/// Maximum number of message objects each RPC connections may create. Exceeding this
/// limit will either create a capnproto error and cause the RPC connection to be closed
/// or cause a subscription to be silently deleted.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_MESSAGE_OBJECTS_PER_RPC_CONNECTION: usize = 128;

pub(super) struct BearclawImpl {
    bootstrap_proxy_endpoint: Arc<String>,
    storage: crate::storage::Channel,
    proxy_command_tx: tokio::sync::mpsc::Sender<crate::ProxyCommand>,
    shutdown_command_tx: tokio::sync::mpsc::Sender<()>,
    death_notification_tx: tokio::sync::mpsc::Sender<()>,
    thread_id: usize,
    client_id: usize,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: usize,
    num_active_messages: Rc<Cell<usize>>,
}

impl BearclawImpl {
    pub(super) fn new(
        bootstrap_proxy_endpoint: Arc<String>,
        storage: crate::storage::Channel,
        proxy_command_tx: tokio::sync::mpsc::Sender<crate::ProxyCommand>,
        shutdown_command_tx: tokio::sync::mpsc::Sender<()>,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
        thread_id: usize,
        client_id: usize,
    ) -> Self {
        Self {
            bootstrap_proxy_endpoint,
            storage,
            proxy_command_tx,
            shutdown_command_tx,
            death_notification_tx,
            thread_id,
            client_id,
            num_active_subscriptions: Default::default(),
            num_total_subscriptions: Default::default(),
            num_active_messages: Default::default(),
        }
    }
}

impl bearclaw_capnp::bearclaw::Server for BearclawImpl {
    #[tracing::instrument(level = "trace", skip_all)]
    fn send(
        &mut self,
        params: bearclaw_capnp::bearclaw::SendParams,
        mut results: bearclaw_capnp::bearclaw::SendResults,
    ) -> Promise<(), capnp::Error> {
        let bootstrap_proxy_endpoint = self.bootstrap_proxy_endpoint.clone();

        Promise::from_future(async move {
            // Should these be checked outside the future?
            let params = params.get()?;
            let conn = params.get_conn_info()?;
            let host = conn.get_host()?;
            let port = conn.get_port();
            let is_https = conn.get_is_https();
            let request = params.get_request()?;

            tracing::trace!("Connecting to bootstrap proxy to forward a request from client");
            let mut forwarder =
                crate::bootstrap_proxy::Forwarder::connect(&bootstrap_proxy_endpoint)
                    .await
                    .map_err(|e| {
                        tracing::error!("Bootstrap proxy connection failed: {:?}", e);
                        capnp::Error::failed("Failed to forward request to proxy".to_owned())
                    })?;

            tracing::trace!("Forwarding request to bootstrap proxy");
            let response = forwarder
                .forward(host, port, is_https, request)
                .await
                .map_err(|e| {
                    tracing::error!("Bootstrap proxy forwarding failed: {:?}", e);
                    capnp::Error::failed("Failed to forward request to proxy".to_owned())
                })?;

            match response {
                Some(response) => results.get().init_response().set_some(&response)?,
                None => results.get().init_response().set_none(()),
            };

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn intercept(
        &mut self,
        params: bearclaw_capnp::bearclaw::InterceptParams,
        mut results: bearclaw_capnp::bearclaw::InterceptResults,
    ) -> Promise<(), capnp::Error> {
        if self.num_active_subscriptions.get() >= MAX_SUBSCRIPTIONS_PER_RPC_CONNECTION {
            tracing::warn!(
                "Max subscriptions ({}) exceeded for RPC connection resulting in RPC exception",
                MAX_SUBSCRIPTIONS_PER_RPC_CONNECTION
            );
            return Promise::err(capnp::Error::overloaded(format!(
                "Maximum number of subscriptions for this connection ({}) exceeded",
                MAX_SUBSCRIPTIONS_PER_RPC_CONNECTION
            )));
        }

        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() + 1);
        self.num_total_subscriptions += 1;

        tracing::trace!(num_active_subscriptions = self.num_active_subscriptions.get());

        let proxy_command_tx = self.proxy_command_tx.clone();
        let subscriber = pry!(pry!(params.get()).get_subscriber());
        let storage = self.storage.clone();
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_active_messages = self.num_active_messages.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let subscription_id = self.num_total_subscriptions;
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            proxy_command_tx
                .send(crate::ProxyCommand::Subscribe(reply_tx))
                .await
                .map_err(|e| {
                    tracing::error!("Send to proxy control channel failed: {:?}", e);
                    capnp::Error::failed("Unable to talk to proxy control channel".to_owned())
                })?;
            let interceptor_rx = reply_rx.await.map_err(|e| {
                tracing::error!("Receiving reply from proxy control channel failed: {:?}", e);
                capnp::Error::failed(
                    "Unable to receive reply from proxy control channel".to_owned(),
                )
            })?;
            let subscription = SubscriptionImpl::new(
                interceptor_rx,
                subscriber,
                storage,
                num_active_subscriptions,
                num_active_messages,
                thread_id,
                client_id,
                subscription_id,
                death_notification_tx,
            );
            results
                .get()
                .set_subscription(capnp_rpc::new_client(subscription));
            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn build_info(
        &mut self,
        _: bearclaw_capnp::bearclaw::BuildInfoParams,
        mut results: bearclaw_capnp::bearclaw::BuildInfoResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let dirty_build = git_version::git_version!().contains("-modified");
        let mut info = results.get().init_build_info();

        info.set_version(&format!(
            "{}{}",
            env!("VERGEN_BUILD_SEMVER"),
            if dirty_build { "-dirty" } else { "" }
        ));
        info.set_is_dirty(dirty_build);
        info.set_build_timestamp(env!("VERGEN_BUILD_TIMESTAMP"));

        match option_env!("VERGEN_GIT_BRANCH") {
            Some(x) => pry!(info.reborrow().init_git_branch().set_some(x)),
            None => info.reborrow().init_git_branch().set_none(()),
        }

        match option_env!("VERGEN_GIT_COMMIT_COUNT") {
            Some(x) => pry!(info.reborrow().init_git_commit_count().set_some(x)),
            None => info.reborrow().init_git_commit_count().set_none(()),
        }

        match option_env!("VERGEN_GIT_COMMIT_TIMESTAMP") {
            Some(x) => pry!(info.reborrow().init_git_commit_timestamp().set_some(x)),
            None => info.reborrow().init_git_commit_timestamp().set_none(()),
        }

        match option_env!("VERGEN_GIT_SHA") {
            Some(x) => pry!(info.reborrow().init_git_sha().set_some(x)),
            None => info.reborrow().init_git_sha().set_none(()),
        }

        info.set_rustc_channel(env!("VERGEN_RUSTC_CHANNEL"));
        info.set_rustc_commit_date(env!("VERGEN_RUSTC_COMMIT_DATE"));
        info.set_rustc_commit_hash(env!("VERGEN_RUSTC_COMMIT_HASH"));
        info.set_rustc_host_triple(env!("VERGEN_RUSTC_HOST_TRIPLE"));
        info.set_rustc_semver(env!("VERGEN_RUSTC_SEMVER"));
        info.set_cargo_features(env!("VERGEN_CARGO_FEATURES"));
        info.set_cargo_profile(env!("VERGEN_CARGO_PROFILE"));
        info.set_cargo_target_triple(env!("VERGEN_CARGO_TARGET_TRIPLE"));
        info.set_db_engine_version(&format!("sqlite {}", rusqlite::version()));

        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn exit(
        &mut self,
        _: bearclaw_capnp::bearclaw::ExitParams,
        _: bearclaw_capnp::bearclaw::ExitResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let shutdown_command_tx = self.shutdown_command_tx.clone();

        Promise::from_future(async move {
            shutdown_command_tx.send(()).await.map_err(|e| {
                tracing::error!("Send to shutdown command channel failed: {:?}", e);
                capnp::Error::failed("Unable to talk to shutdown command channel".to_owned())
            })?;
            Ok(())
        })
    }
}

struct SubscriptionImpl {
    num_active_subscriptions: Rc<Cell<usize>>,
    _terminator_tx: tokio::sync::oneshot::Sender<()>,
    subscription_id: usize,
}

impl SubscriptionImpl {
    fn new(
        mut interceptor_rx: tokio::sync::broadcast::Receiver<crate::storage::HistoryId>,
        subscriber: bearclaw_capnp::intercepted_message_subscriber::Client,
        storage: crate::storage::Channel,
        num_active_subscriptions: Rc<Cell<usize>>,
        num_active_messages: Rc<Cell<usize>>,
        thread_id: usize,
        client_id: usize,
        subscription_id: usize,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        let (terminator_tx, mut terminator_rx) = tokio::sync::oneshot::channel();

        tokio::task::Builder::new()
            .name(&format!("rpc-tid{}-cid{}-subscription{}", thread_id, client_id, subscription_id))
            .spawn_local(async move {
                let _death_notification_tx = death_notification_tx;
                loop {
                    tokio::select!(
                        history_id = interceptor_rx.recv() => {
                            if let Ok(history_id) = history_id {
                                if num_active_messages.get() >= MAX_MESSAGE_OBJECTS_PER_RPC_CONNECTION {
                                    tracing::warn!("Max message objects exceeded for RPC connection resulting in silently dropped subscription");
                                    return;
                                }
                                num_active_messages.set(num_active_messages.get() + 1);
                                let msg = storage.get_http_history(history_id).await.unwrap();
                                tracing::trace!(num_active_messages = num_active_messages.get());
                                tracing::trace!("Performing subscriber callback for intercepted message");
                                let mut request = subscriber.push_message_request();
                                request.get().set_message(capnp_rpc::new_client(InterceptedMessageImpl::new(msg, num_active_messages.clone())));
                                request.send().promise.await.unwrap();
                            } else {
                                tracing::trace!("Unable to receive intercepted message");
                                return;
                            }
                        }
                        _ = &mut terminator_rx => {
                            tracing::trace!("Terminated");
                            return;
                        }
                    );
                }
            })
            .unwrap();

        Self {
            num_active_subscriptions,
            _terminator_tx: terminator_tx,
            subscription_id,
        }
    }
}

impl Drop for SubscriptionImpl {
    fn drop(&mut self) {
        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() - 1);
        tracing::trace!(num_active_subscriptions = self.num_active_subscriptions.get());
        // the terminator receiver in our spawned task will get triggered when our sender side is
        // dropped, so there's no need to trigger it explicitly here
        tracing::trace!("subscription {} dropped by client", self.subscription_id);
    }
}

impl bearclaw_capnp::subscription::Server for SubscriptionImpl {}

struct InterceptedMessageImpl {
    msg: crate::storage::HttpMessage,
    num_active_messages: Rc<Cell<usize>>,
}

impl InterceptedMessageImpl {
    fn new(msg: crate::storage::HttpMessage, num_active_messages: Rc<Cell<usize>>) -> Self {
        Self {
            msg,
            num_active_messages,
        }
    }
}

impl bearclaw_capnp::intercepted_message::Server for InterceptedMessageImpl {
    #[tracing::instrument(level = "trace", skip_all)]
    fn connection_info(
        &mut self,
        _: bearclaw_capnp::intercepted_message::ConnectionInfoParams,
        mut results: bearclaw_capnp::intercepted_message::ConnectionInfoResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut conn = results.get().init_conn();
        conn.set_host(&self.msg.host);
        conn.set_port(self.msg.port);
        conn.set_is_https(self.msg.is_https);
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn request_timestamp(
        &mut self,
        _: bearclaw_capnp::intercepted_message::RequestTimestampParams,
        mut results: bearclaw_capnp::intercepted_message::RequestTimestampResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut time = results.get().init_time();
        time.set_secs(self.msg.request_time.timestamp());
        time.set_nsecs(self.msg.request_time.timestamp_subsec_nanos());
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn request_bytes(
        &mut self,
        _: bearclaw_capnp::intercepted_message::RequestBytesParams,
        mut results: bearclaw_capnp::intercepted_message::RequestBytesResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        results.get().set_request(&self.msg.request);
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn response_timestamp(
        &mut self,
        _: bearclaw_capnp::intercepted_message::ResponseTimestampParams,
        mut results: bearclaw_capnp::intercepted_message::ResponseTimestampResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut time = results.get().init_time();
        time.set_secs(self.msg.response_time.timestamp());
        time.set_nsecs(self.msg.response_time.timestamp_subsec_nanos());
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn response_bytes(
        &mut self,
        _: bearclaw_capnp::intercepted_message::ResponseBytesParams,
        mut results: bearclaw_capnp::intercepted_message::ResponseBytesResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut response = results.get().init_response();
        match &self.msg.response {
            Ok(m) => response.set_ok(m),
            Err(err) => match err {
                crate::storage::HttpError::Dns => response.set_err(bearclaw_capnp::HttpError::Dns),
                crate::storage::HttpError::CouldNotConnect => {
                    response.set_err(bearclaw_capnp::HttpError::CouldNotConnect)
                }
                crate::storage::HttpError::ConnectionClosed => {
                    response.set_err(bearclaw_capnp::HttpError::ConnectionClosed)
                }
                crate::storage::HttpError::ResponseTimeout => {
                    response.set_err(bearclaw_capnp::HttpError::ResponseTimeout)
                }
                crate::storage::HttpError::ResponseTooLarge => {
                    response.set_err(bearclaw_capnp::HttpError::ResponseTooLarge)
                }
            },
        }
        Promise::ok(())
    }
}

impl Drop for InterceptedMessageImpl {
    fn drop(&mut self) {
        self.num_active_messages
            .set(self.num_active_messages.get() - 1);
        tracing::trace!(num_active_messages = self.num_active_messages.get());
    }
}
