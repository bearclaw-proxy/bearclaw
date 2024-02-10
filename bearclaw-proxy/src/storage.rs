use std::path::Path;

use bytes::{Buf, BufMut};
use chrono::TimeZone;
use rusqlite::OptionalExtension;

/// Lets us identify if we created the file
const APPLICATION_ID: u32 = 0xb34c14;
/// Updated whenever our schema changes. Will be used to know when to migrate database versions.
const FILE_FORMAT_VERSION: i64 = 2;
// Let's start with the highest compression level, we can scale it back later if it causes problems
const ZSTD_COMPRESSION_LEVEL: i32 = 22;
// NOTE: Check to see if you should update this if you add any SQL statements
const PREPARED_STATEMENT_CACHE_CAPACITY: usize = 64;
const LOAD_HTTP_REQUEST_INITIAL_BUFFER_CAPACITY: usize = 512;
const LOAD_HTTP_RESPONSE_INITIAL_BUFFER_CAPACITY: usize = 1024;
// TODO: This should be configurable
const MAX_SCENARIOS: u32 = 256;
// TODO: This limit should be stored in the database
const MAX_SCENARIO_TREE_DEPTH: u8 = 16;
/// Maximum number of messages that can be queued in the various broadcast notification channels.
/// If a receiver cannot process messages fast enough it will receive an error and fail.
/// TODO: Value chosen arbitrarily.
const BROADCAST_CHANNEL_CAPACITY: usize = 128;

pub(super) struct Database {
    db: rusqlite::Connection,
    storage_stats: StorageStats,
    history_update_tx: tokio::sync::watch::Sender<()>,
    methodology_update_tx: tokio::sync::watch::Sender<()>,
    scenario_update_tx: tokio::sync::broadcast::Sender<ScenarioId>,
}

impl Database {
    /// Creates a new database or opens and validates an existing one
    pub(super) fn open_or_create<P: AsRef<Path>>(path: &P) -> OpenResult<Self> {
        let db = {
            tracing::trace!("attempting to open an existing database file");
            // Try to open an existing file by not specifying the create flag
            if let Ok(db) = rusqlite::Connection::open_with_flags(
                path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_URI
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                tracing::trace!("existing database opened, checking its validity");
                check_valid(&db)?;
                init(&db)?;

                db
            } else {
                // If that didn't work, use the default set of flags which tries to create the file
                let mut db = rusqlite::Connection::open(path)?;

                tracing::trace!("open failed");
                tracing::trace!("attempting to create a new database");
                init(&db)?;
                create(&mut db)?;

                db
            }
        };

        let (history_update_tx, _) = tokio::sync::watch::channel(());
        let (methodology_update_tx, _) = tokio::sync::watch::channel(());
        let (scenario_update_tx, _) = tokio::sync::broadcast::channel(BROADCAST_CHANNEL_CAPACITY);

        Ok(Self {
            db,
            storage_stats: Default::default(),
            history_update_tx,
            methodology_update_tx,
            scenario_update_tx,
        })
    }

    /// The storage message processing loop. It is intended to run in its own thread.
    pub(super) fn run(mut self, mut storage_rx: tokio::sync::mpsc::Receiver<Command>) {
        // This will exit when all senders are destroyed
        while let Some(cmd) = storage_rx.blocking_recv() {
            match cmd {
                Command::StoreHttpHistory {
                    test_id,
                    http_message,
                    reply,
                } => {
                    let history_id = self.store_http_history(test_id, http_message);
                    reply.send(HistoryId(history_id)).unwrap();
                }
                Command::GetHttpHistory { history_id, reply } => {
                    let http_message = self.get_http_history(history_id);
                    reply.send(http_message).unwrap();
                }
                Command::CreateHistorySearch { reply } => {
                    let result = self.create_history_search();
                    reply.send(result).unwrap();
                }
                Command::GetHistorySearchItems {
                    history_search_id,
                    start_index,
                    count,
                    reply,
                } => {
                    let result =
                        self.get_history_search_items(history_search_id, start_index, count);
                    reply.send(result).unwrap();
                }
                Command::GetHistorySearchCount {
                    history_search_id,
                    reply,
                } => {
                    let result = self.get_history_search_count(history_search_id);
                    reply.send(result).unwrap();
                }
                Command::SubscribeHistorySearch {
                    history_search_id,
                    reply,
                } => {
                    let result = self.subscribe_history_search(history_search_id);
                    reply.send(result).unwrap();
                }
                Command::DeleteHistorySearch {
                    history_search_id,
                    reply,
                } => {
                    let result = self.delete_history_search(history_search_id);
                    reply.send(result).unwrap();
                }
                Command::ListScenarios { reply } => {
                    let result = self.list_scenarios();
                    reply.send(result).unwrap();
                }
                Command::CreateScenario {
                    parent,
                    user_defined_id,
                    description,
                    type_,
                    reply,
                } => {
                    let result = self.create_scenario(parent, user_defined_id, description, type_);
                    reply.send(result).unwrap();
                }
                Command::LookupScenario {
                    user_defined_id,
                    reply,
                } => {
                    let result = self.lookup_scenario(user_defined_id);
                    reply.send(result).unwrap();
                }
                Command::MoveScenarioBefore {
                    move_scenario_id,
                    before_scenario_id,
                    reply,
                } => {
                    let result = self.move_scenario_before(move_scenario_id, before_scenario_id);
                    reply.send(result).unwrap();
                }
                Command::MoveScenarioInside {
                    move_scenario_id,
                    parent_scenario_id,
                    reply,
                } => {
                    let result = self.move_scenario_inside(move_scenario_id, parent_scenario_id);
                    reply.send(result).unwrap();
                }
                Command::MoveScenarioAfter {
                    move_scenario_id,
                    after_scenario_id,
                    reply,
                } => {
                    let result = self.move_scenario_after(move_scenario_id, after_scenario_id);
                    reply.send(result).unwrap();
                }
                Command::DeleteScenario { scenario_id, reply } => {
                    let result = self.delete_scenario(scenario_id);
                    reply.send(result).unwrap();
                }
                Command::GetScenarioInfo { scenario_id, reply } => {
                    let result = self.get_scenario_info(scenario_id);
                    reply.send(result).unwrap();
                }
                Command::UpdateScenarioInfo {
                    scenario_id,
                    user_defined_id,
                    description,
                    previous_modified_time,
                    reply,
                } => {
                    let result = self.update_scenario_info(
                        scenario_id,
                        user_defined_id,
                        description,
                        previous_modified_time,
                    );
                    reply.send(result).unwrap();
                }
                Command::SubscribeScenario { reply } => {
                    let result = self.subscribe_scenario();
                    reply.send(result).unwrap();
                }
                Command::SubscribeMethodology { reply } => {
                    let result = self.subscribe_methodology();
                    reply.send(result).unwrap();
                }
            }
        }

        if self.storage_stats.bytes_stored > 0 {
            let bytes_compressed = self.storage_stats.bytes_total
                - self.storage_stats.bytes_deduplicated
                - self.storage_stats.bytes_stored;
            let percentage_stored_of_total = (self.storage_stats.bytes_stored as f64
                / self.storage_stats.bytes_total as f64)
                * 100.0;

            tracing::debug!(self.storage_stats.bytes_total);
            tracing::debug!(self.storage_stats.bytes_stored);
            tracing::debug!(self.storage_stats.bytes_deduplicated);
            tracing::debug!(bytes_compressed);
            tracing::debug!("Percentage bytes stored of total: {percentage_stored_of_total:.0}%");
        }
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn store_http_history(&mut self, test_id: Option<TestId>, http_message: HttpMessage) -> i64 {
        let txn = self.db.transaction().unwrap();
        let mut storage_stats = Default::default();

        tracing::trace!("Creating history record");
        let history_id = {
            let mut insert = txn
                .prepare_cached("INSERT INTO history (date_secs, date_nsecs) VALUES (?,?)")
                .unwrap();
            insert
                .execute((
                    http_message.request_time.timestamp(),
                    http_message.request_time.timestamp_subsec_nanos(),
                ))
                .unwrap();

            txn.last_insert_rowid()
        };

        tracing::trace!("Creating test_history record");
        {
            // Test ID 1 is guaranteed to exist, it holds proxied requests
            let test_id = test_id.unwrap_or(TestId(1)).0;
            let mut insert = txn
                .prepare_cached("INSERT INTO test_history (test_id, history_id) VALUES (?,?)")
                .unwrap();
            insert.execute((test_id, history_id)).unwrap();
        }

        let (request_location_id, request_header_id, opt_request_body_id) =
            store_http_request(&txn, &http_message.request, &mut storage_stats);

        let (response_header_id, response_body_id) =
            if let Ok(http_response) = &http_message.response {
                store_http_response(&txn, http_response, &mut storage_stats)
            } else {
                let empty_content = store_content(&txn, b"", &mut storage_stats);

                (empty_content, empty_content)
            };

        let connection_info_id = {
            let host_id = {
                let mut query = txn
                    .prepare_cached("SELECT host_id FROM host WHERE host = ?")
                    .unwrap();

                if let Some(host_id) = query
                    .query_row((&http_message.host,), |row| row.get(0))
                    .optional()
                    .unwrap()
                {
                    tracing::trace!("Using existing host_id record");

                    host_id
                } else {
                    tracing::trace!("Creating host_id record");
                    let mut insert = txn
                        .prepare_cached("INSERT INTO host (host) VALUES (?)")
                        .unwrap();

                    insert.execute((&http_message.host,)).unwrap();

                    txn.last_insert_rowid()
                }
            };

            let mut query = txn
                .prepare_cached(
                    "
                    SELECT connection_info_id
                    FROM connection_info
                    WHERE host_id = ? AND port = ? AND is_https = ?
                    ",
                )
                .unwrap();

            if let Some(connection_info_id) = query
                .query_row((host_id, http_message.port, http_message.is_https), |row| {
                    row.get(0)
                })
                .optional()
                .unwrap()
            {
                tracing::trace!("Using existing connection_info record");

                connection_info_id
            } else {
                tracing::trace!("Creating connection_info record");
                let mut insert = txn
                    .prepare_cached(
                        "
                        INSERT INTO
                            connection_info
                        (host_id, port, is_https)
                        VALUES
                            (?,?,?)
                        ",
                    )
                    .unwrap();

                insert
                    .execute((host_id, http_message.port, http_message.is_https))
                    .unwrap();

                txn.last_insert_rowid()
            }
        };

        {
            tracing::trace!("Creating http_history record");
            let mut insert = txn
                .prepare_cached(
                    "
                    INSERT INTO
                        http_history
                    (
                        http_history_id,
                        connection_info_id,
                        request_location_id,
                        request_header_id,
                        response_date_secs,
                        response_date_nsecs,
                        response_header_id,
                        response_body_id
                    )
                    VALUES
                        (?,?,?,?,?,?,?,?)
                    ",
                )
                .unwrap();

            insert
                .execute((
                    history_id,
                    connection_info_id,
                    request_location_id,
                    request_header_id,
                    http_message.response_time.timestamp(),
                    http_message.response_time.timestamp_subsec_nanos(),
                    response_header_id,
                    response_body_id,
                ))
                .unwrap();
        }

        if let Some(request_body_id) = opt_request_body_id {
            tracing::trace!("Creating http_history_request_body record");
            let mut insert = txn
                .prepare_cached(
                    "
                    INSERT INTO
                        http_history_request_body
                    (
                        http_history_id,
                        request_body_id
                    )
                    VALUES
                        (?,?)
                    ",
                )
                .unwrap();

            insert.execute((history_id, request_body_id)).unwrap();
        }

        if let Err(error) = http_message.response {
            tracing::trace!("Creating http_error record");
            let mut insert = txn
                .prepare_cached(
                    "INSERT INTO http_error (http_history_id, http_error_enum_id) VALUES (?,?)",
                )
                .unwrap();
            let error_code: u8 = error.into();

            insert.execute((history_id, error_code)).unwrap();
        }

        txn.commit().unwrap();

        // It's ok for this to return an error if there are no subscribers
        let _ = self.history_update_tx.send(());

        tracing::debug!(storage_stats.bytes_total);
        tracing::debug!(storage_stats.bytes_deduplicated);
        tracing::debug!(storage_stats.bytes_stored);

        self.storage_stats.bytes_total += storage_stats.bytes_total;
        self.storage_stats.bytes_deduplicated += storage_stats.bytes_deduplicated;
        self.storage_stats.bytes_stored += storage_stats.bytes_stored;

        history_id
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_http_history(&mut self, http_history_id: HistoryId) -> LookupResult<HttpMessage> {
        let txn = self.db.transaction().unwrap();
        let (
            host,
            port,
            is_https,
            request_date_secs,
            request_date_nsecs,
            request_location_id,
            request_header_id,
            opt_request_body_id,
            response_date_secs,
            response_date_nsecs,
            response_header_id,
            response_body_id,
            opt_http_error_enum_id,
        ) = {
            let mut query = txn
                .prepare_cached(
                    "
                    SELECT
                        host.host,
                        connection_info.port,
                        connection_info.is_https,
                        history.date_secs,
                        history.date_nsecs,
                        http_history.request_location_id,
                        http_history.request_header_id,
                        http_history_request_body.request_body_id,
                        http_history.response_date_secs,
                        http_history.response_date_nsecs,
                        http_history.response_header_id,
                        http_history.response_body_id,
                        http_error.http_error_enum_id
                    FROM
                        http_history
                    INNER JOIN
                        history
                    ON
                        http_history.http_history_id = history.history_id
                    INNER JOIN
                        connection_info
                    ON
                        http_history.connection_info_id = connection_info.connection_info_id
                    INNER JOIN
                        host
                    ON
                        connection_info.host_id = host.host_id
                    LEFT JOIN
                        http_history_request_body
                    ON
                        http_history.http_history_id = http_history_request_body.http_history_id
                    LEFT JOIN
                        http_error
                    ON
                        http_history.http_history_id = http_error.http_history_id
                    WHERE
                        http_history.http_history_id = ?
                    ",
                )
                .unwrap();

            if let Some(result) = query
                .query_row((http_history_id.0,), |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get::<usize, Option<u8>>(12)?,
                    ))
                })
                .optional()
                .unwrap()
            {
                result
            } else {
                return Err(LookupError::NotFound);
            }
        };

        let request = load_http_request(
            &txn,
            request_location_id,
            request_header_id,
            opt_request_body_id,
        );
        let response = if let Some(http_error_enum_id) = opt_http_error_enum_id {
            let err = HttpError::try_from(http_error_enum_id).unwrap();
            Err(err)
        } else {
            Ok(load_http_response(
                &txn,
                response_header_id,
                response_body_id,
            ))
        };

        txn.commit().unwrap();

        Ok(HttpMessage {
            request_time: chrono::Local
                .timestamp_opt(request_date_secs, request_date_nsecs)
                .unwrap(),
            response_time: chrono::Local
                .timestamp_opt(response_date_secs, response_date_nsecs)
                .unwrap(),
            host,
            port,
            is_https,
            request,
            response,
        })
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn create_history_search(&mut self) -> HistorySearchId {
        HistorySearchId
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_history_search_count(
        &mut self,
        _history_search_id: HistorySearchId,
    ) -> LookupResult<u32> {
        let txn = self.db.transaction().unwrap();
        let result = {
            let mut query = txn
                .prepare_cached("SELECT COUNT(history_id) FROM history")
                .unwrap();

            query.query_row([], |row| row.get(0)).unwrap()
        };

        txn.commit().unwrap();

        Ok(result)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_history_search_items(
        &mut self,
        _history_search_id: HistorySearchId,
        start_index: u32,
        count: u32,
    ) -> LookupResult<Vec<HistoryId>> {
        let txn = self.db.transaction().unwrap();
        let result = {
            let mut result = Vec::with_capacity(count as usize);
            let mut query = txn
                .prepare_cached(
                    "SELECT history_id FROM history ORDER BY history_id LIMIT ? OFFSET ?",
                )
                .unwrap();
            let query = query
                .query_map((count, start_index), |row| Ok(HistoryId(row.get(0)?)))
                .unwrap();

            for item in query {
                result.push(item.unwrap())
            }

            result
        };

        txn.commit().unwrap();

        Ok(result)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn subscribe_history_search(
        &self,
        _history_search_id: HistorySearchId,
    ) -> tokio::sync::watch::Receiver<()> {
        self.history_update_tx.subscribe()
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn delete_history_search(&mut self, _history_search_id: HistorySearchId) -> LookupResult<()> {
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn list_scenarios(&mut self) -> Vec<ScenarioTreeNode> {
        let txn = self.db.transaction().unwrap();
        let parent_id = None;
        let recursion_depth = 0;
        let result = list_scenario_children(&txn, parent_id, recursion_depth);

        txn.commit().unwrap();

        result
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn create_scenario(
        &mut self,
        parent_id: Option<ScenarioId>,
        user_defined_id: String,
        description: String,
        type_: ScenarioType,
    ) -> Result<ScenarioId, CreateScenarioError> {
        let txn = self.db.transaction().unwrap();
        let parent_id = parent_id.map(|x| x.0);

        let num_scenarios: u32 = {
            let mut query = txn
                .prepare_cached("SELECT COUNT(scenario_id) FROM scenario")
                .unwrap();

            query.query_row([], |row| row.get(0)).unwrap()
        };

        if num_scenarios >= MAX_SCENARIOS {
            return Err(CreateScenarioError::ScenarioLimitExceeded);
        }

        if let Some(parent_id) = parent_id {
            // Check that the parent exists and that the new child will not exceed the recursion
            // limit
            if let Some(depth) = get_scenario_depth(&txn, parent_id) {
                if depth + 1 >= MAX_SCENARIO_TREE_DEPTH {
                    return Err(CreateScenarioError::MaxScenarioTreeDepthExceeded);
                }
            } else {
                return Err(CreateScenarioError::ParentDoesNotExist);
            }
        }

        {
            let mut query = txn
                .prepare_cached("SELECT scenario_id FROM scenario WHERE user_defined_id = ?")
                .unwrap();

            if query
                .query_row((&user_defined_id,), |_| Ok(()))
                .optional()
                .unwrap()
                .is_some()
            {
                return Err(CreateScenarioError::IdAlreadyExists);
            }
        }

        let next_position = {
            if parent_id.is_some() {
                let mut query = txn
                    .prepare_cached("SELECT MAX(position) FROM scenario WHERE parent_id = ?")
                    .unwrap();

                if let Some(position) = query
                    .query_row((parent_id,), |row| Ok(row.get::<usize, Option<u32>>(0)))
                    .unwrap()
                    .unwrap()
                {
                    position + 1
                } else {
                    1
                }
            } else {
                let mut query = txn
                    .prepare_cached("SELECT MAX(position) FROM scenario WHERE parent_id IS NULL")
                    .unwrap();

                if let Some(position) = query
                    .query_row([], |row| Ok(row.get::<usize, Option<u32>>(0)))
                    .unwrap()
                    .unwrap()
                {
                    position + 1
                } else {
                    1
                }
            }
        };

        let scenario_id = {
            let scenario_type_enum_id: u8 = type_.into();
            let now = chrono::offset::Local::now();
            let mut insert = txn
                .prepare_cached(
                    "
                    INSERT INTO scenario (
                        parent_id,
                        position,
                        scenario_type_enum_id,
                        user_defined_id,
                        description,
                        date_created,
                        date_modified_secs,
                        date_modified_nsecs
                    ) VALUES (?,?,?,?,?,?,?,?)
                    ",
                )
                .unwrap();

            insert
                .execute((
                    parent_id,
                    next_position,
                    scenario_type_enum_id,
                    &user_defined_id,
                    &description,
                    now.timestamp(),
                    now.timestamp(),
                    now.timestamp_subsec_nanos(),
                ))
                .unwrap();

            txn.last_insert_rowid()
        };

        txn.commit().unwrap();

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(ScenarioId(scenario_id))
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn lookup_scenario(&mut self, user_defined_id: String) -> LookupResult<ScenarioId> {
        let txn = self.db.transaction().unwrap();

        let result = {
            let mut query = txn
                .prepare_cached("SELECT scenario_id FROM scenario WHERE user_defined_id = ?")
                .unwrap();
            let query = query
                .query_row((user_defined_id,), |row| row.get::<usize, i64>(0))
                .optional()
                .unwrap();

            if let Some(id) = query {
                Ok(ScenarioId(id))
            } else {
                Err(LookupError::NotFound)
            }
        };

        txn.commit().unwrap();

        result
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn move_scenario_before(
        &mut self,
        move_scenario_id: ScenarioId,
        before_scenario_id: ScenarioId,
    ) -> Result<(), MoveScenarioError> {
        let txn = self.db.transaction().unwrap();
        let (old_position, old_parent) = {
            let mut query = txn
                .prepare_cached("SELECT position, parent_id FROM scenario WHERE scenario_id = ?")
                .unwrap();
            let query = query
                .query_row((move_scenario_id.0,), |row| {
                    Ok((row.get::<usize, u32>(0)?, row.get::<usize, Option<i64>>(1)?))
                })
                .optional()
                .unwrap();

            if let Some((position, parent_id)) = query {
                (position, parent_id)
            } else {
                return Err(MoveScenarioError::MoveScenarioNotFound);
            }
        };
        let (before_position, new_parent) = {
            let mut query = txn
                .prepare_cached("SELECT position, parent_id FROM scenario WHERE scenario_id = ?")
                .unwrap();
            let query = query
                .query_row((before_scenario_id.0,), |row| {
                    Ok((row.get::<usize, u32>(0)?, row.get::<usize, Option<i64>>(1)?))
                })
                .optional()
                .unwrap();

            if let Some((position, parent_id)) = query {
                (position, parent_id)
            } else {
                return Err(MoveScenarioError::TargetScenarioNotFound);
            }
        };

        // NOTE: We allow moving to a different parent

        if old_parent != new_parent {
            if let Some(new_parent) = new_parent {
                // Make sure we aren't reparented under one of our own children
                {
                    let mut query = txn
                        .prepare_cached(
                            "
                            WITH RECURSIVE
                                parent(scenario_id, parent_id)
                            AS
                            (
                                SELECT scenario_id, parent_id FROM scenario WHERE scenario_id = ?

                                UNION ALL

                                SELECT
                                    s.scenario_id, s.parent_id
                                FROM
                                    parent p
                                JOIN
                                    scenario s
                                ON
                                    p.parent_id = s.scenario_id
                            )
                            SELECT scenario_id FROM parent WHERE scenario_id = ?
                            ",
                        )
                        .unwrap();

                    if query
                        .query_row((new_parent, move_scenario_id.0), |_| Ok(()))
                        .optional()
                        .unwrap()
                        .is_some()
                    {
                        // The new parent is the scenario or a child of the scenario
                        return Err(MoveScenarioError::TargetScenarioIsChildOfMoveScenario);
                    }
                }

                // Make sure we don't exceed the depth limit
                let new_parent_depth = get_scenario_depth(&txn, new_parent).unwrap();
                let my_child_depth = get_scenario_child_depth(&txn, move_scenario_id.0);
                let new_total_depth = new_parent_depth + my_child_depth;

                if new_total_depth >= MAX_SCENARIO_TREE_DEPTH {
                    return Err(MoveScenarioError::MaxScenarioTreeDepthExceeded);
                }
            }
        }

        // First, temporarily set the position of the scenario to some unused value so we don't
        // violate a uniqueness constraint while updating the positions
        {
            // We guarantee position 0 is never used
            let mut update = txn
                .prepare_cached("UPDATE scenario SET position = 0 WHERE scenario_id = ?")
                .unwrap();

            update.execute((move_scenario_id.0,)).unwrap();
        }

        // Now, change the position of everything after the old position on the old parent to
        // remove the gap
        if let Some(old_parent) = old_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position - 1 WHERE parent_id = ? AND position > ?",
            ).unwrap();

            update.execute((old_parent, old_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position - 1 WHERE parent_id IS NULL AND position > ?").unwrap();

            update.execute((old_position,)).unwrap();
        }

        let mut target_position = before_position;

        if old_parent == new_parent && target_position > old_position {
            // account for the shift we just did if we're moving inside the same parent
            target_position -= 1;
        }

        let target_position = target_position;

        // Then, change the position of the target and everything after it on the new parent to
        // create a gap

        // This increases the position by 1, but does it in a roundabout way to avoid violating a
        // uniqueness constraint. TODO: Is there a better way to do this?

        if let Some(new_parent) = new_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position + 999999 WHERE parent_id = ? AND position >= ?",
            ).unwrap();

            update.execute((new_parent, target_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position + 999999 WHERE parent_id IS NULL AND position >= ?").unwrap();

            update.execute((target_position,)).unwrap();
        }

        if let Some(new_parent) = new_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position - 999998 WHERE parent_id = ? AND position >= ?",
            ).unwrap();

            update.execute((new_parent, target_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position - 999998 WHERE parent_id IS NULL AND position >= ?").unwrap();

            update.execute((target_position,)).unwrap();
        }

        // Finally, set the new position and parent on the scenario
        {
            let mut update = txn
                .prepare_cached(
                    "UPDATE scenario SET parent_id = ?, position = ? WHERE scenario_id = ?",
                )
                .unwrap();

            update
                .execute((new_parent, target_position, move_scenario_id.0))
                .unwrap();
        }

        txn.commit().unwrap();

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn move_scenario_after(
        &mut self,
        move_scenario_id: ScenarioId,
        after_scenario_id: ScenarioId,
    ) -> Result<(), MoveScenarioError> {
        let txn = self.db.transaction().unwrap();
        let (old_position, old_parent) = {
            let mut query = txn
                .prepare_cached("SELECT position, parent_id FROM scenario WHERE scenario_id = ?")
                .unwrap();
            let query = query
                .query_row((move_scenario_id.0,), |row| {
                    Ok((row.get::<usize, u32>(0)?, row.get::<usize, Option<i64>>(1)?))
                })
                .optional()
                .unwrap();

            if let Some((position, parent_id)) = query {
                (position, parent_id)
            } else {
                return Err(MoveScenarioError::MoveScenarioNotFound);
            }
        };
        let (after_position, new_parent) = {
            let mut query = txn
                .prepare_cached("SELECT position, parent_id FROM scenario WHERE scenario_id = ?")
                .unwrap();
            let query = query
                .query_row((after_scenario_id.0,), |row| {
                    Ok((row.get::<usize, u32>(0)?, row.get::<usize, Option<i64>>(1)?))
                })
                .optional()
                .unwrap();

            if let Some((position, parent_id)) = query {
                (position, parent_id)
            } else {
                return Err(MoveScenarioError::TargetScenarioNotFound);
            }
        };

        // NOTE: We allow moving to a different parent

        if old_parent != new_parent {
            if let Some(new_parent) = new_parent {
                // Make sure we aren't reparented under one of our own children
                {
                    let mut query = txn
                        .prepare_cached(
                            "
                            WITH RECURSIVE
                                parent(scenario_id, parent_id)
                            AS
                            (
                                SELECT scenario_id, parent_id FROM scenario WHERE scenario_id = ?

                                UNION ALL

                                SELECT
                                    s.scenario_id, s.parent_id
                                FROM
                                    parent p
                                JOIN
                                    scenario s
                                ON
                                    p.parent_id = s.scenario_id
                            )
                            SELECT scenario_id FROM parent WHERE scenario_id = ?
                            ",
                        )
                        .unwrap();

                    if query
                        .query_row((new_parent, move_scenario_id.0), |_| Ok(()))
                        .optional()
                        .unwrap()
                        .is_some()
                    {
                        // The new parent is the scenario or a child of the scenario
                        return Err(MoveScenarioError::TargetScenarioIsChildOfMoveScenario);
                    }
                }

                // Make sure we don't exceed the depth limit
                let new_parent_depth = get_scenario_depth(&txn, new_parent).unwrap();
                let my_child_depth = get_scenario_child_depth(&txn, move_scenario_id.0);
                let new_total_depth = new_parent_depth + my_child_depth;

                if new_total_depth >= MAX_SCENARIO_TREE_DEPTH {
                    return Err(MoveScenarioError::MaxScenarioTreeDepthExceeded);
                }
            }
        }

        // First, temporarily set the position of the scenario to some unused value so we don't
        // violate a uniqueness constraint while updating the positions
        {
            // We guarantee position 0 is never used
            let mut update = txn
                .prepare_cached("UPDATE scenario SET position = 0 WHERE scenario_id = ?")
                .unwrap();

            update.execute((move_scenario_id.0,)).unwrap();
        }

        // Now, change the position of everything after the old position on the old parent to
        // remove the gap
        if let Some(old_parent) = old_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position - 1 WHERE parent_id = ? AND position > ?",
            ).unwrap();

            update.execute((old_parent, old_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position - 1 WHERE parent_id IS NULL AND position > ?").unwrap();

            update.execute((old_position,)).unwrap();
        }

        let mut target_position = after_position + 1;

        if old_parent == new_parent && target_position > old_position {
            // account for the shift we just did if we're moving inside the same parent
            target_position -= 1;
        }

        let target_position = target_position;

        // Then, change the position of the target and everything after it on the new parent to
        // create a gap

        // This increases the position by 1, but does it in a roundabout way to avoid violating a
        // uniqueness constraint. TODO: Is there a better way to do this?

        if let Some(new_parent) = new_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position + 999999 WHERE parent_id = ? AND position >= ?",
            ).unwrap();

            update.execute((new_parent, target_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position + 999999 WHERE parent_id IS NULL AND position >= ?").unwrap();

            update.execute((target_position,)).unwrap();
        }

        if let Some(new_parent) = new_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position - 999998 WHERE parent_id = ? AND position >= ?",
            ).unwrap();

            update.execute((new_parent, target_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position - 999998 WHERE parent_id IS NULL AND position >= ?").unwrap();

            update.execute((target_position,)).unwrap();
        }

        // Finally, set the new position and parent on the scenario
        {
            let mut update = txn
                .prepare_cached(
                    "UPDATE scenario SET parent_id = ?, position = ? WHERE scenario_id = ?",
                )
                .unwrap();

            update
                .execute((new_parent, target_position, move_scenario_id.0))
                .unwrap();
        }

        txn.commit().unwrap();

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn move_scenario_inside(
        &mut self,
        move_scenario_id: ScenarioId,
        parent_scenario_id: ScenarioId,
    ) -> Result<(), MoveScenarioError> {
        let txn = self.db.transaction().unwrap();
        let (old_position, old_parent) = {
            let mut query = txn
                .prepare_cached("SELECT position, parent_id FROM scenario WHERE scenario_id = ?")
                .unwrap();
            let query = query
                .query_row((move_scenario_id.0,), |row| {
                    Ok((row.get::<usize, u32>(0)?, row.get::<usize, Option<i64>>(1)?))
                })
                .optional()
                .unwrap();

            if let Some(row) = query {
                row
            } else {
                return Err(MoveScenarioError::MoveScenarioNotFound);
            }
        };

        {
            let mut query = txn
                .prepare_cached("SELECT scenario_id FROM scenario WHERE scenario_id = ?")
                .unwrap();

            if query
                .query_row((parent_scenario_id.0,), |_| Ok(()))
                .optional()
                .unwrap()
                .is_none()
            {
                return Err(MoveScenarioError::TargetScenarioNotFound);
            }
        };

        // NOTE: In this case we are always moving to a different parent

        let new_parent = parent_scenario_id.0;

        if old_parent == Some(new_parent) {
            // If the target is our parent, we don't need to do anything
            txn.commit().unwrap();
            return Ok(());
        }

        // Make sure we aren't reparented under one of our own children
        {
            let mut query = txn
                .prepare_cached(
                    "
                    WITH RECURSIVE
                        parent(scenario_id, parent_id)
                    AS
                    (
                        SELECT scenario_id, parent_id FROM scenario WHERE scenario_id = ?

                        UNION ALL

                        SELECT
                            s.scenario_id, s.parent_id
                        FROM
                            parent p
                        JOIN
                            scenario s
                        ON
                            p.parent_id = s.scenario_id
                    )
                    SELECT scenario_id FROM parent WHERE scenario_id = ?
                    ",
                )
                .unwrap();

            if query
                .query_row((new_parent, move_scenario_id.0), |_| Ok(()))
                .optional()
                .unwrap()
                .is_some()
            {
                // The new parent is the scenario or a child of the scenario
                return Err(MoveScenarioError::TargetScenarioIsChildOfMoveScenario);
            }
        }

        // Make sure we don't exceed the depth limit
        {
            let new_parent_depth = get_scenario_depth(&txn, new_parent).unwrap();
            let my_child_depth = get_scenario_child_depth(&txn, move_scenario_id.0);
            let new_total_depth = new_parent_depth + my_child_depth;

            if new_total_depth >= MAX_SCENARIO_TREE_DEPTH {
                return Err(MoveScenarioError::MaxScenarioTreeDepthExceeded);
            }
        }

        // First, temporarily set the position of the scenario to some unused value so we don't
        // violate a uniqueness constraint while updating the positions
        {
            // We guarantee position 0 is never used
            let mut update = txn
                .prepare_cached("UPDATE scenario SET position = 0 WHERE scenario_id = ?")
                .unwrap();

            update.execute((move_scenario_id.0,)).unwrap();
        }

        // Now, change the position of everything after the old position on the old parent to
        // remove the gap
        if let Some(old_parent) = old_parent {
            let mut update = txn.prepare_cached(
                "UPDATE scenario SET position = position - 1 WHERE parent_id = ? AND position > ?",
            ).unwrap();

            update.execute((old_parent, old_position)).unwrap();
        } else {
            let mut update =
                txn.prepare_cached("UPDATE scenario SET position = position - 1 WHERE parent_id IS NULL AND position > ?").unwrap();

            update.execute((old_position,)).unwrap();
        }

        // Get the first free position under the new parent
        let target_position = {
            let mut query = txn
                .prepare_cached("SELECT MAX(position) FROM scenario WHERE parent_id = ?")
                .unwrap();

            if let Some(position) = query
                .query_row((new_parent,), |row| Ok(row.get::<usize, Option<u32>>(0)))
                .unwrap()
                .unwrap()
            {
                position + 1
            } else {
                1
            }
        };

        // Finally, set the new position and parent on the scenario
        {
            let mut update = txn
                .prepare_cached(
                    "UPDATE scenario SET parent_id = ?, position = ? WHERE scenario_id = ?",
                )
                .unwrap();

            update
                .execute((new_parent, target_position, move_scenario_id.0))
                .unwrap();
        }

        txn.commit().unwrap();

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn delete_scenario(&mut self, scenario_id: ScenarioId) -> Result<(), DeleteScenarioError> {
        let txn = self.db.transaction().unwrap();

        delete_scenario(&txn, scenario_id.0)?;
        txn.commit().unwrap();

        // Notify subscribers that this scenario has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.scenario_update_tx.send(scenario_id);

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_scenario_info(&mut self, scenario_id: ScenarioId) -> LookupResult<ScenarioInfo> {
        let txn = self.db.transaction().unwrap();
        let scenario_info = {
            let mut query = txn
                .prepare_cached(
                    "
                    SELECT
                        scenario_id,
                        scenario_type_enum_id,
                        user_defined_id,
                        description,
                        date_created,
                        date_modified_secs,
                        date_modified_nsecs
                    FROM
                        scenario
                    WHERE
                        scenario_id = ?
                    ",
                )
                .unwrap();
            let scenario_info = query.query_row((scenario_id.0,), deserialize_scenario);

            if let Ok(scenario_info) = scenario_info {
                scenario_info
            } else {
                return Err(LookupError::NotFound);
            }
        };

        txn.commit().unwrap();

        Ok(scenario_info)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn update_scenario_info(
        &mut self,
        scenario_id: ScenarioId,
        user_defined_id: String,
        description: String,
        previous_modified_time: chrono::DateTime<chrono::Local>,
    ) -> Result<(), UpdateScenarioInfoError> {
        let txn = self.db.transaction().unwrap();
        let existing_info = {
            let mut query = txn
                .prepare_cached(
                    "
                    SELECT
                        scenario_id,
                        scenario_type_enum_id,
                        user_defined_id,
                        description,
                        date_created,
                        date_modified_secs,
                        date_modified_nsecs
                    FROM
                        scenario
                    WHERE
                        scenario_id = ?
                    ",
                )
                .unwrap();
            let scenario_info = query.query_row((scenario_id.0,), deserialize_scenario);

            if let Ok(scenario_info) = scenario_info {
                scenario_info
            } else {
                return Err(UpdateScenarioInfoError::ScenarioNotFound);
            }
        };

        if previous_modified_time != existing_info.modified_time {
            return Err(UpdateScenarioInfoError::ModifiedTimeDoesNotMatchDatabase);
        }

        if user_defined_id != existing_info.user_defined_id {
            let mut query = txn
                .prepare_cached("SELECT scenario_id FROM scenario WHERE user_defined_id = ?")
                .unwrap();

            if query
                .query_row((&user_defined_id,), |_| Ok(()))
                .optional()
                .unwrap()
                .is_some()
            {
                return Err(UpdateScenarioInfoError::UserDefinedIdAlreadyExists);
            }
        }

        let (date_modified_secs, date_modified_nsecs) = {
            let now = chrono::offset::Local::now();
            (now.timestamp(), now.timestamp_subsec_nanos())
        };

        {
            let mut update = txn
                .prepare_cached(
                    "
                    UPDATE
                        scenario
                    SET
                        user_defined_id = ?,
                        description = ?,
                        date_modified_secs = ?,
                        date_modified_nsecs = ?
                    WHERE
                        scenario_id = ?
                    ",
                )
                .unwrap();

            update
                .execute((
                    &user_defined_id,
                    &description,
                    date_modified_secs,
                    date_modified_nsecs,
                    scenario_id.0,
                ))
                .unwrap();
        }

        txn.commit().unwrap();

        // Notify subscribers that this scenario has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.scenario_update_tx.send(scenario_id);

        // Notify subscribers that the methodology has been updated. It's OK for this to return an
        // error if there are no subscribers.
        let _ = self.methodology_update_tx.send(());

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn subscribe_scenario(&self) -> tokio::sync::broadcast::Receiver<ScenarioId> {
        self.scenario_update_tx.subscribe()
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn subscribe_methodology(&self) -> tokio::sync::watch::Receiver<()> {
        self.methodology_update_tx.subscribe()
    }
}

pub(super) type OpenResult<T> = std::result::Result<T, OpenError>;

#[derive(Debug)]
pub(super) enum OpenError {
    WrongApplicationId,
    IncompatibleFileVersion,
    Database(rusqlite::Error),
}

impl From<rusqlite::Error> for OpenError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}

/// Checks that an existing database was created by a compatible version
fn check_valid(db: &rusqlite::Connection) -> OpenResult<()> {
    db.query_row_and_then("SELECT * FROM pragma_application_id", [], |row| {
        let app_id: u32 = row.get(0)?;
        tracing::trace!(app_id);

        if app_id == APPLICATION_ID {
            Ok(())
        } else {
            Err(OpenError::WrongApplicationId)
        }
    })?;

    db.query_row_and_then("SELECT * FROM pragma_user_version", [], |row| {
        let user_version: i64 = row.get(0)?;
        tracing::trace!(user_version);

        if user_version == FILE_FORMAT_VERSION {
            Ok(())
        } else {
            Err(OpenError::IncompatibleFileVersion)
        }
    })
}

/// Initializes a database connection. Called after a datbase is either created or opened.
fn init(db: &rusqlite::Connection) -> OpenResult<()> {
    db.pragma_update(None, "foreign_keys", 1)?;
    db.pragma_update(None, "secure_delete", 1)?;
    db.set_prepared_statement_cache_capacity(PREPARED_STATEMENT_CACHE_CAPACITY);
    Ok(())
}

/// Create a new database
fn create(db: &mut rusqlite::Connection) -> OpenResult<()> {
    let txn = db.transaction()?;

    txn.pragma_update(None, "application_id", APPLICATION_ID)?;
    txn.pragma_update(None, "user_version", FILE_FORMAT_VERSION)?;
    txn.execute_batch(include_str!("../sql/create.sql"))?;
    txn.commit()?;

    Ok(())
}

#[derive(Debug)]
pub(crate) enum Command {
    StoreHttpHistory {
        test_id: Option<TestId>,
        http_message: HttpMessage,
        reply: tokio::sync::oneshot::Sender<HistoryId>,
    },
    GetHttpHistory {
        history_id: HistoryId,
        reply: tokio::sync::oneshot::Sender<LookupResult<HttpMessage>>,
    },
    CreateHistorySearch {
        reply: tokio::sync::oneshot::Sender<HistorySearchId>,
    },
    GetHistorySearchItems {
        history_search_id: HistorySearchId,
        start_index: u32,
        count: u32,
        reply: tokio::sync::oneshot::Sender<LookupResult<Vec<HistoryId>>>,
    },
    GetHistorySearchCount {
        history_search_id: HistorySearchId,
        reply: tokio::sync::oneshot::Sender<LookupResult<u32>>,
    },
    SubscribeHistorySearch {
        history_search_id: HistorySearchId,
        reply: tokio::sync::oneshot::Sender<tokio::sync::watch::Receiver<()>>,
    },
    DeleteHistorySearch {
        history_search_id: HistorySearchId,
        reply: tokio::sync::oneshot::Sender<LookupResult<()>>,
    },
    ListScenarios {
        reply: tokio::sync::oneshot::Sender<Vec<ScenarioTreeNode>>,
    },
    CreateScenario {
        parent: Option<ScenarioId>,
        user_defined_id: String,
        description: String,
        type_: ScenarioType,
        reply: tokio::sync::oneshot::Sender<Result<ScenarioId, CreateScenarioError>>,
    },
    LookupScenario {
        user_defined_id: String,
        reply: tokio::sync::oneshot::Sender<LookupResult<ScenarioId>>,
    },
    MoveScenarioBefore {
        move_scenario_id: ScenarioId,
        before_scenario_id: ScenarioId,
        reply: tokio::sync::oneshot::Sender<Result<(), MoveScenarioError>>,
    },
    MoveScenarioAfter {
        move_scenario_id: ScenarioId,
        after_scenario_id: ScenarioId,
        reply: tokio::sync::oneshot::Sender<Result<(), MoveScenarioError>>,
    },
    MoveScenarioInside {
        move_scenario_id: ScenarioId,
        parent_scenario_id: ScenarioId,
        reply: tokio::sync::oneshot::Sender<Result<(), MoveScenarioError>>,
    },
    DeleteScenario {
        scenario_id: ScenarioId,
        reply: tokio::sync::oneshot::Sender<Result<(), DeleteScenarioError>>,
    },
    GetScenarioInfo {
        scenario_id: ScenarioId,
        reply: tokio::sync::oneshot::Sender<LookupResult<ScenarioInfo>>,
    },
    UpdateScenarioInfo {
        scenario_id: ScenarioId,
        user_defined_id: String,
        description: String,
        previous_modified_time: chrono::DateTime<chrono::Local>,
        reply: tokio::sync::oneshot::Sender<Result<(), UpdateScenarioInfoError>>,
    },
    SubscribeScenario {
        reply: tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<ScenarioId>>,
    },
    SubscribeMethodology {
        reply: tokio::sync::oneshot::Sender<tokio::sync::watch::Receiver<()>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TestId(pub(crate) i64);

#[derive(Clone, Copy, Debug)]
pub(crate) struct HistoryId(pub(crate) i64);

pub(crate) type LookupResult<T> = std::result::Result<T, LookupError>;

#[derive(Debug)]
pub(crate) enum LookupError {
    NotFound,
}

#[derive(Debug)]
pub(crate) struct HttpMessage {
    pub(crate) request_time: chrono::DateTime<chrono::Local>,
    pub(crate) response_time: chrono::DateTime<chrono::Local>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) is_https: bool,
    pub(crate) request: bytes::Bytes,
    pub(crate) response: HttpResult<bytes::Bytes>,
}

pub(crate) type HttpResult<T> = std::result::Result<T, HttpError>;

// NOTE: This must be synced with the enum in the database definition
//       and the capnp IDL
#[derive(Debug, num_enum::IntoPrimitive, num_enum::TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum HttpError {
    Dns = 1,
    CouldNotConnect = 2,
    ConnectionClosed = 3,
    ResponseTimeout = 4,
    ResponseTooLarge = 5,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HistorySearchId;

#[derive(Debug)]
pub(crate) struct ScenarioTreeNode {
    pub(crate) info: ScenarioInfo,
    pub(crate) children: Vec<ScenarioTreeNode>,
}

#[derive(Debug)]
pub(crate) struct ScenarioInfo {
    pub(crate) id: ScenarioId,
    pub(crate) user_defined_id: String,
    pub(crate) description: String,
    pub(crate) type_: ScenarioType,
    pub(crate) created_time: chrono::DateTime<chrono::Local>,
    pub(crate) modified_time: chrono::DateTime<chrono::Local>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScenarioId(i64);

// NOTE: This must be synced with the enum in the database definition
//       and the capnp IDL
#[derive(Debug, Eq, num_enum::IntoPrimitive, PartialEq, num_enum::TryFromPrimitive)]
#[repr(u8)]
pub(crate) enum ScenarioType {
    Container = 2,
    Generic = 3,
    Location = 4,
    Endpoint = 5,
    Authorization = 6,
    BusinessLogic = 7,
}

#[derive(Debug)]
pub(crate) enum CreateScenarioError {
    IdAlreadyExists,
    ParentDoesNotExist,
    MaxScenarioTreeDepthExceeded,
    ScenarioLimitExceeded,
}

#[derive(Debug)]
pub(crate) enum MoveScenarioError {
    MoveScenarioNotFound,
    TargetScenarioNotFound,
    TargetScenarioIsChildOfMoveScenario,
    MaxScenarioTreeDepthExceeded,
}

#[derive(Debug)]
pub(crate) enum DeleteScenarioError {
    ScenarioNotFound,
    ScenarioHasTestingData(String),
}

#[derive(Debug)]
pub(crate) enum UpdateScenarioInfoError {
    ScenarioNotFound,
    ModifiedTimeDoesNotMatchDatabase,
    UserDefinedIdAlreadyExists,
}

#[derive(Default)]
struct StorageStats {
    bytes_total: u64,
    bytes_stored: u64,
    bytes_deduplicated: u64,
}

#[tracing::instrument(level = "trace", skip_all)]
fn store_http_request(
    txn: &rusqlite::Transaction,
    http_request: &bytes::Bytes,
    storage_stats: &mut StorageStats,
) -> (i64, i64, Option<i64>) {
    let index = {
        let mut found_first = false;
        let mut result = None;

        // Find the second newline. This separates the GET line and the Host header from the rest
        // of the header because the rest of the header is fairly static and can be deduplicated.
        for (index, byte) in http_request.iter().enumerate() {
            if *byte == b'\n' {
                if found_first {
                    result = Some(index + 1);
                    break;
                } else {
                    found_first = true;
                }
            }
        }

        result
    };

    if let Some(index) = index {
        let request_location_id = store_content(txn, &http_request[0..index], storage_stats);

        let (request_header_id, request_body_id) = if index < http_request.len() {
            store_http_request_remainder(txn, &http_request[index..], storage_stats)
        } else {
            (store_content(txn, b"", storage_stats), None)
        };

        (request_location_id, request_header_id, request_body_id)
    } else {
        let request_location_id = store_content(txn, b"", storage_stats);
        let request_header_id = store_content(txn, http_request, storage_stats);

        (request_location_id, request_header_id, None)
    }
}

fn store_http_request_remainder(
    txn: &rusqlite::Transaction,
    http_request: &[u8],
    storage_stats: &mut StorageStats,
) -> (i64, Option<i64>) {
    // Store the header and response body in two separate chunks, so duplicate bodies can be
    // deduplicated.

    if let Some(index) = http_request.windows(4).position(|w| w == b"\r\n\r\n") {
        let index = index + 4;
        let request_header_id = store_content(txn, &http_request[0..index], storage_stats);
        let request_body_id = if index < http_request.len() {
            Some(store_content(txn, &http_request[index..], storage_stats))
        } else {
            None
        };

        (request_header_id, request_body_id)
    } else {
        (store_content(txn, http_request, storage_stats), None)
    }
}

#[tracing::instrument(level = "trace", skip_all)]
fn store_content(
    txn: &rusqlite::Transaction,
    content: &[u8],
    storage_stats: &mut StorageStats,
) -> i64 {
    let len = content.len() as u64;
    let hash = hash_bytes(content);
    let mut query = txn
        .prepare_cached("SELECT content_id FROM content WHERE hash = ?")
        .unwrap();

    storage_stats.bytes_total += len;

    if let Some(content_id) = query
        .query_row((hash,), |row| row.get(0))
        .optional()
        .unwrap()
    {
        tracing::trace!("Using existing content record");
        storage_stats.bytes_deduplicated += len;

        content_id
    } else {
        tracing::trace!("Creating content record");
        //let content_str = String::from_utf8_lossy(content);
        //tracing::trace!("    Storing content: {content_str:?}");

        let compressed_data = zstd::bulk::compress(content, ZSTD_COMPRESSION_LEVEL).unwrap();
        let compressed_len = compressed_data.len() as u64;

        storage_stats.bytes_stored += compressed_len;

        if compressed_len > len {
            tracing::debug!(
                "Compression resulted in increase from {len} to {compressed_len} bytes"
            );
        }

        let mut insert = txn
            .prepare_cached("INSERT INTO content (hash, data) VALUES (?,?)")
            .unwrap();

        insert.execute((hash, compressed_data)).unwrap();

        txn.last_insert_rowid()
    }
}

fn hash_bytes(bytes: &[u8]) -> i64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(bytes);
    // We only use the first 8 bytes from the 32 byte hash. While this could theoretically result in
    // hash collisions, we assume the chance of this occuring in practice for our expected database
    // size to be so unlikely that we don't even bother checking for it. Is this a mistake?
    let mut buf = [0u8; 8];
    hasher.finalize_xof().fill(&mut buf);
    i64::from_ne_bytes(buf)
}

#[tracing::instrument(level = "trace", skip_all)]
fn store_http_response(
    txn: &rusqlite::Transaction,
    http_response: &bytes::Bytes,
    storage_stats: &mut StorageStats,
) -> (i64, i64) {
    // Store the header and response body in two separate chunks, so duplicate bodies can be
    // deduplicated.
    let (header, body) = {
        if let Some(index) = http_response.windows(4).position(|w| w == b"\r\n\r\n") {
            let index = index + 4;
            let header = &http_response[0..index];
            let body = if index < http_response.len() {
                &http_response[index..]
            } else {
                &b""[..]
            };

            (header, body)
        } else {
            let header = &http_response[..];
            let body = &b""[..];

            (header, body)
        }
    };

    let header_id = store_content(txn, header, storage_stats);
    let body_id = store_content(txn, body, storage_stats);

    (header_id, body_id)
}

#[tracing::instrument(level = "trace", skip_all)]
fn load_http_request(
    txn: &rusqlite::Transaction,
    request_location_id: i64,
    request_header_id: i64,
    opt_request_body_id: Option<i64>,
) -> bytes::Bytes {
    let mut bytes = bytes::BytesMut::with_capacity(LOAD_HTTP_REQUEST_INITIAL_BUFFER_CAPACITY);

    load_content(txn, request_location_id, &mut bytes);
    load_content(txn, request_header_id, &mut bytes);
    if let Some(request_body_id) = opt_request_body_id {
        load_content(txn, request_body_id, &mut bytes);
    }

    bytes.freeze()
}

fn load_content(txn: &rusqlite::Transaction, content_id: i64, bytes: &mut bytes::BytesMut) {
    let mut query = txn
        .prepare_cached("SELECT data FROM content WHERE content_id = ?")
        .unwrap();
    let compressed_data: Vec<u8> = query.query_row((content_id,), |row| row.get(0)).unwrap();

    zstd::stream::copy_decode(compressed_data.reader(), bytes.writer()).unwrap();
}

#[tracing::instrument(level = "trace", skip_all)]
fn load_http_response(
    txn: &rusqlite::Transaction,
    response_header_id: i64,
    response_body_id: i64,
) -> bytes::Bytes {
    let mut bytes = bytes::BytesMut::with_capacity(LOAD_HTTP_RESPONSE_INITIAL_BUFFER_CAPACITY);

    load_content(txn, response_header_id, &mut bytes);
    load_content(txn, response_body_id, &mut bytes);

    bytes.freeze()
}

fn list_scenario_children(
    txn: &rusqlite::Transaction,
    parent_id: Option<ScenarioId>,
    recursion_depth: u8,
) -> Vec<ScenarioTreeNode> {
    // TODO: This limit should be stored in the database
    assert!(recursion_depth <= MAX_SCENARIO_TREE_DEPTH);

    let mut query = if parent_id.is_some() {
        txn.prepare_cached(
            "
            SELECT
                scenario_id,
                scenario_type_enum_id,
                user_defined_id,
                description,
                date_created,
                date_modified_secs,
                date_modified_nsecs
            FROM
                scenario
            WHERE
                parent_id = ?
            ORDER BY
                position
            ",
        )
        .unwrap()
    } else {
        txn.prepare_cached(
            "
            SELECT
                scenario_id,
                scenario_type_enum_id,
                user_defined_id,
                description,
                date_created,
                date_modified_secs,
                date_modified_nsecs
            FROM
                scenario
            WHERE
                parent_id IS NULL
                -- This is the 'Unassigned Proxy Traffic' scenario
                AND scenario_id > 1
            ORDER BY
                position
            ",
        )
        .unwrap()
    };

    let query = if parent_id.is_some() {
        query
            .query_map((parent_id.map(|x| x.0),), deserialize_scenario)
            .unwrap()
    } else {
        query.query_map([], deserialize_scenario).unwrap()
    };

    let mut result = Vec::new();

    for info in query {
        let info = info.unwrap();
        let parent_id = Some(info.id);
        let recursion_depth = recursion_depth + 1;
        let children = list_scenario_children(txn, parent_id, recursion_depth);

        result.push(ScenarioTreeNode { info, children });
    }

    result
}

fn deserialize_scenario(row: &rusqlite::Row) -> rusqlite::Result<ScenarioInfo> {
    let scenario_id = row.get(0)?;
    let scenario_type_enum_id = row.get::<usize, u8>(1)?;
    let user_defined_id = row.get(2)?;
    let description = row.get(3)?;
    let date_created = row.get(4)?;
    let date_modified_secs = row.get(5)?;
    let date_modified_nsecs = row.get(6)?;

    Ok(ScenarioInfo {
        id: ScenarioId(scenario_id),
        user_defined_id,
        description,
        type_: ScenarioType::try_from(scenario_type_enum_id).unwrap(),
        created_time: chrono::Local.timestamp_opt(date_created, 0).unwrap(),
        modified_time: chrono::Local
            .timestamp_opt(date_modified_secs, date_modified_nsecs)
            .unwrap(),
    })
}

/// If the scenario exists, returns the absolute depth of the scenario
fn get_scenario_depth(txn: &rusqlite::Transaction, scenario_id: i64) -> Option<u8> {
    let mut query = txn
        .prepare_cached(
            "
            WITH RECURSIVE
                depth(scenario_id, depth)
            AS
            (
                SELECT scenario_id, 0 FROM scenario WHERE parent_id IS NULL

                UNION ALL

                SELECT
                    s.scenario_id, depth + 1
                FROM
                    depth d
                JOIN
                    scenario s
                ON
                    d.scenario_id = s.parent_id
            )
            SELECT depth FROM depth WHERE scenario_id = ?
            ",
        )
        .unwrap();

    if let Some(depth) = query
        .query_row((scenario_id,), |row| Ok(row.get(0)))
        .optional()
        .unwrap()
    {
        let depth = depth.unwrap();

        Some(depth)
    } else {
        None
    }
}

/// Returns the deepest depth of the scenario and all the child scenarios relative to the scenario
/// itself
fn get_scenario_child_depth(txn: &rusqlite::Transaction, scenario_id: i64) -> u8 {
    let mut query = txn
        .prepare_cached(
            "
            WITH RECURSIVE
                depth(scenario_id, depth)
            AS
            (
                SELECT scenario_id, 2 FROM scenario WHERE parent_id = ?

                UNION ALL

                SELECT
                    s.scenario_id, depth + 1
                FROM
                    depth d
                JOIN
                    scenario s
                ON
                    d.scenario_id = s.parent_id
            )
            SELECT MAX(depth) FROM depth
            ",
        )
        .unwrap();

    if let Some(depth) = query
        .query_row((scenario_id,), |row| Ok(row.get(0)))
        .optional()
        .unwrap()
    {
        let depth = depth.unwrap();

        if let Some(depth) = depth {
            depth
        } else {
            1
        }
    } else {
        1
    }
}

fn delete_scenario(
    txn: &rusqlite::Transaction,
    scenario_id: i64,
) -> Result<(), DeleteScenarioError> {
    // Verify that we exist and get some information we'll need later
    let (parent_id, position, user_defined_id) =
        {
            let mut query = txn.prepare_cached(
        "SELECT parent_id, position, user_defined_id FROM scenario WHERE scenario_id = ?",
            ).unwrap();
            let query = query
                .query_row((scenario_id,), |row| {
                    Ok((
                        row.get::<usize, Option<i64>>(0)?,
                        row.get::<usize, u32>(1)?,
                        row.get(2)?,
                    ))
                })
                .optional()
                .unwrap();

            if let Some(values) = query {
                values
            } else {
                return Err(DeleteScenarioError::ScenarioNotFound);
            }
        };

    // Recursively delete our children before deleting ourself
    {
        let mut query = txn
            .prepare_cached("SELECT scenario_id FROM scenario WHERE parent_id = ?")
            .unwrap();
        let query = query
            .query_map((scenario_id,), |row| row.get::<usize, i64>(0))
            .unwrap();

        for child_id in query {
            let child_id = child_id.unwrap();

            delete_scenario(txn, child_id)?;
        }
    }

    // Delete ourself. We are relying on the database engine to enforce constraints to tell if
    // we can delete the record or not. TODO: Make sure we verify that constraint enforcement is
    // turned on.
    {
        let mut delete = txn
            .prepare_cached("DELETE FROM scenario WHERE scenario_id = ?")
            .unwrap();

        match delete.execute((scenario_id,)) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _)) => {
                if e.code == rusqlite::ErrorCode::ConstraintViolation {
                    return Err(DeleteScenarioError::ScenarioHasTestingData(user_defined_id));
                } else {
                    panic!("{e:?}");
                }
            }
            Err(e) => panic!("{e:?}"),
        }
    }

    // Adjust the position of any scenarios after this one to avoid any gaps
    if let Some(parent_id) = parent_id {
        let mut update = txn
            .prepare_cached(
                "UPDATE scenario SET position = position - 1 WHERE parent_id = ? AND position > ?",
            )
            .unwrap();

        update.execute((parent_id, position)).unwrap();
    } else {
        let mut update = txn.prepare_cached(
            "UPDATE scenario SET position = position - 1 WHERE parent_id IS NULL AND position > ?",
        ).unwrap();

        update.execute((position,)).unwrap();
    }

    Ok(())
}

#[derive(Clone)]
pub(crate) struct Channel {
    storage_tx: tokio::sync::mpsc::Sender<Command>,
}

impl Channel {
    pub(super) fn from_sender(storage_tx: tokio::sync::mpsc::Sender<Command>) -> Self {
        Self { storage_tx }
    }

    pub(crate) async fn store_http_history(
        &self,
        test_id: Option<TestId>,
        http_message: HttpMessage,
    ) -> ChannelResult<HistoryId> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::StoreHttpHistory {
                test_id,
                http_message,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn get_http_history(
        &self,
        history_id: HistoryId,
    ) -> ChannelResult<LookupResult<HttpMessage>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::GetHttpHistory { history_id, reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn create_history_search(&self) -> ChannelResult<HistorySearchId> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::CreateHistorySearch { reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn get_history_search_items(
        &self,
        history_search_id: HistorySearchId,
        start_index: u32,
        count: u32,
    ) -> ChannelResult<LookupResult<Vec<HistoryId>>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::GetHistorySearchItems {
                history_search_id,
                start_index,
                count,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn get_history_search_count(
        &self,
        history_search_id: HistorySearchId,
    ) -> ChannelResult<LookupResult<u32>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::GetHistorySearchCount {
                history_search_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn subscribe_history_search(
        &self,
        history_search_id: HistorySearchId,
    ) -> ChannelResult<tokio::sync::watch::Receiver<()>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::SubscribeHistorySearch {
                history_search_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn delete_history_search(
        &self,
        history_search_id: HistorySearchId,
    ) -> ChannelResult<LookupResult<()>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::DeleteHistorySearch {
                history_search_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn list_scenarios(&self) -> ChannelResult<Vec<ScenarioTreeNode>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::ListScenarios { reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn create_scenario(
        &self,
        parent: Option<ScenarioId>,
        user_defined_id: String,
        description: String,
        type_: ScenarioType,
    ) -> ChannelResult<Result<ScenarioId, CreateScenarioError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::CreateScenario {
                parent,
                user_defined_id,
                description,
                type_,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn lookup_scenario(
        &self,
        user_defined_id: String,
    ) -> ChannelResult<LookupResult<ScenarioId>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::LookupScenario {
                user_defined_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn move_scenario_before(
        &self,
        move_scenario_id: ScenarioId,
        before_scenario_id: ScenarioId,
    ) -> ChannelResult<Result<(), MoveScenarioError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::MoveScenarioBefore {
                move_scenario_id,
                before_scenario_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn move_scenario_after(
        &self,
        move_scenario_id: ScenarioId,
        after_scenario_id: ScenarioId,
    ) -> ChannelResult<Result<(), MoveScenarioError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::MoveScenarioAfter {
                move_scenario_id,
                after_scenario_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn move_scenario_inside(
        &self,
        move_scenario_id: ScenarioId,
        parent_scenario_id: ScenarioId,
    ) -> ChannelResult<Result<(), MoveScenarioError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::MoveScenarioInside {
                move_scenario_id,
                parent_scenario_id,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn delete_scenario(
        &self,
        scenario_id: ScenarioId,
    ) -> ChannelResult<Result<(), DeleteScenarioError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::DeleteScenario { scenario_id, reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn get_scenario_info(
        &self,
        scenario_id: ScenarioId,
    ) -> ChannelResult<LookupResult<ScenarioInfo>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::GetScenarioInfo { scenario_id, reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn update_scenario_info(
        &self,
        scenario_id: ScenarioId,
        user_defined_id: String,
        description: String,
        previous_modified_time: chrono::DateTime<chrono::Local>,
    ) -> ChannelResult<Result<(), UpdateScenarioInfoError>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::UpdateScenarioInfo {
                scenario_id,
                user_defined_id,
                description,
                previous_modified_time,
                reply,
            })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn subscribe_scenario(
        &self,
    ) -> ChannelResult<tokio::sync::broadcast::Receiver<ScenarioId>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::SubscribeScenario { reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }

    pub(crate) async fn subscribe_methodology(
        &self,
    ) -> ChannelResult<tokio::sync::watch::Receiver<()>> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::SubscribeMethodology { reply })
            .await?;
        reply_rx.await.map_err(|e| e.into())
    }
}

pub(crate) type ChannelResult<T> = std::result::Result<T, ChannelError>;

#[derive(Debug)]
pub(crate) enum ChannelError {
    Send,
    Receive,
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for ChannelError {
    fn from(_value: tokio::sync::mpsc::error::SendError<T>) -> Self {
        Self::Send
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for ChannelError {
    fn from(_value: tokio::sync::oneshot::error::RecvError) -> Self {
        ChannelError::Receive
    }
}
