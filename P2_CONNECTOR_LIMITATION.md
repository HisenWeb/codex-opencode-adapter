# Connector limitation

This draft contains the P2 history hardening helper and tests, but it is not yet wired into the live server request path.

Reason: during this connector-backed edit session, reading/updating existing files such as `server.rs` and `responses_to_chat.rs` was not reliable enough to safely modify the main request path.

Do not merge this branch as final P2 unless the helper is wired into the request path and validated locally.
