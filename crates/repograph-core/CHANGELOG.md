# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/maikbasel/repograph/compare/repograph-core-v0.4.0...repograph-core-v0.5.0) - 2026-07-23

### Added

- *(doctor)* add --fix and concrete refresh guidance for skill artifacts
- *(init)* splice an always-loaded repograph pointer into CLAUDE.md
- *(find)* auto-refresh stale repos before search

### Other

- satisfy clippy::manual_assert_eq in git test
- apply rustfmt to new code
- uppercase README.md title
- *(readme)* add crates.io and GitHub download badges

## [0.4.0](https://github.com/maikbasel/repograph/compare/repograph-core-v0.3.1...repograph-core-v0.4.0) - 2026-07-01

### Added

- add repograph-setup management skill + edit command ([#17](https://github.com/maikbasel/repograph/pull/17))
- *(find)* expose retrieval mode in JSON envelope and verify semantic path ([#16](https://github.com/maikbasel/repograph/pull/16))
- cross-repo hybrid search (repograph index + find) ([#12](https://github.com/maikbasel/repograph/pull/12))

## [0.3.1](https://github.com/maikbasel/repograph/compare/repograph-core-v0.3.0...repograph-core-v0.3.1) - 2026-06-13

### Fixed

- *(deps)* update rust crate dirs to v6 ([#5](https://github.com/maikbasel/repograph/pull/5))

### Other

- *(release)* stop tagging repograph-core to fix failing dist run

## [0.3.0](https://github.com/maikbasel/repograph/compare/repograph-core-v0.2.1...repograph-core-v0.3.0) - 2026-06-05

### Added

- *(update)* add `repograph update` command and passive update notifier ([#10](https://github.com/maikbasel/repograph/pull/10))

### Fixed

- *(init)* make generated skill description discoverable

### Other

- Merge origin/master into renovate/rust-dependencies
- Merge pull request #6 from maikbasel/renovate/toml-1.x

## [0.2.1](https://github.com/maikbasel/repograph/compare/repograph-core-v0.2.0...repograph-core-v0.2.1) - 2026-06-04

### Other

- correct JSON envelope shapes in agent artifact body
