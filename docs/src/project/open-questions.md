# Open Questions

## Pending

- [ ] Request body transforms -- Some OAuth endpoints validate request body content (e.g., tool name prefixes, system prompt content).
  Should stoat support configurable body transforms, or should this be handled externally?
  The current design avoids body parsing entirely, which keeps the proxy simple but may be insufficient for some use cases.
- [ ] `run` subcommand -- A `stoat run -- <command>` mode that starts the proxy, sets environment variables (e.g., `SOME_API_BASE=http://127.0.0.1:<port>`), and launches a child process.
  Currently deferred in favor of shell wrapping, but the convenience may be worth the implementation cost.
  The environment variable names would need to be configurable to stay generic.
- [ ] Multi-provider support -- Running multiple upstream proxies from a single `stoat` process (or a single config file with multiple upstream sections).
  Currently one config file = one upstream.
  Running multiple instances is a reasonable workaround.
- [ ] Token refresh margin -- How much time before `expires_at` should the proxy proactively refresh?
  A fixed margin (e.g., 60 seconds) is simple.
  A configurable margin adds flexibility but also config surface area.
- [ ] Config file discovery -- Should stoat look for a default config file (e.g., `~/.config/stoat/config.yaml`) when `--config` is not specified?
  Or should `--config` always be required to keep behavior explicit?
- [ ] Logo -- The concept is a stoat (the animal) centered between bidirectional flow arrows, with different line weights on each side to express transformation.
  The stoat face (front-on, outline style) is the simple form.
  The arrows will be drawn manually in Inkscape: two parallel horizontal lines with half-arrowheads (top-right, bottom-left), thin on one side and bold on the other.

## Deferred

- [ ] Response body transforms -- If request body transforms are added, response body transforms may also be needed (e.g., stripping prefixes added to outgoing requests).
- [ ] Concurrent token refresh -- If multiple requests arrive simultaneously with an expired token, only one refresh should be performed.
  The others should wait for the result.
  This is an implementation detail but worth designing carefully.
- [ ] TLS for the local listener -- The proxy listens on localhost over plain HTTP.
  For non-localhost use (not currently planned), TLS would be needed.
- [ ] Health check endpoint -- A `GET /health` or similar endpoint on the proxy for monitoring.
- [ ] Metrics -- Request counts, latencies, token refresh counts.
  Useful for debugging but adds complexity.
