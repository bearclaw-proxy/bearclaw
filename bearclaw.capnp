# WARNING: This file is undergoing active design and development and will change in breaking,
#          incompatible ways.

@0xfd91532fe1094bdd;
# This signature must be unique to this file

interface Bearclaw {
	send @2 (connInfo :ConnectionInfo, request :Data)
		-> (response :Option(Data));
	# Create a new connection to `conn`, send the request, and return the response received.
	# If unable to connect or no response is received, returns 'Option::none'.
	# This should be changed to return a `Result` with the recorded error details.
	# The connection will only be used to send this single request.
	# The proxy may send the request using any HTTP protocol version, irregardless of the protocol
	# version specified in `request`.
	# This will be removed in the future in favor of having the client create their own connections
	# explicitly.

	searchHistory @3 ()
		-> (historySearch :HistorySearch);
	# Returns a list of all messages in the proxy history that match your search query (you can't
	# specify the query yet). You can subscribe to receive notifications when new history items are
	# created that match your query.

	getHistoryItem @4 (historyId :HistoryId)
		-> (result :Result(HttpMessage, GetHistoryItemError));

	createScenario @5 (info :NewScenarioInfo)
		-> (result :Fallible(CreateScenarioError));

	getScenario @6 (scenarioId :Text)
		-> (result :Result(Scenario, GetScenarioError));

	listScenarios @7 ()
		-> (list :List(ScenarioTreeNode));

	subscribeMethodology @8 (subscriber :MethodologySubscriber)
		-> (subscription :Subscription);

	exit @1 ();
	# Shuts down the bearclaw proxy application

	getBuildInfo @0 ()
		-> (buildInfo :BuildInfo);
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

	subscribe @2 (subscriber :HistorySubscriber) -> (subscription :Subscription);
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
	notFound @0 :Void;
}

struct ScenarioTreeNode {
	info @0 :ScenarioInfo;
	children @1 :List(ScenarioTreeNode);
}

struct ScenarioInfo {
	id @0 :Text;
	description @1 :Text;
	type @2 :ScenarioType;
	createdTimestamp @3 :Time;
	modifiedTimestamp @4 :Time;
}

enum ScenarioType {
	container @0;
	generic @1;
	location @2;
	endpoint @3;
	authorization @4;
	businessLogic @5;
}

struct NewScenarioInfo {
	id @0 :Text;
	description @1 :Text;
	type @2 :ScenarioType;
}

struct CreateScenarioError {
	union {
		idAlreadyExists @0 :Void;
		scenarioLimitExceeded @1 :Void;
	}
}

struct GetScenarioError {
	notFound @0 :Void;
}

interface MethodologySubscriber {
	notifyScenarioTreeChanged @0 ();
	# A scenario was created, updated, moved, or deleted
}

interface Scenario {
	getInfo @0 ()
		-> (result :Result(ScenarioInfo, ScenarioGetInfoError));

	updateInfo @1 (info: UpdateScenarioInfo)
		-> (result :Fallible(ScenarioUpdateInfoError));
	# Updates the scenario with `info`. `info` must have been returned by a previous call to
	# `getInfo`. An error is returned if the scenario has been updated by another script. This is
	# determined by comparing the modification timestamp in `info` to the modification timestamp of
	# the scenario.

	subscribeScenario @2 (subscriber :ScenarioSubscriber)
		-> (subscription :Subscription);

	createChildScenario @3 (info :NewScenarioInfo)
		-> (result :Fallible(ScenarioCreateChildScenarioError));

	moveBefore @4 (before :Scenario)
		-> (result :Fallible(ScenarioMoveError));

	moveAfter @5 (after :Scenario)
		-> (result :Fallible(ScenarioMoveError));

	moveInside @6 (parent :Scenario)
		-> (result :Fallible(ScenarioMoveError));

	delete @7 ()
		-> (result :Fallible(ScenarioDeleteError));
	# Delete the scenario and all of its child scenarios. Scenarios with testing data cannot be
	# deleted. Instead, you can create a container scenario for unwanted scenarios and move this
	# scenario inside of it. No scenarios are deleted if an error occurs, even if some child scenarios
	# could have been successfully deleted.
}

struct ScenarioGetInfoError {
	scenarioDeleted @0 :Void;
}

struct Fallible(E) {
# A potential error of type E.

	union {
		success @0 :Void;
		fail @1 :E;
	}
}

struct UpdateScenarioInfo {
	id @0 :Text;
	description @1 :Text;
	previousModifiedTimestamp @2 :Time;
	# The modified timestamp from the existing scenario info. This is used to detect if another script
	# has updated the scenario since you last downloaded the scenario info.
}

struct ScenarioUpdateInfoError {
	union {
		scenarioDeleted @0 :Void;
		scenarioUpdatedBySomeoneElse @1 :Void;
		# The scenario has been updated by another script since you downloaded the scenario info. To force
		# the update, get the scenario info again (with the updated modification timestamp) and resubmit
		# your changes.
		idAlreadyExists @2 :Void;
	}
}

interface ScenarioSubscriber {
	notifyScenarioUpdated @0 ();
}

struct ScenarioSubscribeScenarioError {
	scenarioDeleted @0 :Void;
}

struct ScenarioCreateChildScenarioError {
	union {
		scenarioDeleted @0 :Void;
		idAlreadyExists @1 :Void;
		maxScenarioTreeDepthExceeded @2 :Void;
		scenarioLimitExceeded @3 :Void;
	}
}

struct ScenarioMoveError {
	union {
		thisScenarioDeleted @0 :Void;
		targetScenarioDeleted @1 :Void;
		targetScenarioIsChildOfThisScenario @2 :Void;
		# You cannot move a scenario inside of itself
		maxScenarioTreeDepthExceeded @3 :Void;
	}
}

struct ScenarioDeleteError {
	union {
		scenarioAlreadyDeleted @0 :Void;
		scenarioHasTestingData :group {
			scenarioId @1 :Text;
		}
		# You cannot delete a scenario that has been used for testing. Instead, you can create a scenario
		# container for unwanted, used scenarios.
	}
}

struct BuildInfo {
	version @0 :Text;
	isDirty @1 :Bool;
	buildTimestamp @2 :Text;
	gitInfo @3 :Option(GitInfo);
	rustCompilerInfo @4 :RustCompilerInfo;
	cargoInfo @5 :CargoInfo;
	libraryInfo @6 :LibraryInfo;
}

struct GitInfo {
	branch @0 :Text;
	commitCount @1 :Text;
	commitTimestamp @2 :Text;
	sha @3 :Text;
}

struct RustCompilerInfo {
	channel @0 :Text;
	commitDate @1 :Text;
	commitHash @2 :Text;
	hostTriple @3 :Text;
	semver @4 :Text;
}

struct CargoInfo {
	features @0 :Text;
	profile @1 :Text;
	targetTriple @2 :Text;
}

struct LibraryInfo {
	dbEngine @0 :Text;
	compressionEngine @1 :Text;
}