pub(super) fn help_text() -> String {
    "MCP (Model Context Protocol) server management (issue #44).\n\n\
     Usage:\n  \
       /mcp list                       list discovered MCP servers grouped by scope\n  \
       /mcp status                     show live connection status\n  \
       /mcp add <name> [flags]         create a new stdio config (user scope by default)\n  \
       /mcp edit <name> [flags]        update an existing config (auto-detects scope)\n  \
       /mcp remove <name> [--scope=..] delete a config from an editable scope\n  \
       /mcp approve <name...> [--all-project] approve .mcp.json project server(s)\n  \
       /mcp reject <name...>           reject .mcp.json project server(s)\n  \
       /mcp connect <name>             connect an existing server\n  \
       /mcp disconnect <name>          disconnect a connected server\n  \
       /mcp reconnect <name>           reconnect a server\n  \
       /mcp auth start <name>          print OAuth authorization URL\n  \
       /mcp auth complete <name> --code=<code> [--state=<state>] store OAuth token\n  \
       /mcp auth status <name>         show redacted OAuth credential status\n  \
       /mcp auth clear <name>          clear stored OAuth token\n\n\
     Flags for add/edit:\n  \
       --command=<cmd>     executable (stdio transport)\n  \
       --arg=<arg>         positional argument (repeatable)\n  \
       --env=<K=V>         environment variable (repeatable)\n  \
       --url=<url>         URL (sse / streamable-http transports)\n  \
       --transport=stdio|sse|streamable-http   transport kind (default: stdio)\n  \
       --scope=user|project    persistence scope (default: user for add, auto for edit)\n  \
       --oauth-auth-server-metadata-url=<url>  OAuth RFC 8414 metadata URL\n  \
       --oauth-client-id=<id>                  OAuth public client id\n  \
       --oauth-callback-port=<port>            OAuth loopback redirect port\n  \
       --oauth-scope=<scope>                   OAuth scope (repeatable)\n  \
       --browser           tag this server as a browser-MCP server\n\
       --header=K=V         additional non-sensitive HTTP header (repeatable)\n\
       --bearer-token-env-var=ENV   read bearer token from environment variable\n\
       --env-http-header=Header=EnvVar  HTTP header from env var (repeatable)\n\
       --oauth-resource=<url>       OAuth resource parameter (RFC 8707)\n\
       --auth=oauth                 authentication mode (chatgpt is reserved)\n\n\
     Discovery sources (low -> high precedence):\n\
     - plugin-contributed MCP servers\n\
     - ~/.allthecodes/settings.json (user scope)\n\
     - .allthecodes/settings.json in the current project (project scope)\n"
        .to_string()
}
