# MCP preview

MCP lets a model use tools provided by another program. It is optional in Nosis Harness.
Normal `nh chat` and `nh tui` sessions do not require an MCP server.

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

The local server checks the protocol metadata and matching HTTP headers before dispatching
tools. An integration that previously sent a bare JSON-RPC body must add the modern request
metadata and headers. See the official [HTTP transport specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http).

## Connect Nosis to another server

Declare destinations in your user configuration, `~/.nosis/mcp.toml`. On Windows, `~` means
your user profile directory. A project's `.nosis/mcp.toml` can restrict that configuration;
it cannot add trusted destinations or redirect your credentials.

Keep credentials in the OS vault through `nh key add <entry>`, using its hidden prompt.
Do not paste keys into TOML, commands, tool arguments, or HTTP metadata headers. The configured
credential must be approved for the server's exact origin, and the project's send policy
must permit the destination. Repository settings cannot grant those permissions themselves.

Use `trust = "ask"` while evaluating a server. Inspect the requested tool and arguments
before approving. A server's tool descriptions and returned text are untrusted data.

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

Paste only the token value into that prompt, without the `Bearer ` prefix. Add this entry to
your user `~/.nosis/mcp.toml`, preserving any existing entries:

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

Your next `nh chat` session discovers tools named `mcp__local__...` and asks before each call.
Chat still uses your configured model and its normal billing. When the server restarts with
a new generated token, update the vault entry through the same hidden prompt.

## When a connection fails

| Message or symptom | What to check |
| --- | --- |
| Unsupported protocol version | The affected server is skipped with a warning; the session can continue. Confirm it supports the modern stateless interface. Changing only the version string cannot add a legacy handshake. |
| Missing or mismatched metadata | Update the integrating client to send matching protocol, method, and tool-name headers and body fields. |
| Credential origin not approved | Verify the intended server address and the credential's trusted origin configuration. |
| Destination blocked by law | Check the user and project send policies. Keep project restrictions in place unless you deliberately change them. |
| Malformed, mismatched, or oversized reply | Stop and inspect the server or proxy. Do not assume that the tool did no work merely because its reply was rejected. |

If a call may have changed external state, inspect that state before manually retrying.
