use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use capnp::capability::Promise;
use capnp_rpc::pry;
use chrono::TimeZone;

use crate::{bearclaw_capnp, storage};

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

/// Maximum number of message objects each RPC connections may create.
/// TODO: Value chosen arbitrarily. This should be configurable by the end user.
const MAX_SCENARIO_OBJECTS_PER_RPC_CONNECTION: usize = 128;

pub(super) struct BearclawImpl {
    bootstrap_proxy_endpoint: Arc<String>,
    storage: crate::storage::Channel,
    shutdown_command_tx: tokio::sync::mpsc::Sender<()>,
    shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
    death_notification_tx: tokio::sync::mpsc::Sender<()>,
    thread_id: usize,
    client_id: usize,
    num_active_searches: Rc<Cell<usize>>,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: Rc<Cell<usize>>,
    num_active_history_items: Rc<Cell<usize>>,
    num_active_scenarios: Rc<Cell<usize>>,
    scenario_server_set: Rc<
        RefCell<capnp_rpc::WeakCapabilityServerSet<ScenarioImpl, bearclaw_capnp::scenario::Client>>,
    >,
}

impl BearclawImpl {
    pub(super) fn new(
        bootstrap_proxy_endpoint: Arc<String>,
        storage: crate::storage::Channel,
        shutdown_command_tx: tokio::sync::mpsc::Sender<()>,
        shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
        death_notification_tx: tokio::sync::mpsc::Sender<()>,
        thread_id: usize,
        client_id: usize,
    ) -> Self {
        Self {
            bootstrap_proxy_endpoint,
            storage,
            shutdown_command_tx,
            shutdown_notification_rx,
            death_notification_tx,
            thread_id,
            client_id,
            num_active_searches: Default::default(),
            num_active_subscriptions: Default::default(),
            num_total_subscriptions: Default::default(),
            num_active_history_items: Default::default(),
            num_active_scenarios: Default::default(),
            scenario_server_set: Default::default(),
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

        tracing::debug!(num_active_searches = self.num_active_searches.get());

        let storage = self.storage.clone();
        let num_active_searches = self.num_active_searches.clone();
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let shutdown_notification_rx = self.shutdown_notification_rx.clone();
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let history_search_id = storage.create_history_search().await.unwrap();

            let history_search = capnp_rpc::new_client(HistorySearchImpl {
                history_search_id,
                storage,
                num_active_searches,
                num_active_subscriptions,
                num_total_subscriptions,
                thread_id,
                client_id,
                shutdown_notification_rx,
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
        let history_id =
            crate::storage::HistoryId(pry!(pry!(params.get()).get_history_id()).get_id());
        let storage = self.storage.clone();
        let num_active_history_items = self.num_active_history_items.clone();

        Promise::from_future(async move {
            let msg = storage.get_http_history(history_id).await.unwrap();

            match msg {
                Ok(msg) => {
                    if num_active_history_items.get() >= MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION {
                        tracing::warn!(
                            "Maximum history item objects ({}) exceeded for RPC connection",
                            MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION
                        );

                        return Err(capnp::Error::failed(format!(
                            "Maximum history item objects ({}) exceeded for RPC connection",
                            MAX_HISTORY_OBJECTS_PER_RPC_CONNECTION
                        )));
                    }

                    num_active_history_items.set(num_active_history_items.get() + 1);
                    tracing::debug!(num_active_history_items = num_active_history_items.get());

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
        let dirty_build = git_version::git_version!(fallback = "").contains("-modified");
        let mut info = results.get().init_build_info();

        info.set_version(&format!(
            "{}{}",
            env!("VERGEN_BUILD_SEMVER"),
            if dirty_build { "-dirty" } else { "" }
        ));
        info.set_is_dirty(dirty_build);
        info.set_build_timestamp(env!("VERGEN_BUILD_TIMESTAMP"));

        #[allow(clippy::option_env_unwrap)]
        if option_env!("VERGEN_GIT_BRANCH").is_some() {
            let mut info = info.reborrow().init_git_info().init_some();

            info.set_branch(option_env!("VERGEN_GIT_BRANCH").unwrap());
            info.set_commit_count(option_env!("VERGEN_GIT_COMMIT_COUNT").unwrap());
            info.set_commit_timestamp(option_env!("VERGEN_GIT_COMMIT_TIMESTAMP").unwrap());
            info.set_sha(option_env!("VERGEN_GIT_SHA").unwrap());
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

            info.set_db_engine(&format!("sqlite {}", rusqlite::version()));
            info.set_compression_engine(&format!("zstd {}", zstd::zstd_safe::version_string()));
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

    #[tracing::instrument(level = "trace", skip_all)]
    fn create_scenario(
        &mut self,
        params: bearclaw_capnp::bearclaw::CreateScenarioParams,
        mut results: bearclaw_capnp::bearclaw::CreateScenarioResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let info = pry!(pry!(params.get()).get_info());
        let parent = None;
        let user_defined_id = pry!(info.get_id()).to_string();
        let description = pry!(info.get_description()).to_string();
        let type_ = match pry!(info.get_type()) {
            bearclaw_capnp::ScenarioType::Container => crate::storage::ScenarioType::Container,
            bearclaw_capnp::ScenarioType::Generic => crate::storage::ScenarioType::Generic,
            bearclaw_capnp::ScenarioType::Location => crate::storage::ScenarioType::Location,
            bearclaw_capnp::ScenarioType::Endpoint => crate::storage::ScenarioType::Endpoint,
            bearclaw_capnp::ScenarioType::Authorization => {
                crate::storage::ScenarioType::Authorization
            }
            bearclaw_capnp::ScenarioType::BusinessLogic => {
                crate::storage::ScenarioType::BusinessLogic
            }
        };

        Promise::from_future(async move {
            let result = storage
                .create_scenario(parent, user_defined_id, description, type_)
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(crate::storage::CreateScenarioError::IdAlreadyExists) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_id_already_exists(()),
                Err(crate::storage::CreateScenarioError::ParentDoesNotExist) => {
                    panic!("Impossible error")
                }
                Err(crate::storage::CreateScenarioError::MaxScenarioTreeDepthExceeded) => {
                    panic!("Impossible error")
                }
                Err(crate::storage::CreateScenarioError::ScenarioLimitExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_limit_exceeded(()),
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_scenario(
        &mut self,
        params: bearclaw_capnp::bearclaw::GetScenarioParams,
        mut results: bearclaw_capnp::bearclaw::GetScenarioResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let user_defined_id = pry!(pry!(params.get()).get_scenario_id()).to_string();
        let storage = self.storage.clone();
        let num_active_scenarios = self.num_active_scenarios.clone();
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let shutdown_notification_rx = self.shutdown_notification_rx.clone();
        let death_notification_tx = self.death_notification_tx.clone();
        let server_set = self.scenario_server_set.clone();

        Promise::from_future(async move {
            let result = storage.lookup_scenario(user_defined_id).await.unwrap();

            match result {
                Ok(scenario_id) => {
                    if num_active_scenarios.get() >= MAX_SCENARIO_OBJECTS_PER_RPC_CONNECTION {
                        tracing::warn!(
                            "Maximum scenario objects ({}) exceeded for RPC connection",
                            MAX_SCENARIO_OBJECTS_PER_RPC_CONNECTION
                        );

                        return Err(capnp::Error::failed(format!(
                            "Maximum scenario objects ({}) exceeded for RPC connection",
                            MAX_SCENARIO_OBJECTS_PER_RPC_CONNECTION
                        )));
                    }

                    num_active_scenarios.set(num_active_scenarios.get() + 1);
                    tracing::debug!(num_active_scenarios = num_active_scenarios.get());

                    let scenario = server_set.borrow_mut().new_client(ScenarioImpl {
                        scenario_id,
                        storage,
                        server_set: server_set.clone(),
                        num_active_scenarios,
                        num_active_subscriptions,
                        num_total_subscriptions,
                        thread_id,
                        client_id,
                        shutdown_notification_rx,
                        death_notification_tx,
                    });

                    results.get().init_result().set_ok(scenario)?;
                }
                Err(storage::LookupError::NotFound) => {
                    results.get().init_result().init_err().set_not_found(());
                }
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn list_scenarios(
        &mut self,
        _: bearclaw_capnp::bearclaw::ListScenariosParams,
        mut results: bearclaw_capnp::bearclaw::ListScenariosResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();

        Promise::from_future(async move {
            let scenarios = storage.list_scenarios().await.unwrap();
            let mut list = results.get().init_list(scenarios.len() as u32);
            let mut builder =
                capnp_rpc::ImbuedMessageBuilder::new(capnp::message::HeapAllocator::new());

            for (i, scenario) in scenarios.iter().enumerate() {
                list.set_with_caveats(i as u32, serialize_scenario_node(&mut builder, scenario))?;
            }

            Ok(())
        })
    }

    fn subscribe_methodology(
        &mut self,
        params: bearclaw_capnp::bearclaw::SubscribeMethodologyParams,
        mut results: bearclaw_capnp::bearclaw::SubscribeMethodologyResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let subscriber = pry!(pry!(params.get()).get_subscriber());
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let shutdown_notification_rx = self.shutdown_notification_rx.clone();
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let methodology_update_rx = storage.subscribe_methodology().await.unwrap();

            if num_active_subscriptions.get() >= MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION {
                tracing::warn!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                );

                return Err(capnp::Error::failed(format!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                )));
            }

            let subscription_id = num_total_subscriptions.get();

            num_active_subscriptions.set(num_active_subscriptions.get() + 1);
            num_total_subscriptions.set(num_total_subscriptions.get() + 1);

            tracing::debug!(num_active_subscriptions = num_active_subscriptions.get());

            let subscription = MethodologySubscriptionImpl::new(
                methodology_update_rx,
                subscriber,
                num_active_subscriptions,
                thread_id,
                client_id,
                subscription_id,
                shutdown_notification_rx,
                death_notification_tx,
            );

            results
                .get()
                .set_subscription(capnp_rpc::new_client(subscription));

            Ok(())
        })
    }
}

impl Drop for BearclawImpl {
    fn drop(&mut self) {
        tracing::debug!("BearclawImpl dropped");
    }
}

struct HistorySearchImpl {
    history_search_id: crate::storage::HistorySearchId,
    storage: crate::storage::Channel,
    num_active_searches: Rc<Cell<usize>>,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: Rc<Cell<usize>>,
    thread_id: usize,
    client_id: usize,
    shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
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
        let storage = self.storage.clone();
        let history_search_id = self.history_search_id;
        let subscriber = pry!(pry!(params.get()).get_subscriber());
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let shutdown_notification_rx = self.shutdown_notification_rx.clone();
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let history_update_rx = storage
                .subscribe_history_search(history_search_id)
                .await
                .unwrap();

            if num_active_subscriptions.get() >= MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION {
                tracing::warn!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                );

                return Err(capnp::Error::failed(format!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                )));
            }

            let subscription_id = num_total_subscriptions.get();

            num_active_subscriptions.set(num_active_subscriptions.get() + 1);
            num_total_subscriptions.set(num_total_subscriptions.get() + 1);

            tracing::debug!(num_active_subscriptions = num_active_subscriptions.get());

            let subscription = HistorySubscriptionImpl::new(
                history_update_rx,
                subscriber,
                num_active_subscriptions,
                thread_id,
                client_id,
                subscription_id,
                shutdown_notification_rx,
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
        tracing::debug!(num_active_searches = self.num_active_searches.get());
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

struct HistorySubscriptionImpl {
    num_active_subscriptions: Rc<Cell<usize>>,
    _terminator_tx: tokio::sync::oneshot::Sender<()>,
    subscription_id: usize,
}

impl HistorySubscriptionImpl {
    fn new(
        mut history_update_rx: tokio::sync::watch::Receiver<()>,
        subscriber: bearclaw_capnp::history_subscriber::Client,
        num_active_subscriptions: Rc<Cell<usize>>,
        thread_id: usize,
        client_id: usize,
        subscription_id: usize,
        mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
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
                        result = history_update_rx.changed() => {
                            result.unwrap();

                            tracing::trace!("Performing subscriber callback for new proxy history item");
                            let request = subscriber.notify_new_item_request();

                            if let Err(e) = request.send().promise.await {
                                tracing::warn!("Unable to send history callback to client: {e:?}");
                                return;
                            }
                        }
                        _ = &mut terminator_rx => {
                            tracing::trace!("Containing object dropped by client");
                            return;
                        }
                        // TODO: I don't understand why this is necessary. If this isn't here
                        // neither the scenario nor the subscription gets dropped on exit.
                        _ = shutdown_notification_rx.changed() => {
                            tracing::trace!("Exiting due to application shutdown");
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

impl Drop for HistorySubscriptionImpl {
    fn drop(&mut self) {
        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() - 1);
        tracing::debug!(num_active_subscriptions = self.num_active_subscriptions.get());
        // the terminator receiver in our spawned task will get triggered when our sender side is
        // dropped, so there's no need to trigger it explicitly here
        tracing::trace!("subscription {} dropped by client", self.subscription_id);
    }
}

impl bearclaw_capnp::subscription::Server for HistorySubscriptionImpl {}

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
        tracing::debug!(num_active_history_items = self.num_active_history_items.get());
        tracing::trace!("HttpMessage dropped by client");
    }
}

struct ScenarioImpl {
    scenario_id: crate::storage::ScenarioId,
    storage: crate::storage::Channel,
    server_set: Rc<
        RefCell<capnp_rpc::WeakCapabilityServerSet<ScenarioImpl, bearclaw_capnp::scenario::Client>>,
    >,
    num_active_scenarios: Rc<Cell<usize>>,
    num_active_subscriptions: Rc<Cell<usize>>,
    num_total_subscriptions: Rc<Cell<usize>>,
    thread_id: usize,
    client_id: usize,
    shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
    death_notification_tx: tokio::sync::mpsc::Sender<()>,
}

impl bearclaw_capnp::scenario::Server for ScenarioImpl {
    #[tracing::instrument(level = "trace", skip_all)]
    fn get_info(
        &mut self,
        _params: bearclaw_capnp::scenario::GetInfoParams,
        mut results: bearclaw_capnp::scenario::GetInfoResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let scenario_id = self.scenario_id;

        Promise::from_future(async move {
            let result = storage.get_scenario_info(scenario_id).await.unwrap();

            match result {
                Ok(scenario_info) => {
                    serialize_scenario(&mut results.get().init_result().init_ok(), &scenario_info);
                }
                Err(storage::LookupError::NotFound) => results
                    .get()
                    .init_result()
                    .init_err()
                    .set_scenario_deleted(()),
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn update_info(
        &mut self,
        params: bearclaw_capnp::scenario::UpdateInfoParams,
        mut results: bearclaw_capnp::scenario::UpdateInfoResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let scenario_id = self.scenario_id;
        let info = pry!(pry!(params.get()).get_info());
        let user_defined_id = pry!(info.get_id()).to_string();
        let description = pry!(info.get_description()).to_string();
        let previous_modified_time = {
            let time = pry!(info.get_previous_modified_timestamp());
            let secs = time.get_secs();
            let nsecs = time.get_nsecs();

            match chrono::Local.timestamp_opt(secs, nsecs) {
                chrono::LocalResult::Single(s) => s,
                chrono::LocalResult::None | chrono::LocalResult::Ambiguous(_, _) => {
                    return Promise::err(capnp::Error::failed("Invalid Time".to_string()))
                }
            }
        };

        Promise::from_future(async move {
            let result = storage
                .update_scenario_info(
                    scenario_id,
                    user_defined_id,
                    description,
                    previous_modified_time,
                )
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(storage::UpdateScenarioInfoError::ScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_deleted(()),
                Err(storage::UpdateScenarioInfoError::ModifiedTimeDoesNotMatchDatabase) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_updated_by_someone_else(()),
                Err(storage::UpdateScenarioInfoError::UserDefinedIdAlreadyExists) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_id_already_exists(()),
            }

            Ok(())
        })
    }

    fn subscribe_scenario(
        &mut self,
        params: bearclaw_capnp::scenario::SubscribeScenarioParams,
        mut results: bearclaw_capnp::scenario::SubscribeScenarioResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let scenario_id = self.scenario_id;
        let subscriber = pry!(pry!(params.get()).get_subscriber());
        let num_active_subscriptions = self.num_active_subscriptions.clone();
        let num_total_subscriptions = self.num_total_subscriptions.clone();
        let thread_id = self.thread_id;
        let client_id = self.client_id;
        let shutdown_notification_rx = self.shutdown_notification_rx.clone();
        let death_notification_tx = self.death_notification_tx.clone();

        Promise::from_future(async move {
            let scenario_update_rx = storage.subscribe_scenario().await.unwrap();

            if num_active_subscriptions.get() >= MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION {
                tracing::warn!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                );

                return Err(capnp::Error::failed(format!(
                    "Maximum subscription objects ({}) exceeded for RPC connection",
                    MAX_SUBSCRIPTION_OBJECTS_PER_RPC_CONNECTION
                )));
            }

            let subscription_id = num_total_subscriptions.get();

            num_active_subscriptions.set(num_active_subscriptions.get() + 1);
            num_total_subscriptions.set(num_total_subscriptions.get() + 1);

            tracing::debug!(num_active_subscriptions = num_active_subscriptions.get());

            let subscription = ScenarioSubscriptionImpl::new(
                scenario_id,
                scenario_update_rx,
                subscriber,
                num_active_subscriptions,
                thread_id,
                client_id,
                subscription_id,
                shutdown_notification_rx,
                death_notification_tx,
            );

            results
                .get()
                .set_subscription(capnp_rpc::new_client(subscription));

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn create_child_scenario(
        &mut self,
        params: bearclaw_capnp::scenario::CreateChildScenarioParams,
        mut results: bearclaw_capnp::scenario::CreateChildScenarioResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let scenario_id = self.scenario_id;
        let info = pry!(pry!(params.get()).get_info());
        let parent = Some(scenario_id);
        let user_defined_id = pry!(info.get_id()).to_string();
        let description = pry!(info.get_description()).to_string();
        let type_ = match pry!(info.get_type()) {
            bearclaw_capnp::ScenarioType::Container => crate::storage::ScenarioType::Container,
            bearclaw_capnp::ScenarioType::Generic => crate::storage::ScenarioType::Generic,
            bearclaw_capnp::ScenarioType::Location => crate::storage::ScenarioType::Location,
            bearclaw_capnp::ScenarioType::Endpoint => crate::storage::ScenarioType::Endpoint,
            bearclaw_capnp::ScenarioType::Authorization => {
                crate::storage::ScenarioType::Authorization
            }
            bearclaw_capnp::ScenarioType::BusinessLogic => {
                crate::storage::ScenarioType::BusinessLogic
            }
        };

        Promise::from_future(async move {
            let result = storage
                .create_scenario(parent, user_defined_id, description, type_)
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(crate::storage::CreateScenarioError::IdAlreadyExists) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_id_already_exists(()),
                Err(crate::storage::CreateScenarioError::ParentDoesNotExist) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_deleted(()),
                Err(crate::storage::CreateScenarioError::MaxScenarioTreeDepthExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_max_scenario_tree_depth_exceeded(()),
                Err(crate::storage::CreateScenarioError::ScenarioLimitExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_limit_exceeded(()),
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn move_before(
        &mut self,
        params: bearclaw_capnp::scenario::MoveBeforeParams,
        mut results: bearclaw_capnp::scenario::MoveBeforeResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let server_set = self.server_set.clone();
        let move_scenario_id = self.scenario_id;
        let target = pry!(pry!(params.get()).get_before());

        Promise::from_future(async move {
            let target_scenario_id = {
                let target = capnp::capability::get_resolved_cap(target).await;
                let server_set = server_set.borrow();
                let scenario_impl = server_set.get_local_server_of_resolved(&target).ok_or(
                    capnp::Error::failed(
                        "The `before` object was not created by this server".to_string(),
                    ),
                )?;
                let target_scenario_id = scenario_impl.borrow().scenario_id;

                target_scenario_id
            };

            let result = storage
                .move_scenario_before(move_scenario_id, target_scenario_id)
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(storage::MoveScenarioError::MoveScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_this_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioIsChildOfMoveScenario) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_is_child_of_this_scenario(()),
                Err(storage::MoveScenarioError::MaxScenarioTreeDepthExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_max_scenario_tree_depth_exceeded(()),
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn move_after(
        &mut self,
        params: bearclaw_capnp::scenario::MoveAfterParams,
        mut results: bearclaw_capnp::scenario::MoveAfterResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let server_set = self.server_set.clone();
        let move_scenario_id = self.scenario_id;
        let target = pry!(pry!(params.get()).get_after());

        Promise::from_future(async move {
            let target_scenario_id = {
                //let target = params.get()?.get_after()?;
                let target = capnp::capability::get_resolved_cap(target).await;
                let server_set = server_set.borrow();
                let scenario_impl = server_set.get_local_server_of_resolved(&target).ok_or(
                    capnp::Error::failed(
                        "The `after` object was not created by this server".to_string(),
                    ),
                )?;
                let target_scenario_id = scenario_impl.borrow().scenario_id;

                target_scenario_id
            };

            let result = storage
                .move_scenario_after(move_scenario_id, target_scenario_id)
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(storage::MoveScenarioError::MoveScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_this_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioIsChildOfMoveScenario) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_is_child_of_this_scenario(()),
                Err(storage::MoveScenarioError::MaxScenarioTreeDepthExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_max_scenario_tree_depth_exceeded(()),
            }

            Ok(())
        })
    }

    fn move_inside(
        &mut self,
        params: bearclaw_capnp::scenario::MoveInsideParams,
        mut results: bearclaw_capnp::scenario::MoveInsideResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let server_set = self.server_set.clone();
        let move_scenario_id = self.scenario_id;
        let target = pry!(pry!(params.get()).get_parent());

        Promise::from_future(async move {
            let target_scenario_id = {
                //let target = params.get()?.get_after()?;
                let target = capnp::capability::get_resolved_cap(target).await;
                let server_set = server_set.borrow();
                let scenario_impl = server_set.get_local_server_of_resolved(&target).ok_or(
                    capnp::Error::failed(
                        "The `parent` object was not created by this server".to_string(),
                    ),
                )?;
                let target_scenario_id = scenario_impl.borrow().scenario_id;

                target_scenario_id
            };

            let result = storage
                .move_scenario_inside(move_scenario_id, target_scenario_id)
                .await
                .unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(storage::MoveScenarioError::MoveScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_this_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_deleted(()),
                Err(storage::MoveScenarioError::TargetScenarioIsChildOfMoveScenario) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_target_scenario_is_child_of_this_scenario(()),
                Err(storage::MoveScenarioError::MaxScenarioTreeDepthExceeded) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_max_scenario_tree_depth_exceeded(()),
            }

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn delete(
        &mut self,
        _params: bearclaw_capnp::scenario::DeleteParams,
        mut results: bearclaw_capnp::scenario::DeleteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let storage = self.storage.clone();
        let scenario_id = self.scenario_id;

        Promise::from_future(async move {
            let result = storage.delete_scenario(scenario_id).await.unwrap();

            match result {
                Ok(_) => results.get().init_result().set_success(()),
                Err(storage::DeleteScenarioError::ScenarioNotFound) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .set_scenario_already_deleted(()),
                Err(storage::DeleteScenarioError::ScenarioHasTestingData(s)) => results
                    .get()
                    .init_result()
                    .init_fail()
                    .init_scenario_has_testing_data()
                    .set_scenario_id(&s),
            }

            Ok(())
        })
    }
}

impl Drop for ScenarioImpl {
    fn drop(&mut self) {
        self.num_active_scenarios
            .set(self.num_active_scenarios.get() - 1);
        tracing::debug!(num_active_scenarios = self.num_active_scenarios.get());
        tracing::trace!("Scenario dropped by client");

        self.server_set.borrow_mut().gc();
    }
}

// Am I using the ImbuedMessageBuilder correctly?
fn serialize_scenario_node<'a>(
    builder: &'a mut capnp_rpc::ImbuedMessageBuilder<capnp::message::HeapAllocator>,
    node: &'a crate::storage::ScenarioTreeNode,
) -> bearclaw_capnp::scenario_tree_node::Reader<'a> {
    let mut out = builder
        .get_root::<bearclaw_capnp::scenario_tree_node::Builder>()
        .unwrap();

    serialize_scenario(&mut out.reborrow().init_info(), &node.info);

    let mut children = out.reborrow().init_children(node.children.len() as u32);
    let mut child_builder =
        capnp_rpc::ImbuedMessageBuilder::new(capnp::message::HeapAllocator::new());

    for (i, child) in node.children.iter().enumerate() {
        children
            .set_with_caveats(i as u32, serialize_scenario_node(&mut child_builder, child))
            .unwrap();
    }

    out.into_reader()
}

fn serialize_scenario(
    builder: &mut bearclaw_capnp::scenario_info::Builder,
    info: &storage::ScenarioInfo,
) {
    builder.set_id(&info.user_defined_id);
    builder.set_description(&info.description);
    builder.set_type(match info.type_ {
        crate::storage::ScenarioType::Container => bearclaw_capnp::ScenarioType::Container,
        crate::storage::ScenarioType::Generic => bearclaw_capnp::ScenarioType::Generic,
        crate::storage::ScenarioType::Location => bearclaw_capnp::ScenarioType::Location,
        crate::storage::ScenarioType::Endpoint => bearclaw_capnp::ScenarioType::Endpoint,
        crate::storage::ScenarioType::Authorization => bearclaw_capnp::ScenarioType::Authorization,
        crate::storage::ScenarioType::BusinessLogic => bearclaw_capnp::ScenarioType::BusinessLogic,
    });

    let mut time = builder.reborrow().init_created_timestamp();
    time.set_secs(info.created_time.timestamp());
    time.set_nsecs(info.created_time.timestamp_subsec_nanos());

    let mut time = builder.reborrow().init_modified_timestamp();
    time.set_secs(info.modified_time.timestamp());
    time.set_nsecs(info.modified_time.timestamp_subsec_nanos());
}

struct ScenarioSubscriptionImpl {
    num_active_subscriptions: Rc<Cell<usize>>,
    _terminator_tx: tokio::sync::oneshot::Sender<()>,
    subscription_id: usize,
}

impl ScenarioSubscriptionImpl {
    fn new(
        scenario_id: crate::storage::ScenarioId,
        mut scenario_update_rx: tokio::sync::broadcast::Receiver<crate::storage::ScenarioId>,
        subscriber: bearclaw_capnp::scenario_subscriber::Client,
        num_active_subscriptions: Rc<Cell<usize>>,
        thread_id: usize,
        client_id: usize,
        subscription_id: usize,
        mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
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
                        notify_scenario_id = scenario_update_rx.recv() => {
                            if let Err(tokio::sync::broadcast::error::RecvError::Lagged(amount)) = notify_scenario_id {
                                tracing::warn!("Scenario subscription lagged and missed {amount} messages");
                                continue;
                            }

                            let notify_scenario_id = notify_scenario_id.unwrap();

                            if notify_scenario_id == scenario_id {
                                let request = subscriber.notify_scenario_updated_request();

                                tracing::trace!("Performing subscriber callback for scenario update");

                                if let Err(e) = request.send().promise.await {
                                    tracing::warn!("Unable to send scenario update callback to client: {e:?}");
                                    // Stop trying to send further callbacks
                                    return;
                                }
                            }
                        }
                        _ = &mut terminator_rx => {
                            tracing::trace!("Containing object dropped by client");
                            return;
                        }
                        // TODO: I don't understand why this is necessary. If this isn't here
                        // neither the scenario nor the subscription gets dropped on exit.
                        _ = shutdown_notification_rx.changed() => {
                            tracing::trace!("Exiting due to application shutdown");
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

impl Drop for ScenarioSubscriptionImpl {
    fn drop(&mut self) {
        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() - 1);
        tracing::debug!(num_active_subscriptions = self.num_active_subscriptions.get());
        // the terminator receiver in our spawned task will get triggered when our sender side is
        // dropped, so there's no need to trigger it explicitly here
        tracing::trace!("subscription {} dropped by client", self.subscription_id);
    }
}

impl bearclaw_capnp::subscription::Server for ScenarioSubscriptionImpl {}

struct MethodologySubscriptionImpl {
    num_active_subscriptions: Rc<Cell<usize>>,
    _terminator_tx: tokio::sync::oneshot::Sender<()>,
    subscription_id: usize,
}

impl MethodologySubscriptionImpl {
    fn new(
        mut methodology_update_rx: tokio::sync::watch::Receiver<()>,
        subscriber: bearclaw_capnp::methodology_subscriber::Client,
        num_active_subscriptions: Rc<Cell<usize>>,
        thread_id: usize,
        client_id: usize,
        subscription_id: usize,
        mut shutdown_notification_rx: tokio::sync::watch::Receiver<()>,
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
                        result = methodology_update_rx.changed() => {
                            result.unwrap();

                            let request = subscriber.notify_scenario_tree_changed_request();

                            tracing::trace!("Performing subscriber callback for methodology update");

                            if let Err(e) = request.send().promise.await {
                                tracing::warn!("Unable to send methodology update callback to client: {e:?}");
                                // Stop trying to send further callbacks
                                return;
                            }
                        }
                        _ = &mut terminator_rx => {
                            tracing::trace!("Containing object dropped by client");
                            return;
                        }
                        // TODO: I don't understand why this is necessary. If this isn't here
                        // neither the scenario nor the subscription gets dropped on exit.
                        _ = shutdown_notification_rx.changed() => {
                            tracing::trace!("Exiting due to application shutdown");
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

impl Drop for MethodologySubscriptionImpl {
    fn drop(&mut self) {
        self.num_active_subscriptions
            .set(self.num_active_subscriptions.get() - 1);
        tracing::debug!(num_active_subscriptions = self.num_active_subscriptions.get());
        // the terminator receiver in our spawned task will get triggered when our sender side is
        // dropped, so there's no need to trigger it explicitly here
        tracing::trace!("subscription {} dropped by client", self.subscription_id);
    }
}

impl bearclaw_capnp::subscription::Server for MethodologySubscriptionImpl {}
