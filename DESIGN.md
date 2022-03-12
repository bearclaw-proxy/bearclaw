## User Story

The user starts the bearclaw UI and creates a new bearclaw project directory. This starts a
bearclaw proxy in a separate process. The bearclaw proxy listens on default or user-specified
ports for RPC and the HTTP(S) proxy.

A new root certificate is generated if this is the first time the user has started bearclaw.

Two new files are created in the project directory: a bearclaw project file and a python script
that connects to the bearclaw proxy RPC port and registers callbacks.

The user defines what methodology they want to use. A methodology is an ordered list of tests
to be performed. The purpose of the methodology is to keep the user organized and to track what
still needs to be done.

All testing in bearclaw is performed against a step in the methodology (referred to as a
methodology node), a specific task assigned to that methodology node (e.g. a location in the
sitemap), and a user-defined description of the specific test they are performing.

For example:

- Methodology node: Reflected cross-site scripting
  - Assigned task: Endpoint at https://example.com/target1.php
    - Test: Look for reflected parameter values
    - Test: Determine if parameter 'foo' is vulnerable
  - Assigned task: Endpoint at https://example.com/target2.php
    - Test: Look for reflected parameter values
    - Test: Determine if parameter 'bar' is vulnerable

The bearclaw UI offers to optionally create a new methodology based on the
[OWASP WSTG](https://owasp.org/www-project-web-security-testing-guide/). The user can add or
remove methodology nodes from the UI or from a python script. The methodology nodes form a tree.

When adding a methodology node, the user specifies the type of testing that will be performed for
that node. This determines what can be assigned to the node and its UI.

The options are:

- *Location Testing*: A location is a directory or file on a server. Locations are assigned to the
  node and testing is performed for each location.
- *Endpoint Testing*: An endpoint is a location and possibly a set of parameter values that
  uniquely identify a single unit of server-side logic. An endpoint includes a list of parameters
  that provide input to the logic. Endpoints are assigned to the node and testing is performed for
  each endpoint.
- *Authorization Testing*: Locations are assigned to the node. The user defines a list of roles and
  testing is performed for each combination of assigned location and role.
- *Business Logic Testing*: Endpoints are assigned to the node. The user groups endpoints into
  one or more ordered multi-step flows and performs testing on each flow as a unit.
- *Generic Testing*: A single logical task. Nothing can be assigned to the node. Testing is
  performed for the node.
- *None*: The node is only used as a container in the methodology tree for child nodes. Nothing can
  be assigned to the node. Testing cannot be performed for the node.

The user opens the generated python script in their favorite editor and adds functionality
to perform any of the following tasks on each request as it passes through the proxy:

- Before the request is sent to the server:
  - Determine if the request is in scope
  - Modify the request
- Before the response is sent to the client:
  - Modify the response
  - Determine if the request is calling an endpoint that runs code on the server. If it is:
    - For each parameter in the request, identify whether the parameter is a:
      - meaningful input to the server side functionality
      - location that is used to designate server side functionality,
      - not meaningful (e.g. a timestamp).
  - Determine if the request location should be added to the sitemap
    - Might ignore requests that return 404 or redirects, for example
- Custom encodings
  - If there is a message or parameter encoding that is not supported by bearclaw, the user can
    define a custom one here that implements identify, decode, and encode functions. These
    will get called automatically by the proxy as needed.

The user runs this python script.

This python script is special because the proxy requires it to run for each request to know how
to process the request. The bearclaw proxy stops proxying requests if this script stops running.
This script should not take a long time to perform each task and should not do any other kinds
of tasks. Only a single instance of this script can be running at a time.

The user is expected to run additional, separate python scripts or REPLs that do not do this
special per-request processing. These scripts can do everything that the UI can do. Any number
of additional scripts can be running at the same time. They are not required to be running and
can be started and stopped at any time.

The user configures their browser to connect to the bearclaw HTTP(S) proxy. If needed, the user
installs the bearclaw root certificate into the browser trust store.

The user browses the target website in the browser. The proxy stores the requests and responses
that are in scope in the project file.

The user then switches to the bearclaw UI.

The user looks at the proxy history view to see a list of all in-scope requests that have passed
through the proxy. The user clicks on some requests to drill down into the details showing the
contents of the request and response and the analysis the main python script has done.

The user switches to the sitemap view. This shows each location in all in-scope targets in a tree.
For each location in the sitemap, this shows the number of:

- methodology nodes that the user needs to decide if it should be assigned to
- methodology nodes it is assigned to but has not been completed
- issues the user has identified

For each location that has not been assigned, the user determines which, if any, methodology nodes
it should be assigned to.

The user switches to the endpoint list view. For each endpoint, this shows the number of:

- methodology nodes that the user needs to decide if it should be assigned to
- methodology nodes it is assigned to but has not been completed
- issues the user has identified

For each endpoint that has not been assigned, the user determines which, if any, methodology nodes
it should be assigned to.

The user switches to the methodology list view. This shows each methodology node and the number of:

- recently discovered locations or endpoints that the user needs to assign or ignore
- items that need to be tested
- issues the user has identified

The user drills down into a methodology node from this view.

On the methodology detail view, the user sees a list of recently discovered locations or endpoints
that can be assigned to the methodology node. They mark each one as either assigned or inapplicable.

Additionally, there is a list of items that the user can test. The layout of this screen depends
on the type of testing the methodology node is configured for. The user can mark these items
as complete or drill down into them. The user drills down into one of these items to enter the
testing list view.

The testing list view asks the user to either create a new test by typing a description of the
testing they intend to perform, or selecting a previously created test from a list. This then takes
the user to the testing detail view.

From the testing detail view, the user can perform testing in the following ways:

- Click a button to capture proxy traffic for the current test. The user can then test from the
  browser.
- Manually modify and re-issue requests that were previously captured by the the proxy.
- Programmatically modify and re-issue requests from a python script
- Programmatically create a temporary HTTP(S) proxy port for this test. This can be used to run an
  external tool through the proxy, e.g. by setting an environment variable or using proxychains,
  and have its requests be assigned to this test. 

All testing that is performed in these ways is visible in the testing detail view, similar to the
proxy history view.

It is intended that the user perform simple manual exploration from the browser or by re-issuing
requests from the UI, and thorough testing such as running wordlists through parameters from a
python script. The user might create their own library of python scripts for different types of
testing they commonly do.

The python scripts can evaluate each request/response that it generates and tag it or any of its
components that it finds interesting. These tags are visible in the UI and can help draw the
user's attention to interesting things.

After performing testing, the user can select requests and create issues from them.

Additionally, the user can enter testing notes on a specific request, the testing detail view, and
the methodology detail view.

Once the user is finished testing this assigned item in the methodology node, they go back to the
methodology detail view and mark the item as complete, then move on to the next assigned item or
another methodology node.

As the user continues to test the target application, new locations or endpoints may be identified.
The application keeps track of these so the user doesn't miss anything.

There are additional views the user can access to analyze the data collected by the proxy:

- *Parameter view*: Explore the parameters seen by the application, their values, and where they
  occured.
- *Issue view*: All issues created by the user.
- *Notes view*: Everything that the user has entered testing notes on.

All views in the application support extensive sorting and filtering and allow the user to drill
down into related data.

Finally, the user can generate a report showing what was tested.
 
## Architecture

The application is written in [rust](https://doc.rust-lang.org/stable/book/). This is the language
I'm most comfortable with. It has good safety and performance properties. It also has decent OS
support.

The application should be designed to be cross platform, although I only intend to create linux
releases myself.

The UI toolkit is [GTK4](https://gtk-rs.org/gtk4-rs/stable/latest/book/). This seems to be the best
supported toolkit in rust. Version 4 because it has scalable lists which should help with
performance and memory usage since our lists could have a large number of rows. The toolkit is
native on linux and supported on Windows and MacOS.

While having a non-native UI on non-linux platforms is a poor user experience, I don't have the
capacity nor the desire to maintain native UIs for those platforms. I also don't want a
browser-based UI; while it is inherently cross-platform, it is non-native on all platforms, has
performance, resource usage, and UX cost, and is much more complex. In addition, I don't want to use
a native rust toolkit; while they look promising, I don't believe they are mature enough for use and
don't want to spend my time improving them for this project.

The default user scripting language is Python. Python is popular, offers a large standard library,
and has a huge ecosystem of packages. We need to be able to leverage existing work and quickly
write scripts for whatever situation we encounter so having a thriving ecosystem is a necessity.
While Python is the default, users are free to use their favorite scripting language, although they
will have to figure out how to get it to work themselves.

Currently, the proxy, the UI, and the scripting engine each run in their own separate process. The
scripting engine is just standard python. The UI and scripting engine connect to the proxy and
communicate with it using [capnproto](https://capnproto.org) RPC. It is intended that the UI and the
scripting engine use the same interface to communicate with the proxy -- everything one can do the
other should also be able to do.

In the future, it would probably be nice to make the proxy a library that can be directly embedded
into the UI process.

The capnproto interface should be designed to make maximum use of pipelining, and expose it in an
ergonomic way. The interface should be designed to avoid transmitting entire HTTP messages over the
wire as much as possible.

The project file format is [sqlite](https://github.com/rusqlite/rusqlite). This has good performance
and is battle tested. Using a relational database will be handy for filtering and search, and
normalizing the data will help conserve disk space.

The project file should use both compression and deduplication to conserve disk space.

The compression library is [ZSTD](https://github.com/gyscos/zstd-rs). It gives very high compression
ratios for the cpu cost and supports custom dictionaries which we can leverage on common web content
types.

The TLS library is [openssl](https://github.com/sfackler/rust-openssl). We need to use a library
that can connect to the most websites, especially the ones that are using insecure algorithms.

The proxy will use tokio for asynchronous programming, as that seems to be the best fit for the
use case. This works well with capnproto which also supports asynchronous programming. However, the
sqlite database will live in its own dedicated thread and communicate with the proxy over a channel,
as sqlite does not have good support for asynchronous programming.
