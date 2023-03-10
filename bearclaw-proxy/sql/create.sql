-- Table and index creation
      
CREATE TABLE methodology (
    methodology_id           INTEGER PRIMARY KEY NOT NULL,
    parent_id                INTEGER NULL,
    position                 INTEGER NOT NULL,
    methodology_type_enum_id INTEGER NOT NULL,
    description              TEXT    NOT NULL,
    date_created             INTEGER NOT NULL,
    date_modified            INTEGER NOT NULL,
    UNIQUE (parent_id, position),
    FOREIGN KEY (parent_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (methodology_type_enum_id)
    REFERENCES methodology_type_enum (methodology_type_enum_id)
) STRICT;

CREATE TABLE methodology_type_enum (
    methodology_type_enum_id INTEGER PRIMARY KEY NOT NULL,
    description              TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE generic_test (
    methodology_id INTEGER NOT NULL,
    test_id        INTEGER UNIQUE NOT NULL,
    PRIMARY KEY (methodology_id, test_id),
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE test (
    test_id       INTEGER PRIMARY KEY NOT NULL,
    date_created  INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    description   TEXT    NOT NULL,
    is_done       INTEGER NOT NULL DEFAULT FALSE
) STRICT;

CREATE TABLE methodology_location (
    methodology_id            INTEGER NOT NULL,
    location_id               INTEGER NOT NULL,
    assignment_status_enum_id INTEGER NOT NULL,
    PRIMARY KEY (methodology_id, location_id),
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (location_id)
    REFERENCES location (location_id),
    FOREIGN KEY (assignment_status_enum_id)
    REFERENCES assignment_status_enum (assignment_status_enum_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE location (
    location_id   INTEGER PRIMARY KEY NOT NULL,
    is_visited    INTEGER NOT NULL DEFAULT FALSE,
    path          TEXT    NOT NULL,
    date_created  INTEGER NOT NULL,
    date_modified INTEGER NOT NULL
) STRICT;

CREATE TABLE assignment_status_enum (
    assignment_status_enum_id INTEGER PRIMARY KEY NOT NULL,
    description               TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE location_test (
    methodology_id INTEGER NOT NULL,
    location_id    INTEGER NOT NULL,
    test_id        INTEGER UNIQUE NOT NULL,
    PRIMARY KEY (methodology_id, location_id, test_id),
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (location_id)
    REFERENCES location (location_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE methodology_endpoint (
    methodology_id            INTEGER NOT NULL,
    endpoint_id               INTEGER NOT NULL,
    assignment_status_enum_id INTEGER NOT NULL,
    PRIMARY KEY (methodology_id, endpoint_id),
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (endpoint_id)
    REFERENCES endpoint (endpoint_id),
    FOREIGN KEY (assignment_status_enum_id)
    REFERENCES assignment_status_enum (assignment_status_enum_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE endpoint (
    endpoint_id       INTEGER PRIMARY KEY NOT NULL,
    path              TEXT    UNIQUE NOT NULL,
    message_format_id INTEGER NOT NULL,
    is_visited        INTEGER NOT NULL DEFAULT FALSE,
    date_created      INTEGER NOT NULL,
    date_modified     INTEGER NOT NULL,
    FOREIGN KEY (message_format_id)
    REFERENCES message_format (message_format_id)
) STRICT;

CREATE TABLE message_format (
    message_format_id INTEGER PRIMARY KEY NOT NULL,
    description       TEXT    NOT NULL
) STRICT;

CREATE TABLE endpoint_parameter (
    endpoint_id                     INTEGER NOT NULL,
    parameter_id                    INTEGER NOT NULL,
    endpoint_parameter_type_enum_id INTEGER NOT NULL,
    PRIMARY KEY (endpoint_id, parameter_id),
    FOREIGN KEY (endpoint_id)
    REFERENCES endpoint (endpoint_id),
    FOREIGN KEY (parameter_id)
    REFERENCES parameter (parameter_id),
    FOREIGN KEY (endpoint_parameter_type_enum_id)
    REFERENCES endpoint_parameter_type_enum (endpoint_parameter_type_enum_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE parameter (
    parameter_id           INTEGER PRIMARY KEY NOT NULL, 
    parameter_type_enum_id INTEGER NOT NULL,
    FOREIGN KEY (parameter_type_enum_id)
    REFERENCES parameter_type_enum (parameter_type_enum_id)
) STRICT;

CREATE TABLE parameter_type_enum (
    parameter_type_enum_id INTEGER PRIMARY KEY NOT NULL,
    description            TEXT    UNIQUE NOT NULL    
) STRICT;

CREATE TABLE parameter_path (
    parameter_id           INTEGER NOT NULL, 
    position               INTEGER NOT NULL, 
    parameter_name_id      INTEGER NOT NULL, 
    PRIMARY KEY (parameter_id, position),
    FOREIGN KEY (parameter_name_id)
    REFERENCES parameter_name (parameter_name_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE parameter_name (
    parameter_name_id      INTEGER PRIMARY KEY NOT NULL, 
    name                   TEXT    NOT NULL
) STRICT;

CREATE TABLE endpoint_parameter_type_enum (
    endpoint_parameter_type_enum_id INTEGER PRIMARY KEY NOT NULL,
    description                     TEXT    UNIQUE NOT NULL    
) STRICT;

CREATE TABLE endpoint_parameter_value (
    endpoint_id  INTEGER NOT NULL,
    parameter_id INTEGER NOT NULL,
    value        TEXT    NOT NULL,
    PRIMARY KEY (endpoint_id, parameter_id),
    FOREIGN KEY (endpoint_id)
    REFERENCES endpoint (endpoint_id),
    FOREIGN KEY (parameter_id)
    REFERENCES parameter (parameter_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE endpoint_test (
    methodology_id INTEGER NOT NULL,
    endpoint_id    INTEGER NOT NULL,
    test_id        INTEGER UNIQUE NOT NULL,
    PRIMARY KEY (methodology_id, endpoint_id, test_id),
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id),
    FOREIGN KEY (endpoint_id)
    REFERENCES endpoint (endpoint_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE methodology_role (
    methodology_role_id INTEGER PRIMARY KEY NOT NULL,
    methodology_id      INTEGER NOT NULL,
    description         TEXT    NOT NULL,
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id)
) STRICT;

CREATE TABLE authorization_status (
    methodology_role_id          INTEGER NOT NULL,
    location_id                  INTEGER NOT NULL,
    authorization_status_enum_id INTEGER NOT NULL,
    PRIMARY KEY (methodology_role_id, location_id),
    FOREIGN KEY (methodology_role_id)
    REFERENCES methodology_role (methodology_role_id),
    FOREIGN KEY (location_id)
    REFERENCES location (location_id),
    FOREIGN KEY (authorization_status_enum_id)
    REFERENCES authorization_status_enum (authorization_status_enum_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE authorization_status_enum (
    authorization_status_enum_id INTEGER PRIMARY KEY NOT NULL,
    description                  TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE authorization_test (
    methodology_role_id INTEGER NOT NULL,
    location_id         INTEGER NOT NULL,
    test_id             INTEGER UNIQUE NOT NULL,
    PRIMARY KEY (methodology_role_id, location_id, test_id),
    FOREIGN KEY (methodology_role_id)
    REFERENCES methodology_role (methodology_role_id),
    FOREIGN KEY (location_id)
    REFERENCES location (location_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE methodology_flow (
    methodology_flow_id INTEGER PRIMARY KEY NOT NULL,
    methodology_id      INTEGER NOT NULL,
    description         TEXT    NOT NULL,
    is_done             INTEGER NOT NULL DEFAULT FALSE,
    FOREIGN KEY (methodology_id)
    REFERENCES methodology (methodology_id)
) STRICT;

CREATE TABLE methodology_flow_endpoint (
    methodology_flow_id INTEGER NOT NULL,
    endpoint_id         INTEGER NOT NULL,
    PRIMARY KEY (methodology_flow_id, endpoint_id),
    FOREIGN KEY (methodology_flow_id)
    REFERENCES methodology_flow (methodology_flow_id),
    FOREIGN KEY (endpoint_id)
    REFERENCES endpoint (endpoint_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE business_logic_test (
    methodology_flow_id INTEGER NOT NULL,
    test_id             INTEGER UNIQUE NOT NULL,
    PRIMARY KEY (methodology_flow_id, test_id),
    FOREIGN KEY (methodology_flow_id)
    REFERENCES methodology_flow (methodology_flow_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE test_history (
    test_id    INTEGER NOT NULL,
    history_id INTEGER NOT NULL,
    is_done    INTEGER NOT NULL DEFAULT FALSE,
    PRIMARY KEY (test_id, history_id),
    FOREIGN KEY (test_id)
    REFERENCES test (test_id),
    FOREIGN KEY (history_id)
    REFERENCES history (history_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE history (
    -- This is AUTOINCREMENT because we depend on the id being montonically increasing
    history_id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    date_secs  INTEGER NOT NULL,
    date_nsecs INTEGER NOT NULL
) STRICT;

CREATE TABLE http_history (
    http_history_id        INTEGER PRIMARY KEY NOT NULL,
    connection_info_id     INTEGER NOT NULL,
    -- Split out the first two lines of the http request header since it is uncompressible and
    -- has little chance of being deduplicated, while the rest of the header has a high chance
    -- of being deduplicated. TODO: Just parse these first two lines and store them in SQL
    -- directly, since there's very little storage cost to doing this and as a benefit we can
    -- directly query against this data. It would probably make sense to do this with the first
    -- line of the response header too.
    request_location_id    INTEGER NOT NULL,
    request_header_id      INTEGER NOT NULL,
    -- request_body_id is stored in a different table since it is usually omitted
    response_date_secs     INTEGER NOT NULL,
    response_date_nsecs    INTEGER NOT NULL,
    response_header_id     INTEGER NOT NULL,
    -- The response body is usually present so we optimize for this case. If it's not present
    -- this field will reference a content record that decompresses into the empty string.
    response_body_id       INTEGER NOT NULL,
    FOREIGN KEY (http_history_id)
    REFERENCES history (history_id),
    FOREIGN KEY (connection_info_id)
    REFERENCES connection_info (connection_info_id),
    FOREIGN KEY (request_location_id)
    REFERENCES content (content_id),
    FOREIGN KEY (request_header_id)
    REFERENCES content (content_id),
    FOREIGN KEY (response_header_id)
    REFERENCES content (content_id),
    FOREIGN KEY (response_body_id)
    REFERENCES content (content_id)
) STRICT;

CREATE TABLE connection_info (
    connection_info_id INTEGER PRIMARY KEY NOT NULL,
    host_id            INTEGER NOT NULL,
    port               INTEGER NOT NULL,
    is_https           INTEGER NOT NULL,
    UNIQUE (host_id, port, is_https),
    FOREIGN KEY (host_id)
    REFERENCES host (host_id)
) STRICT;

CREATE TABLE host (
    host_id INTEGER PRIMARY KEY NOT NULL,
    host    TEXT    UNIQUE NOT NULL
) STRICT;

-- This should not be WITHOUT ROWID because it will contain large rows
CREATE TABLE content (
    content_id INTEGER PRIMARY KEY NOT NULL,
    hash       INTEGER UNIQUE NOT NULL,
    data       BLOB    NOT NULL
) STRICT;

CREATE UNIQUE INDEX idx_content_hash ON content (hash);

CREATE TABLE http_history_request_body (
    http_history_id INTEGER NOT NULL,
    request_body_id INTEGER NOT NULL,
    PRIMARY KEY (http_history_id, request_body_id),
    FOREIGN KEY (http_history_id)
    REFERENCES http_history (http_history_id),
    FOREIGN KEY (request_body_id)
    REFERENCES content (content_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE http_error (
    http_history_id    INTEGER NOT NULL,
    http_error_enum_id INTEGER NOT NULL,
    PRIMARY KEY (http_history_id, http_error_enum_id),
    FOREIGN KEY (http_history_id)
    REFERENCES http_history (http_history_id),
    FOREIGN KEY (http_error_enum_id)
    REFERENCES http_error_enum (http_error_enum_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE http_error_enum (
    http_error_enum_id INTEGER PRIMARY KEY NOT NULL,
    description        TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE history_browser_plugin_data (
    history_id             INTEGER PRIMARY KEY NOT NULL,
    tab_hash               INTEGER NOT NULL,
    browser_url_id         INTEGER NOT NULL,
    request_source_enum_id INTEGER NOT NULL,
    FOREIGN KEY (history_id)
    REFERENCES history (history_id),
    FOREIGN KEY (browser_url_id)
    REFERENCES browser_url (browser_url_id),
    FOREIGN KEY (request_source_enum_id)
    REFERENCES request_source_enum (request_source_enum_id)
) STRICT;

CREATE TABLE browser_url (
    browser_url_id INTEGER PRIMARY KEY NOT NULL,
    browser_url    TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE request_source_enum (
    request_source_enum_id INTEGER PRIMARY KEY NOT NULL,
    description            TEXT    UNIQUE NOT NULL
) STRICT;

CREATE TABLE websocket_history (
    websocket_history_id        INTEGER PRIMARY KEY NOT NULL,
    originating_http_history_id INTEGER NOT NULL,
    websocket_direction_enum_id INTEGER NOT NULL,
    -- the message header is very short and wouldn't benefit from deduplication or compression
    message_header              BLOB    NOT NULL,
    -- This is stored in unmsaked format to improve deduplication and compression. The masking
    -- key is stored in message_header and so the original payload on-the-wire can be
    -- faithfully reconstructed.
    message_payload_id          INTEGER NOT NULL,
    FOREIGN KEY (websocket_history_id)
    REFERENCES history (history_id),
    FOREIGN KEY (originating_http_history_id)
    REFERENCES http_history (http_history_id),
    FOREIGN KEY (websocket_direction_enum_id)
    REFERENCES websocket_direction_enum (websocket_direction_enum_id),
    FOREIGN KEY (message_payload_id)
    REFERENCES content_group (content_group_id)
) STRICT;

CREATE TABLE websocket_direction_enum (
    websocket_direction_enum_id INTEGER PRIMARY KEY NOT NULL,
    description                 TEXT    UNIQUE NOT NULL
) STRICT;

-- Enumerations
-- These must be manually kept in sync with rust code

INSERT INTO
    methodology_type_enum
VALUES
    (1, 'Unassigned Proxy Traffic'),
    (2, 'Container'),
    (3, 'Generic Testing'),
    (4, 'Location Testing'),
    (5, 'Endpoint Testing'),
    (6, 'Authorization Testing'),
    (7, 'Business Logic Testing');

INSERT INTO
    assignment_status_enum
VALUES
    (1, 'Not Applicable'),
    (2, 'Assigned'),
    (3, 'Complete');

INSERT INTO
    authorization_status_enum
VALUES
    (1, 'Untested'),
    (2, 'Authorized'),
    (3, 'Unauthorized');

INSERT INTO
    message_format
VALUES
    (1, 'HTTP'),
    (2, 'Web Socket');

INSERT INTO
    parameter_type_enum
VALUES
    (1, 'HTTP Header'),
    (2, 'HTTP Cookie'),
    (3, 'HTTP Url'),
    (4, 'HTTP Body'),
    (5, 'Non-HTTP');

INSERT INTO
    endpoint_parameter_type_enum
VALUES
    (1, 'Variable'),
    (2, 'Location'),
    (3, 'Ignored');

INSERT INTO
    request_source_enum
VALUES
    (1, 'Unknown'),
    (2, 'User Interaction'),
    (3, 'Javascript');

INSERT INTO
    http_error_enum
VALUES
    (1, 'Dns'),
    (2, 'CouldNotConnect'),
    (3, 'ConnectionClosed'),
    (4, 'ResponseTimeout'),
    (5, 'ResponseTooLarge');

INSERT INTO
    websocket_direction_enum
VALUES
    (1, 'ClientToServer'),
    (2, 'ServerToClient');

-- Special Values

INSERT INTO
    methodology
(
    methodology_id,
    parent_id,
    position,
    methodology_type_enum_id,
    description,
    date_created,
    date_modified                
)
VALUES
    (1, NULL, 0, 1, "Unassigned Proxy Traffic", 0, 0);    

INSERT INTO
    test
(
    test_id,
    date_created,
    date_modified,
    description,
    is_done    
)
VALUES
    (1, 0, 0, "Unassigned Proxy Traffic", FALSE);    
