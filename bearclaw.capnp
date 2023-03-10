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

	intercept @1 (subscriber :InterceptedMessageSubscriber) -> (subscription :Subscription);
	# Receive a callback on `subscriber` when a message is intercepted by the proxy. The message cannot
	# be edited by the client and includes both the request and the response. The callbacks will
	# stop when the returned `subscription` is destroyed.

	buildInfo @2 () -> (buildInfo :BuildInfo);

	exit @3 ();
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

interface InterceptedMessageSubscriber {
# A client implements this interface on an object that they want to receive intercepted message
# callbacks.
# This should be converted to a generic interface once
# https://github.com/capnproto/pycapnp/issues/225 is fixed.
	pushMessage @0 (message: InterceptedMessage) -> ();
}

interface InterceptedMessage {
	connectionInfo @0 () -> (conn :ConnectionInfo);
	requestTimestamp @1 () -> (time :Time);
	requestBytes @2 () -> (request :Data);
	responseTimestamp @3 () -> (time :Time);
	responseBytes @4 () -> (response :HttpResponse);
	# May not contain a response if there was an error forwarding the request. This should be changed
	# to return a `Result` with the recorded error details.
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

interface Subscription {}
# A handle to a subscription for a callback. When this is deleted the subscription will be cancelled
# and no more callbacks will be received.

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