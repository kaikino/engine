# Security

## Reporting

Please report vulnerabilities privately through GitHub's
[security advisory form](https://github.com/kaikino/engine/security/advisories/new)
rather than a public issue. You will get an acknowledgement within a week.

## Scope

KAGAMI listens on a UDP port during netplay and parses datagrams from
peers. The parser (`kagami::net::wire`) is bounds-checked and rejects
malformed messages; a hostile peer can at most stall or desync its own
match. Replay files are parsed with bounds checks and reject truncated or
oversized data.

The runtime is `#![forbid(unsafe_code)]` throughout the workspace. The
console crate has no dependencies; the runtime's dependencies are tracked
by Dependabot.

## Supported versions

Only the latest release receives fixes.
