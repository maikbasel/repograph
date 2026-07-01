# Changelog

## [0.4.0](https://github.com/maikbasel/repograph/compare/repograph-v0.3.1...repograph-v0.4.0) - 2026-07-01

### Added

- add repograph-setup management skill + edit command ([#17](https://github.com/maikbasel/repograph/pull/17))
- *(find)* expose retrieval mode in JSON envelope and verify semantic path ([#16](https://github.com/maikbasel/repograph/pull/16))
- cross-repo hybrid search (repograph index + find) ([#12](https://github.com/maikbasel/repograph/pull/12))

### Other

- *(find)* add lexical control to semantic ranking test

## [0.3.1](https://github.com/maikbasel/repograph/compare/repograph-v0.3.0...repograph-v0.3.1) - 2026-06-13

### Fixed

- *(deps)* update rust crate dirs to v6 ([#5](https://github.com/maikbasel/repograph/pull/5))
- *(formula)* shorten repograph description to pass brew audit

## [0.3.0](https://github.com/maikbasel/repograph/compare/repograph-v0.2.1...repograph-v0.3.0) - 2026-06-05

### Added

- *(update)* add `repograph update` command and passive update notifier ([#10](https://github.com/maikbasel/repograph/pull/10))

### Fixed

- *(init)* make generated skill description discoverable
- *(deps)* update rust-dependencies

## [0.2.1](https://github.com/maikbasel/repograph/compare/repograph-v0.2.0...repograph-v0.2.1) - 2026-06-04

### Fixed

- silence info-level logs in interactive (TTY) sessions

### Other

- migrate release automation from release-please to release-plz

## [0.2.0](https://github.com/maikbasel/repograph/compare/v0.1.0...v0.2.0) (2026-06-02)


### Features

* **agent-skills:** write per-agent instruction artifacts on init ([a96850d](https://github.com/maikbasel/repograph/commit/a96850de16bb54b5f72a06b6762107378e926c6b))
* bootstrap workspace and ship registry-core ([ce42d17](https://github.com/maikbasel/repograph/commit/ce42d17abbf6c736035a8bc1d76e7dfc264eda04))
* **context:** aggregate per-repo agent docs into a single payload ([596d6e4](https://github.com/maikbasel/repograph/commit/596d6e4745a9a7b1a41a5ce51ea495e54eeb70b3))
* **init:** add interactive init command ([b94892c](https://github.com/maikbasel/repograph/commit/b94892cb5d4a55efc99cffcb6e73ebdcb352fd56))
* **shell-integration:** add doctor, switch, and completions commands ([ab0b2df](https://github.com/maikbasel/repograph/commit/ab0b2df5d69695f8b39c9c7529243ed4a5612083))
* **status:** add status command with parallel scan and opt-in fetch ([676e7b3](https://github.com/maikbasel/repograph/commit/676e7b313c41c36b51902fb1fcedb82bda415c4c))
* **workspace:** add workspace grouping and filtered listing ([ac19d3e](https://github.com/maikbasel/repograph/commit/ac19d3e1548061318825100f6332ce096efbc055))


### Bug Fixes

* **ci:** apply rustfmt and drop unused deps ([5f742ca](https://github.com/maikbasel/repograph/commit/5f742ca605f1694aca5c1a8a551da83b7150cc8c))
* **path:** strip Windows \\?\ verbatim prefix from canonical paths ([c43e46d](https://github.com/maikbasel/repograph/commit/c43e46d2179dbbd94c9d1dac401c7cce8de86eb8))
* **prompt:** emit path suggestions with `/` separators on all platforms ([cc6c7b5](https://github.com/maikbasel/repograph/commit/cc6c7b5c966af929f4e06fc9c4363eac50ec3ea6))
* **prompt:** honor USERPROFILE for home resolution on Windows ([a3d5263](https://github.com/maikbasel/repograph/commit/a3d5263fb5af36e859a71e98ecdd55fc234c60ca))
