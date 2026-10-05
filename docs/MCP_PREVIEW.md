# MCP preview

MCP lets a model use tools provided by another program. It is optional in Nosis Harness.
Normal `nh chat` and `nh tui` sessions do not require an MCP server.
The connection guide and tool-review controls below are unreleased source features.
Use `nh mcp --help` to check which commands your installed version provides.

There are two separate features:

- The **client** connects Nosis to an MCP tool server you configure.
- `nh mcp serve` starts Nosis's own **local server**, which exposes route explanations,
  cost information, receipts, and the existing fleet preview.

## Compatibility

The current implementation targets the stateless HTTP tools interface from MCP `2026-07-28`.
It is a limited preview, not a claim to implement every MCP feature. The older initialize and
session handshake is not implemented. Setting an older version in the configuration does not
make it compatible; use a server that supports the modern interface.

Tools that advertise custom parameter-to-header mappings (`x-mcp-header`) are not supported
yet. Nosis excludes them from the offered tool list and reports a warning. Subscriptions,
incoming client notifications, interactive server input requests, and general schema validation
are also outside this preview. Tool names offered to models must use ASCII letters, digits, hyphens,
or underscores, and fit within 64 characters including the `mcp__<server>__` prefix.

The client accepts JSON replies and bounded server-sent event (SSE) replies. A reply must
belong to the request that Nosis sent. Invalid or mismatched replies stop the call instead
of being treated as successful tool output. A failed tool call is not automatically replayed.
Discovery and tool-list requests may retry once after refreshing an OAuth credential.
Replies are limited to 4 MiB, and the client offers at most 512 tools per server.

Tool results preserve text and structured JSON, including structured-only replies.
When a server returns both, Nosis includes both unless the complete text already
represents the same value. Secret redaction happens before JSON serialization,
including each text block whose entire value is JSON. JSON text keeps its number
values and formatting while secret-bearing strings are replaced.
If redaction makes two distinct object keys identical, Nosis reports that the
affected JSON component was omitted; it does not silently choose a value or replay
the tool call. The adapter applies its output size limit to both results and error
messages. This does not add support for
rendering image, audio or resource content blocks.

The local server checks the protocol metadata and matching HTTP headers before dispatching
tools. An integration that previously sent a bare JSON-RPC body must add the modern request
metadata and headers. See the official [HTTP transport specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http).

## Connect Nosis to another server

From an initialized project, start the guide:

```powershell
nh mcp connect
```

Give the connection a short name, enter the server URL, and choose its authentication
method. Use a URL without a password or API key. For API-key authentication, give
the vault entry's name; the guide never asks you to paste the key into configuration.
It shows the destination before confirmation and saves new connections with
approval required and no enabled tools. Cancelling leaves existing settings intact.
Include `https://`, or `http://` for a literal loopback address; a bare host and
port is not accepted by the guide.

This saves settings only. It makes no server or model request. Follow the printed
key and exact-origin permission instructions if needed, then inspect the connection:

```powershell
nh mcp status
```

Status is offline. It shows local readiness and the next action; it cannot prove
that a server is reachable or accepts your credential. To contact just the server's
tool-list endpoint, replace `local` with your chosen name:

```powershell
nh mcp check local
```

Checking reports available definitions and changes. It does not invoke a tool,
make a model request or update the approved selection. Continue with the explicit
review below before using any remote action.
For manually configured OAuth connections, checking or reviewing can refresh
stored authentication tokens as part of contacting the server.

### Manual configuration

Declare destinations in your user configuration, `~/.nosis/mcp.toml`. On Windows, `~` means
your user profile directory. A project's `.nosis/mcp.toml` can restrict that configuration;
it cannot add trusted destinations or redirect your credentials.
Server and OAuth token URLs must include an explicit `https://` or `http://`
prefix and a host. Malformed forms such as `https:example.invalid/mcp` are
refused before any credential lookup or request, so permission checks and the
HTTP client cannot interpret different destinations. Credential transport still
requires HTTPS or a literal loopback HTTP address.

The guided management commands refuse symlinked configuration paths and report
the path to inspect. Keep an ordinary user-owned configuration file when using
these commands.

Guided saves replace the configuration file while preserving its existing text.
On Unix the replacement has private permissions. Other hard links keep pointing
to the previous file; use manual configuration for a managed setup that depends
on retaining that file identity.

Keep credentials in the OS vault through `nh key add <entry>`, using its hidden prompt.
Do not paste keys into TOML, commands, tool arguments, or HTTP metadata headers. The configured
credential must be approved for the server's exact origin, and the project's send policy
must permit the destination. Repository settings cannot grant those permissions themselves.

Use `trust = "ask"` while evaluating a server. Inspect the requested tool and arguments
before approving. A server's tool descriptions and returned text are untrusted data.

## Review and enable actions

After configuring a server, run this in a terminal, substituting its configured name:

```powershell
nh mcp review local
```

Review the destination and available actions, select the tools you want, and confirm
the selection. Enter without a selection cancels. Connecting or discovering a server
does not enable its tools. Existing configurations also need this one-time review.
The review makes no model request and does not execute the listed tools.

Nosis saves your selection and the reviewed definitions in your user configuration.
Tools with detected secrets, invalid or oversized metadata cannot be enabled.
It checks those definitions again before a tool runs. New tools
start disabled; changed or removed tools cannot run under an older approval. Run
the same review command to inspect changes and update the selection.

Start a new chat or TUI session after changing the selection. Existing sessions
keep the selection they loaded. A session with no enabled tools does not contact
that server at startup. Each allowed action requires another tool-list request;
this adds a network round trip, and a failed check prevents the action.

Enabling a tool does not override project restrictions, credential destinations or
the server's `ask`/`auto`/`block` setting. A server's read-only claim is metadata;
Nosis cannot prove that its remote implementation is read-only or unchanged.
Review state is kept in your user profile, using its inherited permissions on
Windows and private permissions for newly created Unix files. Saving updates
requires a filesystem that supports hard links. If a saved state cannot be read
safely, the error names the file to inspect; Nosis does not overwrite it.

## Disable a connection's tools

```powershell
nh mcp disable local
```

Confirm to clear its enabled selection. This works offline, including when the
server is unavailable or its configuration has been removed. It keeps the saved
definitions and connection settings, so you can review them again later.
Existing chat/TUI sessions retain their startup selection; close them and start
new sessions for the disabled state to apply. Disabling is not an immediate
revocation of tools in already running sessions.

## Start the local Nosis server

From an initialized project directory:

```powershell
nh mcp serve
```

The server prints its local address and a generated access token for the current process.
Configure the connecting client to use that token through its credential mechanism. Keep
the token private. Restarting without a stored token creates a new one.

For a token already stored in the OS vault, use `nh mcp serve --token-entry <entry>`.
Caller-supplied tokens must contain at least 32 bytes. Their values are not printed.

The server binds only to `127.0.0.1`. This restriction is permanent. Do not expose it through
a public tunnel, reverse proxy, or port-forwarding rule. Fleet tools can start model work;
the access token is not a read-only credential.

## Example: connect to the local server

Keep `nh mcp serve` running in one terminal. In a second terminal, store its generated token
using the hidden prompt:

```powershell
nh key add nh-mcp-local
```

Paste only the token value into that prompt, without the `Bearer ` prefix. Run
`nh mcp connect`, name the connection `local`, use the URL printed by the server,
and select API-key authentication with vault entry `nh-mcp-local`. Alternatively,
add this entry to your user `~/.nosis/mcp.toml`, preserving any existing entries:

```toml
[servers.local]
url = "http://127.0.0.1:8765/mcp"
auth = "apikey"
vault_entry = "nh-mcp-local"
trust = "ask"
```

In your user `~/.nosis/law.toml`, approve that credential for this exact local origin:

```toml
[credential.nh-mcp-local]
audience = ["http://127.0.0.1:8765"]
```

Merge these settings into existing tables if present. Use the address printed by the server
if you selected a different port. Project send restrictions still apply.

Run `nh mcp status local` to inspect local readiness, then `nh mcp review local`
and enable only the actions you need. Your next `nh chat`
session offers the selected tools named `mcp__local__...` and asks before each call.
Chat still uses your configured model and its normal billing. When the server restarts with
a new generated token, update the vault entry through the same hidden prompt.

## When a connection fails

| Message or symptom | What to check |
| --- | --- |
| Tools need review or no tools are enabled | Run `nh mcp review <server>` in a terminal and select the actions you need. |
| Tool definition changed | Review the before/after definition with `nh mcp review <server>`. A new name or changed description/schema is not automatically trusted. |
| Unsupported protocol version | The affected server is skipped with a warning; the session can continue. Confirm it supports the modern stateless interface. Changing only the version string cannot add a legacy handshake. |
| Missing or mismatched metadata | Update the integrating client to send matching protocol, method, and tool-name headers and body fields. |
| Credential origin not approved | Verify the intended server address and the credential's trusted origin configuration. |
| Destination blocked by law | Check the user and project send policies. Keep project restrictions in place unless you deliberately change them. |
| Malformed, mismatched, or oversized reply | Stop and inspect the server or proxy. Do not assume that the tool did no work merely because its reply was rejected. |

If a call may have changed external state, inspect that state before manually retrying.
