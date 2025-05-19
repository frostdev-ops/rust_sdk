# Changelog

## [0.2.5] - Unreleased

### Added
- Generic type parameters for JWT authentication middleware
- Support for custom JWT validation options
- Feature-specific tests to ensure compilation
- Better error handling in JWT authentication with proper thiserror integration
- Type-safe secret retrieval with `typed_secret` module
- Convenience methods for getting typed secrets directly from SecretClient (`get_typed`, `get_string`, etc.)
- Better documentation with comprehensive examples for typed secrets

### Changed
- Restructured JWT auth module into submodules for better organization
- Improved documentation with examples
- More efficient error handling in the JWT middleware
- Disabled default JWT token expiration validation to improve developer experience
- Fixed all Clippy warnings to improve code quality

### Fixed
- Duplicate imports in prelude module
- Missing error type implementation for JWT auth
- Import path issues with jwt_auth and secret_client
- Removed code duplication in AppStateBuilder
- JWT auth tests now properly pass with correct validation settings
- Fixed doctests by correctly annotating Axum router types
- Various Clippy warnings: default_constructed_unit_structs, needless_borrows_for_generic_args, etc.

### Deprecated
- Direct imports from jwt_auth.rs (use jwt_auth submodules instead)
- AppStateBuilder::try_build() (use build() instead)

## [0.2.4] - Previous Release
- Initial stable release with core functionality

## [0.2.6] - 2024-07-29

### Added
- **Model Manager**:
    - Introduced a comprehensive Model Manager for database-agnostic schema definition, DDL generation, and schema synchronization.
    - Includes support for defining models (tables, columns, types, constraints, indexes) in Rust.
    - Generates SQL for SQLite, MySQL, and PostgreSQL.
    - `sdk_integration.rs`: Provides integration with the PyWatt SDK `DatabaseConnection` for applying models and syncing schemas (e.g., `apply_model`, `sync_schema`).
    - `generator.rs`: Core logic for SQL script generation based on model definitions and database adapters.
    - `definitions.rs`: Contains structs and enums for describing database models (`ModelDescriptor`, `ColumnDescriptor`, `DataType`, etc.).
    - `config.rs`: Configuration for the Model Manager.
    - `errors.rs`: Custom error types for the Model Manager.
    - Added `docs/model_manager.md` providing detailed documentation for the Model Manager.
    - `integration_tests.rs`: Added extensive integration tests for the Model Manager, covering create, sync, and migration scenarios for SQLite, MySQL, and PostgreSQL.
- **Database Tool (`database_tool.rs`)**:
    - Added a new command-line interface (CLI) tool `database-tool`.
    - Supports schema generation to SQL files from YAML/JSON model definitions.
    - Allows applying model definitions to a live database.
    - Provides model validation capabilities.
    - Can generate Rust struct code from YAML/JSON model definitions.
- **Caching Module (`src/cache/`)**:
    - Introduced a new caching module.
    - Provides implementations for various caching backends:
        - `in_memory.rs`: In-memory cache.
        - `file.rs`: File-based cache.
        - `redis.rs`: Redis cache client.
        - `memcached.rs`: Memcached cache client.
    - Includes `patterns.rs` for common caching strategies.
    - `tests.rs`: Unit tests for the caching module.
- **Database Module (`src/database/`)**:
    - Enhanced database adapters (`sqlite.rs`, `mysql.rs`, `postgres.rs`) to support the Model Manager functionalities.
    - `mod.rs` and `tests.rs` updated accordingly.

### Fixed
- **Model Manager (`generator.rs`)**:
    - Updated migration script generation to correctly handle `DROP CONSTRAINT` for SQLite by returning an error with appropriate messaging, as SQLite does not directly support this operation.

## [0.2.10] - Unreleased

### Added
- **Message Module (`src/message.rs`)**:
    - Introduced a standardized structure for encoding and decoding messages between modules.
    - Provides a simple API for creating, encoding, decoding, and streaming messages.
    - Supports multiple encoding formats (JSON, binary, base64).
    - Includes both synchronous and asynchronous I/O operations.
    - Features message streaming capabilities for continuous message exchanges.
    - Added comprehensive error handling for various message operations.
    - Includes full documentation and examples in `docs/MESSAGE_MODULE.md`.
    - Added example code in `examples/message_example.rs` demonstrating usage patterns.

## [0.2.5] - Unreleased

### Added
- Generic type parameters for JWT authentication middleware
- Support for custom JWT validation options
- Feature-specific tests to ensure compilation
- Better error handling in JWT authentication with proper thiserror integration
- Type-safe secret retrieval with `typed_secret` module
- Convenience methods for getting typed secrets directly from SecretClient (`get_typed`, `get_string`, etc.)
- Better documentation with comprehensive examples for typed secrets

### Changed
- Restructured JWT auth module into submodules for better organization
- Improved documentation with examples
- More efficient error handling in the JWT middleware
- Disabled default JWT token expiration validation to improve developer experience
- Fixed all Clippy warnings to improve code quality

### Fixed
- Duplicate imports in prelude module
- Missing error type implementation for JWT auth
- Import path issues with jwt_auth and secret_client
- Removed code duplication in AppStateBuilder
- JWT auth tests now properly pass with correct validation settings
- Fixed doctests by correctly annotating Axum router types
- Various Clippy warnings: default_constructed_unit_structs, needless_borrows_for_generic_args, etc.

### Deprecated
- Direct imports from jwt_auth.rs (use jwt_auth submodules instead)
- AppStateBuilder::try_build() (use build() instead) 