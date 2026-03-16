# stoat

## Streaming OAuth Transformer

A config-driven local reverse proxy that manages OAuth token lifecycle (PKCE flow, storage, refresh) and applies configurable request mutations (header rewriting, query param injection).
Downstream clients talk to stoat as if it were a standard API endpoint with simple key-based auth.
stoat handles the OAuth complexity on their behalf.

All provider-specific details (OAuth endpoints, client IDs, header rewrites, query param additions) live in a user-supplied config file.
The `stoat` binary itself contains no provider-specific code.

## Documentation

### Core Design

- [Architecture](architecture.md) -- Proxy design, request flow, streaming behavior
- [Configuration](configuration.md) -- Config file schema, token storage format

### Project Management

- [Implementation](implementation.md) -- Phase checklist, roadmap
- [Decisions](decisions.md) -- Resolved design decisions and rationale
- [Open Questions](open-questions.md) -- Pending items
