use std::path::Path;

use bytes::{Buf, BufMut};
use chrono::TimeZone;
use rusqlite::OptionalExtension;

/// Lets us identify if we created the file
const APPLICATION_ID: u32 = 0xb34c14;
/// Updated whenever our schema changes. Will be used to know when to migrate database versions.
const FILE_FORMAT_VERSION: i64 = 1;
// Let's start with the highest compression level, we can scale it back later if it causes problems
const ZSTD_COMPRESSION_LEVEL: i32 = 22;
// NOTE: Check to see if you should update this if you add any SQL statements
const PREPARED_STATEMENT_CACHE_CAPACITY: usize = 32;
const LOAD_HTTP_REQUEST_INITIAL_BUFFER_CAPACITY: usize = 512;
const LOAD_HTTP_RESPONSE_INITIAL_BUFFER_CAPACITY: usize = 1024;

pub(super) struct Database {
    db: rusqlite::Connection,
    storage_stats: StorageStats,
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

        Ok(Self {
            db,
            storage_stats: Default::default(),
        })
    }

    /// The storage message processing loop. It is intended to run in its own thread.
    pub(super) fn run(
        mut self,
        mut storage_rx: tokio::sync::mpsc::Receiver<Command>,
    ) -> RunResult<()> {
        // This will exit when all senders are destroyed
        while let Some(cmd) = storage_rx.blocking_recv() {
            match cmd {
                Command::StoreHttpHistory {
                    test_id,
                    http_message,
                    reply,
                } => {
                    let history_id = self.store_http_history(test_id, http_message)?;
                    reply
                        .send(HistoryId(history_id))
                        .map_err(|_| RunError::Channel)?;
                }
                Command::GetHttpHistory {
                    http_history_id,
                    reply,
                } => {
                    let http_message = self.get_http_history(http_history_id)?;
                    reply.send(http_message).map_err(|_| RunError::Channel)?;
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

        Ok(())
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn store_http_history(
        &mut self,
        test_id: Option<TestId>,
        http_message: HttpMessage,
    ) -> RunResult<i64> {
        let txn = self.db.transaction()?;
        let mut storage_stats = Default::default();

        tracing::trace!("Creating history record");
        let history_id = {
            let mut insert =
                txn.prepare_cached("INSERT INTO history (date_secs, date_nsecs) VALUES (?,?)")?;
            insert.execute((
                http_message.request_time.timestamp(),
                http_message.request_time.timestamp_subsec_nanos(),
            ))?;

            txn.last_insert_rowid()
        };

        tracing::trace!("Creating test_history record");
        {
            // Test ID 1 is guaranteed to exist, it holds proxied requests
            let test_id = test_id.unwrap_or(TestId(1)).0;
            let mut insert =
                txn.prepare_cached("INSERT INTO test_history (test_id, history_id) VALUES (?,?)")?;
            insert.execute((test_id, history_id))?;
        }

        let (request_location_id, request_header_id, opt_request_body_id) =
            store_http_request(&txn, &http_message.request, &mut storage_stats)?;

        let (response_header_id, response_body_id) =
            if let Ok(http_response) = &http_message.response {
                store_http_response(&txn, http_response, &mut storage_stats)?
            } else {
                let empty_content = store_content(&txn, b"", &mut storage_stats)?;

                (empty_content, empty_content)
            };

        let connection_info_id = {
            let host_id = {
                let mut query = txn.prepare_cached("SELECT host_id FROM host WHERE host = ?")?;

                if let Some(host_id) = query
                    .query_row((&http_message.host,), |row| row.get(0))
                    .optional()?
                {
                    tracing::trace!("Using existing host_id record");

                    host_id
                } else {
                    tracing::trace!("Creating host_id record");
                    let mut insert = txn.prepare_cached("INSERT INTO host (host) VALUES (?)")?;
                    insert.execute((&http_message.host,))?;

                    txn.last_insert_rowid()
                }
            };

            let mut query = txn.prepare_cached(
                "SELECT connection_info_id
                FROM connection_info
                WHERE host_id = ? AND port = ? AND is_https = ?",
            )?;

            if let Some(connection_info_id) = query
                .query_row((host_id, http_message.port, http_message.is_https), |row| {
                    row.get(0)
                })
                .optional()?
            {
                tracing::trace!("Using existing connection_info record");

                connection_info_id
            } else {
                tracing::trace!("Creating connection_info record");
                let mut insert = txn.prepare_cached(
                    "INSERT INTO
                        connection_info
                    (host_id, port, is_https)
                    VALUES
                        (?,?,?)",
                )?;
                insert.execute((host_id, http_message.port, http_message.is_https))?;

                txn.last_insert_rowid()
            }
        };

        {
            tracing::trace!("Creating http_history record");
            let mut insert = txn.prepare_cached(
                "INSERT INTO
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
                    (?,?,?,?,?,?,?,?)",
            )?;
            insert.execute((
                history_id,
                connection_info_id,
                request_location_id,
                request_header_id,
                http_message.response_time.timestamp(),
                http_message.response_time.timestamp_subsec_nanos(),
                response_header_id,
                response_body_id,
            ))?;
        }

        if let Some(request_body_id) = opt_request_body_id {
            tracing::trace!("Creating http_history_request_body record");
            let mut insert = txn.prepare_cached(
                "INSERT INTO
                    http_history_request_body
                (
                    http_history_id,
                    request_body_id
                )
                VALUES
                    (?,?)",
            )?;
            insert.execute((history_id, request_body_id))?;
        }

        if let Err(error) = http_message.response {
            tracing::trace!("Creating http_error record");
            let mut insert = txn.prepare_cached(
                "INSERT INTO http_error (http_history_id, http_error_enum_id) VALUES (?,?)",
            )?;
            let error_code: u8 = error.into();
            insert.execute((history_id, error_code))?;
        }

        txn.commit()?;

        tracing::debug!(storage_stats.bytes_total);
        tracing::debug!(storage_stats.bytes_deduplicated);
        tracing::debug!(storage_stats.bytes_stored);

        self.storage_stats.bytes_total += storage_stats.bytes_total;
        self.storage_stats.bytes_deduplicated += storage_stats.bytes_deduplicated;
        self.storage_stats.bytes_stored += storage_stats.bytes_stored;

        Ok(history_id)
    }

    #[tracing::instrument(level = "trace", skip_all)]
    fn get_http_history(&mut self, http_history_id: HistoryId) -> RunResult<HttpMessage> {
        let txn = self.db.transaction()?;
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
            let mut query = txn.prepare_cached(
                "SELECT
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
                    http_history.http_history_id = ?",
            )?;

            query.query_row((http_history_id.0,), |row| {
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
            })?
        };

        let request = load_http_request(
            &txn,
            request_location_id,
            request_header_id,
            opt_request_body_id,
        )?;
        let response = if let Some(http_error_enum_id) = opt_http_error_enum_id {
            let err = HttpError::try_from(http_error_enum_id).unwrap();
            Err(err)
        } else {
            Ok(load_http_response(
                &txn,
                response_header_id,
                response_body_id,
            )?)
        };

        txn.commit()?;

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
    db.pragma_update(None, "application_id", APPLICATION_ID)?;
    db.pragma_update(None, "user_version", FILE_FORMAT_VERSION)?;

    let txn = db.transaction()?;
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
        http_history_id: HistoryId,
        reply: tokio::sync::oneshot::Sender<HttpMessage>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TestId(i64);

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
pub(crate) struct HistoryId(i64);

pub(super) type RunResult<T> = std::result::Result<T, RunError>;

#[derive(Debug)]
pub(super) enum RunError {
    Database(rusqlite::Error),
    IO(std::io::Error),
    Channel,
}

impl From<rusqlite::Error> for RunError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}

impl From<std::io::Error> for RunError {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
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
) -> RunResult<(i64, i64, Option<i64>)> {
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

    let result = if let Some(index) = index {
        let request_location_id = store_content(txn, &http_request[0..index], storage_stats)?;

        let (request_header_id, request_body_id) = if index < http_request.len() {
            store_http_request_remainder(txn, &http_request[index..], storage_stats)?
        } else {
            (store_content(txn, b"", storage_stats)?, None)
        };

        (request_location_id, request_header_id, request_body_id)
    } else {
        let request_location_id = store_content(txn, b"", storage_stats)?;
        let request_header_id = store_content(txn, http_request, storage_stats)?;

        (request_location_id, request_header_id, None)
    };

    Ok(result)
}

fn store_http_request_remainder(
    txn: &rusqlite::Transaction,
    http_request: &[u8],
    storage_stats: &mut StorageStats,
) -> RunResult<(i64, Option<i64>)> {
    // Store the header and response body in two separate chunks, so duplicate bodies can be
    // deduplicated.

    let result = if let Some(index) = http_request.windows(4).position(|w| w == b"\r\n\r\n") {
        let index = index + 4;
        let request_header_id = store_content(txn, &http_request[0..index], storage_stats)?;
        let request_body_id = if index < http_request.len() {
            Some(store_content(txn, &http_request[index..], storage_stats)?)
        } else {
            None
        };

        (request_header_id, request_body_id)
    } else {
        (store_content(txn, http_request, storage_stats)?, None)
    };

    Ok(result)
}

#[tracing::instrument(level = "trace", skip_all)]
fn store_content(
    txn: &rusqlite::Transaction,
    content: &[u8],
    storage_stats: &mut StorageStats,
) -> RunResult<i64> {
    let len = content.len() as u64;
    let hash = hash_bytes(content);
    let mut query = txn.prepare_cached("SELECT content_id FROM content WHERE hash = ?")?;

    storage_stats.bytes_total += len;

    let result = if let Some(content_id) = query.query_row((hash,), |row| row.get(0)).optional()? {
        tracing::trace!("Using existing content record");
        storage_stats.bytes_deduplicated += len;

        content_id
    } else {
        tracing::trace!("Creating content record");
        //let content_str = String::from_utf8_lossy(content);
        //tracing::trace!("    Storing content: {content_str:?}");

        let compressed_data = zstd::bulk::compress(content, ZSTD_COMPRESSION_LEVEL)?;

        let compressed_len = compressed_data.len() as u64;
        storage_stats.bytes_stored += compressed_len;

        if compressed_len > len {
            tracing::debug!(
                "Compression resulted in increase from {len} to {compressed_len} bytes"
            );
        }

        let mut insert = txn.prepare_cached("INSERT INTO content (hash, data) VALUES (?,?)")?;
        insert.execute((hash, compressed_data))?;

        txn.last_insert_rowid()
    };

    Ok(result)
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
) -> RunResult<(i64, i64)> {
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

    let header_id = store_content(txn, header, storage_stats)?;
    let body_id = store_content(txn, body, storage_stats)?;

    Ok((header_id, body_id))
}

#[tracing::instrument(level = "trace", skip_all)]
fn load_http_request(
    txn: &rusqlite::Transaction,
    request_location_id: i64,
    request_header_id: i64,
    opt_request_body_id: Option<i64>,
) -> RunResult<bytes::Bytes> {
    let mut bytes = bytes::BytesMut::with_capacity(LOAD_HTTP_REQUEST_INITIAL_BUFFER_CAPACITY);

    load_content(txn, request_location_id, &mut bytes)?;
    load_content(txn, request_header_id, &mut bytes)?;
    if let Some(request_body_id) = opt_request_body_id {
        load_content(txn, request_body_id, &mut bytes)?;
    }

    Ok(bytes.freeze())
}

fn load_content(
    txn: &rusqlite::Transaction,
    content_id: i64,
    bytes: &mut bytes::BytesMut,
) -> RunResult<()> {
    let mut query = txn.prepare_cached("SELECT data FROM content WHERE content_id = ?")?;
    let compressed_data: Vec<u8> = query.query_row((content_id,), |row| row.get(0))?;

    zstd::stream::copy_decode(compressed_data.reader(), bytes.writer())?;

    Ok(())
}

#[tracing::instrument(level = "trace", skip_all)]
fn load_http_response(
    txn: &rusqlite::Transaction,
    response_header_id: i64,
    response_body_id: i64,
) -> RunResult<bytes::Bytes> {
    let mut bytes = bytes::BytesMut::with_capacity(LOAD_HTTP_RESPONSE_INITIAL_BUFFER_CAPACITY);

    load_content(txn, response_header_id, &mut bytes)?;
    load_content(txn, response_body_id, &mut bytes)?;

    Ok(bytes.freeze())
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
        http_history_id: HistoryId,
    ) -> ChannelResult<HttpMessage> {
        let (reply, reply_rx) = tokio::sync::oneshot::channel();
        self.storage_tx
            .send(Command::GetHttpHistory {
                http_history_id,
                reply,
            })
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
