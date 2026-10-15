## ADDED Requirements

### Requirement: Real filesystem engine is integration-tested; fault injection is test-only

The real filesystem implementation of the fs trait seam SHALL be covered by an automated integration-test suite. The suite SHALL run file-operation jobs through the real implementation against temporary directories on the real filesystem, and SHALL remove those directories afterwards. It SHALL cover at least:

- copying a file that carries alternate data streams, attributes, and timestamps;
- a junction inside a tree that is moved;
- paths longer than 260 characters;
- overwriting a read-only target;
- a case-only rename;
- cleanup after a cancelled copy.

Tests that depend on Windows- or NTFS-specific behavior SHALL be compiled and run only on Windows. The fake filesystem and its fault-injection API SHALL be compiled only for tests and SHALL NOT be part of the release binary.

#### Scenario: The suite exercises the real implementation on temporary directories
- **WHEN** `cargo test --workspace` runs on Windows
- **THEN** the real-filesystem suite runs copy, move, delete, and rename jobs through the real implementation on temporary directories, asserts the resulting on-disk state, and leaves none of its temporary directories behind

#### Scenario: Windows-specific tests are gated
- **WHEN** the test suite is built for a non-Windows target
- **THEN** the alternate-data-stream, attribute, junction, and long-path tests are not compiled, and the platform-neutral real-filesystem tests still run

#### Scenario: The release binary carries no fault-injection double
- **WHEN** the workspace is built without tests (for example `cargo build --release`)
- **THEN** the fake filesystem and its fault-injection types are not compiled into `filecommand-core`, and no non-test code can refer to them
