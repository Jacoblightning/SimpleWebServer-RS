# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (Most of the time :) ).

## [Unreleased]

### Changed:
- Improved argument descriptions in help

## [2.0.0]

### BREAKING CHANGES IN THIS UPDATE:
Another update to the path resolver.
Once again, paths that previously returned 404 may no longer.
(API/ABI should be relatively stable now. Hopefully no more major updates for a while)

### Added:
- Url option handling (ignoring)

## [1.0.0]

### BREAKING CHANGES IN THIS UPDATE:
Update to path resolver.
Paths that previously returned 404 may no longer.

### Added
- New tests
- If a path would return a 404, first try adding .html. If that works, return that instead. (Option to toggle this might be added in the future)

### Changed
- Re-work tests. Use a different server for each test
- Corrected error messages from 404s, 400s, and 500s (Well, technically 400s were always correct as every error used "Bad request")



## [0.2.0]

### Added

- Made logfiles opt-in

### Changed

- Readme update
- Change changelog link

### Fixed

- Quiet and Verbose functionality


## [0.1.4] - 2024-04-16

### Changed:

- Fixed version string to be compatible with crates.io
- Why are licenses so confusing? Relicense under Apache-2.0 OR MIT
- Rename project to avoid conflict. (Sadly)


## [0.1.3] - 2024-04-16

### First notable release.

### Info:
- Releasing on crates.io
- (Somewhat) stable argument interface




[unreleased]: https://github.com/Jacoblightning/SimpleWebServer-RS/compare/v2.0.0...main
[2.0.0]: https://github.com/Jacoblightning/SimpleWebServer-RS/compare/v1.0.0...v2.0.0
[1.0.0]: https://github.com/Jacoblightning/SimpleWebServer-RS/compare/0.2.0...v1.0.0
[0.2.0]: https://github.com/Jacoblightning/SimpleWebServer-RS/compare/v0.1.4...0.2.0
[0.1.4]: https://github.com/Jacoblightning/SimpleWebServer-RS/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/Jacoblightning/SimpleWebServer-RS/releases/tag/v0.1.3
