use std::{cell::Cell, rc::Rc, sync::Arc};

use capnp::capability::Promise;
use capnp_rpc::pry;

use crate::bearclaw_capnp;

/// Maximum number of searches each RPC connections may create.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_SEARCH_OBJECTS_PER_RPC_CONNECTION: usize = 128;

/// Maximum number of subscriptions (callbacks) to RPC connections that are allowed.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION: usize = 128;

/// Maximum number of message objects each RPC connections may create.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION: usize = 128;

/// Maximum number of search results that can be returned from a single invocation.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_SEARCH_RESULTS_RETURNED_PER_CALL: u32 = 128;

pub(super) struct BearclawImpl {
    bootstrap_proxy_endpoint: Arc<String>,
    storage: crate::storage::Channel,
    proxy_command_tx: tokio::sync::mpsc::Sender<crate::ProxyCommand>,
    shutdown_command_tx: tokio::sync::mpsc::Sender<()>,
    death_notification_tx: tokio::sync::mpsc::Sender<()>,
    thread_id: usize,
    client_id: usize,
    num_active_searches: Rc<Cell<usize>>,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: Rc<Cell<usize>>,
    num_active_history_items: Rc<Cell<usize>>,
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
            num_active_searches: Default::default(),
            num_active_subscriptions: Default::default(),
            num_total_subscriptions: Default::default(),
            num_active_history_items: Default::default(),
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
                    .unwrap();

            tracing::trace!("Forwarding request to bootstrap proxy");
            let response = forwarder
                .forward(host, port, is_https, request)
                .await
                .unwrap();

            match response {
                Some(response) => results.get().init_response().set_some(&response)?,
                None => results.get().init_response().set_none(()),
            };

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn search_history(
        &mut self,
        _params: bearclaw_capnp::bearclaw::SearchHistoryParams,
        mut results: bearclaw_capnp::bearclaw::SearchHistoryResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        if self.num_active_searches.get() >= MAX_SEARCH_OBJECTS_PER_RPC_CONNECTION {
            tracing::warn!(
                "Maximum search objects ({}) exceeded for RPC connection",
                MAX_SEARCH_OBJECTS_PER_RPC_CONNECTION
            );

            return Promise::err(capnp::Error::failed(format!(
                "Maximum search objects ({}) exceeded for RPC connection",
                MAX_SEARCH_OBJECTS_PER_RPC_CONNECTION
            )));
        }

        self.num_active_searches
            .set(self.num_active_searches.get() + 1);

        tracing::trace!(num_active_searches = self.num_active_searches.get());

        let storage = self.storage.clone();
        let proxy_command_tx = self.proxy_command_tx.clone();
        let num_active_searches = self.num_active_searches.clone();
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let history_search_id = storage.create_history_search().await.unwrap();

            let history_search = capnp_rpc::new_client(HistorySearchImpl {
                history_search_id,
                storage,
                proxy_command_tx,
                num_active_searches,
                num_active_subscriptions,
                num_total_subscriptions,
                thread_id,
                client_id,
                death_notification_tx,
            });
            results.get().set_history_search(history_search);

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_history_item(
        &mut self,
        params: bearclaw_capnp::bearclaw::GetHistoryItemParams,
        mut results: bearclaw_capnp::bearclaw::GetHistoryItemResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        if self.num_active_history_items.get() >= MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION {
            tracing::warn!(
                "Maximum history item objects ({}) exceeded for RPC connection",
                MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION
            );

            return Promise::err(capnp::Error::failed(format!(
                "Maximum history item objects ({}) exceeded for RPC connection",
                MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION
            )));
        }

        self.num_active_history_items
            .set(self.num_active_history_items.get() + 1);

        tracing::trace!(num_active_history_items = self.num_active_history_items.get());

        let history_id =
            crate::storage::HistoryId(pry!(pry!(params.get()).get_history_id()).get_id());
        let storage = self.storage.clone();
        let num_active_history_items = self.num_active_history_items.clone();

        Promise::from_future(async move {
            let msg = storage.get_http_history(history_id).await.unwrap();

            match msg {
                Ok(msg) => {
                    let msg =
                        capnp_rpc::new_client(HttpMessageImpl::new(msg, num_active_history_items));
                    results.get().init_result().set_ok(msg)?;
                }
                Err(crate::storage::LookupError::NotFound) => {
                    results.get().init_result().init_err().set_not_found(());
                }
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_build_info(
        &mut self,
        _: bearclaw_capnp::bearclaw::GetBuildInfoParams,
        mut results: bearclaw_capnp::bearclaw::GetBuildInfoResults,
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

        if option_env!("VERGEN_GIT_BRANCH").is_some() {
            let mut info = info.reborrow().init_git_info().init_some();

            info.set_branch(env!("VERGEN_GIT_BRANCH"));
            info.set_commit_count(env!("VERGEN_GIT_COMMIT_COUNT"));
            info.set_commit_timestamp(env!("VERGEN_GIT_COMMIT_TIMESTAMP"));
            info.set_sha(env!("VERGEN_GIT_SHA"));
        } else {
            info.reborrow().init_git_info().set_none(());
        }

        {
            let mut info = info.reborrow().init_rust_compiler_info();

            info.set_channel(env!("VERGEN_RUSTC_CHANNEL"));
            info.set_commit_date(env!("VERGEN_RUSTC_COMMIT_DATE"));
            info.set_commit_hash(env!("VERGEN_RUSTC_COMMIT_HASH"));
            info.set_host_triple(env!("VERGEN_RUSTC_HOST_TRIPLE"));
            info.set_semver(env!("VERGEN_RUSTC_SEMVER"));
        }

        {
            let mut info = info.reborrow().init_cargo_info();

            info.set_features(env!("VERGEN_CARGO_FEATURES"));
            info.set_profile(env!("VERGEN_CARGO_PROFILE"));
            info.set_target_triple(env!("VERGEN_CARGO_TARGET_TRIPLE"));
        }

        {
            let mut info = info.reborrow().init_library_info();

            info.set_db_engine_version(&format!("sqlite {}", rusqlite::version()));
            info.set_compression_engine_version(&format!(
                "zstd {}",
                zstd::zstd_safe::version_string()
            ));
        }

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
            shutdown_command_tx.send(()).await.unwrap();
            Ok(())
        })
    }
}

struct HistorySearchImpl {
    history_search_id: crate::storage::HistorySearchId,
    storage: crate::storage::Channel,
    proxy_command_tx: tokio::sync::mpsc::Sender<crate::ProxyCommand>,
    num_active_searches: Rc<Cell<usize>>,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: Rc<Cell<usize>>,
    thread_id: usize,
    client_id: usize,
    death_notification_tx: tokio::sync::mpsc::Sender<()>,
}

impl bearclaw_capnp::history_search::Server for HistorySearchImpl {
    #[tracing::instrument(level = "trace", skip_all)]
    fn get_count(
        &mut self,
        _: bearclaw_capnp::history_search::GetCountParams,
        mut results: bearclaw_capnp::history_search::GetCountResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let history_search_id = self.history_search_id;
        let storage = self.storage.clone();

        Promise::from_future(async move {
            let count = storage
                .get_history_search_count(history_search_id)
                .await
                .unwrap()
                .unwrap();

            results.get().set_count(count);

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_items(
        &mut self,
        params: bearclaw_capnp::history_search::GetItemsParams,
        mut results: bearclaw_capnp::history_search::GetItemsResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let history_search_id = self.history_search_id;
        let start_index = pry!(params.get()).get_start_index();
        let count = pry!(params.get())
            .get_count()
            .clamp(0, MAX_SEARCH_RESULTS_RETURNED_PER_CALL);
        let storage = self.storage.clone();

        Promise::from_future(async move {
            let items = storage
                .get_history_search_items(history_search_id, start_index, count)
                .await
                .unwrap()
                .unwrap();

            let mut out_items = results.get().init_items(items.len() as u32);
            // is there a better way to do this?
            let mut builder =
                capnp_rpc::ImbuedMessageBuilder::new(capnp::message::HeapAllocator::new());

            for (i, item) in items.iter().enumerate() {
                let mut out_item = builder.get_root::<bearclaw_capnp::history_id::Builder>()?;
                out_item.set_id(item.0);
                out_items.set_with_caveats(i as u32, out_item.into_reader())?;
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn subscribe(
        &mut self,
        params: bearclaw_capnp::history_search::SubscribeParams,
        mut results: bearclaw_capnp::history_search::SubscribeResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        if self.num_active_subscriptions.get() >= MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION {
            tracing::warn!(
                "Maximum subscription objects ({}) exceeded for RPC connection",
                MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
            );

            return Promise::err(capnp::Error::failed(format!(
                "Maximum subscription objects ({}) exceeded for RPC connection",
                MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
            )));
        }

        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() + 1);
        self.num_total_subscriptions
            .set(self.num_total_subscriptions.get() + 1);

        tracing::trace!(num_active_subscriptions = self.num_active_subscriptions.get());

        let proxy_command_tx = self.proxy_command_tx.clone();
        let subscriber = pry!(pry!(params.get()).get_subscriber());
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let subscription_id = self.num_total_subscriptions.get();
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
            proxy_command_tx
                .send(crate::ProxyCommand::Subscribe(reply_tx))
                .await
                .unwrap();
            let interceptor_rx = reply_rx.await.unwrap();
            let subscription = SubscriptionImpl::new(
                interceptor_rx,
                subscriber,
                num_active_subscriptions,
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
}

impl Drop for HistorySearchImpl {
    fn drop(&mut self) {
        self.num_active_searches
            .set(self.num_active_searches.get() - 1);
        tracing::trace!(num_active_searches = self.num_active_searches.get());
        tracing::trace!("search dropped by client");

        let tid = self.thread_id;
        let cid = self.client_id;
        let storage = self.storage.clone();
        let history_search_id = self.history_search_id;

        tokio::task::Builder::new()
            .name(&format!("rpc-tid{tid}-cid{cid}-history-search-destructor"))
            .spawn_local(async move {
                tracing::trace!("deleting dropped history search");
                storage
                    .delete_history_search(history_search_id)
                    .await
                    .unwrap()
                    .unwrap();
            })
            .unwrap();
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
        subscriber: bearclaw_capnp::history_subscriber::Client,
        num_active_subscriptions: Rc<Cell<usize>>,
        thread_id: usize,
        client_id: usize,
        subscription_id: usize,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
    ) -> Self {
        // This will shut down the spawned task if the client drops this object
        let (terminator_tx, mut terminator_rx) = tokio::sync::oneshot::channel();

        tokio::task::Builder::new()
            .name(&format!("rpc-tid{}-cid{}-subscription{}", thread_id, client_id, subscription_id))
            .spawn_local(async move {
                let _death_notification_tx = death_notification_tx;
                loop {
                    tokio::select!(
                        history_id = interceptor_rx.recv() => {
                            if let Err(tokio::sync::broadcast::error::RecvError::Closed) = history_id {
                                tracing::trace!("Proxy channel closed, shutting down");
                                return;
                            }
                            history_id.unwrap();
                            tracing::trace!("Performing subscriber callback for new proxy history item");
                            let request = subscriber.notify_new_item_request();
                            if let Err(e) = request.send().promise.await {
                                tracing::trace!("Unable to send callback to client: {e:?}");
                                return;
                            }
                        }
                        _ = &mut terminator_rx => {
                            tracing::trace!("Containing object dropped by client");
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

struct HttpMessageImpl {
    msg: crate::storage::HttpMessage,
    num_active_history_items: Rc<Cell<usize>>,
}

impl HttpMessageImpl {
    fn new(msg: crate::storage::HttpMessage, num_active_history_items: Rc<Cell<usize>>) -> Self {
        Self {
            msg,
            num_active_history_items,
        }
    }
}

impl bearclaw_capnp::http_message::Server for HttpMessageImpl {
    #[tracing::instrument(level = "trace", skip_all)]
    fn connection_info(
        &mut self,
        _: bearclaw_capnp::http_message::ConnectionInfoParams,
        mut results: bearclaw_capnp::http_message::ConnectionInfoResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut conn = results.get().init_connection_info();
        conn.set_host(&self.msg.host);
        conn.set_port(self.msg.port);
        conn.set_is_https(self.msg.is_https);
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn request_timestamp(
        &mut self,
        _: bearclaw_capnp::http_message::RequestTimestampParams,
        mut results: bearclaw_capnp::http_message::RequestTimestampResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut time = results.get().init_request_timestamp();
        time.set_secs(self.msg.request_time.timestamp());
        time.set_nsecs(self.msg.request_time.timestamp_subsec_nanos());
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn request_bytes(
        &mut self,
        _: bearclaw_capnp::http_message::RequestBytesParams,
        mut results: bearclaw_capnp::http_message::RequestBytesResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        results.get().set_request_bytes(&self.msg.request);
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn response_timestamp(
        &mut self,
        _: bearclaw_capnp::http_message::ResponseTimestampParams,
        mut results: bearclaw_capnp::http_message::ResponseTimestampResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut time = results.get().init_response_timestamp();
        time.set_secs(self.msg.response_time.timestamp());
        time.set_nsecs(self.msg.response_time.timestamp_subsec_nanos());
        Promise::ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn response_bytes(
        &mut self,
        _: bearclaw_capnp::http_message::ResponseBytesParams,
        mut results: bearclaw_capnp::http_message::ResponseBytesResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut response = results.get().init_response_bytes();
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

impl Drop for HttpMessageImpl {
    fn drop(&mut self) {
        self.num_active_history_items
            .set(self.num_active_history_items.get() - 1);
        tracing::trace!(num_active_history_items = self.num_active_history_items.get());
        tracing::trace!("HttpMessage dropped by client");
    }
}
