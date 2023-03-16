# WARNING: This file is undergoing active design and development and will change in breaking,
#          incompatible ways.

@0x95c0d2ab067d5dd8;
# This signature must be unique to this file

interface Bearclaw {
	send @0 (connInfo :ConnectionInfo, request :Data) -> (response :Option(Data));
	# Create a new connection to `conn`, send the request, and return the response received.
	# If unable to connect or no response is received, returns 'Option::none'.
	# This should be changed to return a `Result` with the recorded error details.
	# The connection will only be used to send this single request.
	# The proxy may send the request using any HTTP protocol version, irregardless of the protocol
	# version specified in `request`.
	# This will be removed in the future in favor of having the client create their own connections
	# explicitly.

	searchHistory @1 () -> (result :Result(HistorySearch, SearchHistoryError));
	# Returns a list of all messages in the proxy history that match your search query (you can't
	# specify the query yet). You can subscribe to receive notifications when new history items are
	# created that match your query.

	getHistoryItem @2 (historyId :HistoryId) -> (result :Result(HttpMessage, GetHistoryItemError));

	buildInfo @3 () -> (buildInfo :BuildInfo);

	exit @4 ();
	# Shuts down the bearclaw proxy application
}

struct ConnectionInfo {
	host @0 :Text;
	port @1 :UInt16;
	isHttps @2 :Bool;
}

struct Option(T) {
# An optional value of type T.
	union {
		some @0 :T;
		none @1 :Void;
	}
}

struct Result(T, E) {
# A value of type T or an error of type E.

	union {
		ok @0 :T;
		err @1 :E;
	}
}

interface HistorySearch {
# Retrieves existing and future proxy history messages that match the search query specified when
# creating this object. You cannot change the search query on an existing object.

	getCount @0 () -> (count :UInt32);
	# Returns the number of proxy history items in the search results. This includes items added to
	# the proxy history after this object was created.

	getItems @1 (startIndex :UInt32, count :UInt32) -> (items :List(HistoryId));
	# Returns up to `count` items starting at `startIndex` in the search results. This includes items
	# added to the proxy history after this object was created. May return less than `count` items if
	# the range is at the end of the search results or if `count` is larger than the maximum allowed
	# value.

	subscribe @2 (subscriber :HistorySubscriber) -> (result :Result(Subscription, SubscribeError));
	# Receive a notification on `subscriber` when a new proxy history message becomes available that
	# matches the search query. You can obtain the new item from `getItems`. Note that it is possible
	# to receive notifications for items you have already accessed and there may be multiple new items
	# available by the time you receive a notification.
}

struct HistoryId {
	id @0 :Int64;
}

interface HistorySubscriber {
# A client implements this interface on an object that they want to receive new item notification
# callbacks.

	notifyNewItem @0 ();
}

interface Subscription {}
# A handle to a subscription for a callback. When this is deleted the subscription will be cancelled
# and no more callbacks will be received.

struct SubscribeError {
	rpcObjectLimitExceeded @0 :RpcObjectLimitType;
}

struct SearchHistoryError {
	rpcObjectLimitExceeded @0 :RpcObjectLimitType;
}

enum RpcObjectLimitType {
	search @0;
	subscription @1;
	historyItem @2;
}

interface HttpMessage {
	connectionInfo @0 () -> (connectionInfo :ConnectionInfo);
	requestTimestamp @1 () -> (requestTimestamp :Time);
	requestBytes @2 () -> (requestBytes :Data);
	responseTimestamp @3 () -> (responseTimestamp :Time);
	responseBytes @4 () -> (responseBytes :HttpResponse);
}

struct Time {
	secs @0 :Int64;
	# Number of non-leap seconds since January 1, 1970 (unix timestamp)
	nsecs @1 :UInt32;
	# Number of nanoseconds since the last second boundary
}

struct HttpResponse {
	union {
		ok @0 :Data;
		err @1 :HttpError;
	}
}

enum HttpError {
	dns @0;
	couldNotConnect @1;
	connectionClosed @2;
	responseTimeout @3;
	responseTooLarge @4;
}

struct GetHistoryItemError {
	rpcObjectLimitExceeded @0 :RpcObjectLimitType;
	notFound @1 :Void;
}

struct BuildInfo {
	version @0 :Text;
	isDirty @1 :Bool;
	buildTimestamp @2 :Text;
	gitBranch @3 :Option(Text);
	gitCommitCount @4 :Option(Text);
	gitCommitTimestamp @5 :Option(Text);
	gitSha @6 :Option(Text);
	rustcChannel @7 :Text;
	rustcCommitDate @8 :Text;
	rustcCommitHash @9 :Text;
	rustcHostTriple @10 :Text;
	rustcSemver @11 :Text;
	cargoFeatures @12 :Text;
	cargoProfile @13 :Text;
	cargoTargetTriple @14 :Text;
	dbEngineVersion @15 :Text;
}