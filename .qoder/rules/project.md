---
trigger: always_on
alwaysApply: true
---

## 1. Code Style & Formatting
### 1.1 Tooling & CI
- **Mandatory Tooling**: Use `rustfmt` for automatic formatting. Manual adjustments are prohibited.
- **CI Integration**: The CI pipeline must include `cargo fmt --check`.
- **Linting**: Use `clippy` for static code quality analysis and strictly adhere to its recommendations.

### 1.2 Basic Format Rules
- **Indentation**: Use 4 spaces. **Tabs are prohibited**.
- **Line Width**: Maximum **100 characters**.
- **Braces**: Left bracket `{` must be on the same line as the declaration (e.g., `fn foo() {`).
- **Semicolons**: Statements must end with a semicolon. Explicit `return` is suggested for clarity.
- **Commas**: Trailing commas are **mandatory** for multi-line struct/array initializations.

### 1.3 Layout & Spacing (Strict)
- **Vertical Spacing**:
  - 3 empty lines after imports.
  - 3 empty lines before main sections.
  - 2 empty lines before subsections.
- **Segmentation**: Use markers to categorize code blocks: `// === Section Name ===`.

## 2. Naming Conventions

| Element Type | Naming Style | Example |
| :--- | :--- | :--- |
| Variables, Functions, Modules | snake_case | `user_id`, `calculate_total` |
| Structs, Enums, Traits | UpperCamelCase | `UserInfo`, `OrderStatus` |
| Constants, Statics | UPPER_SNAKE_CASE | `MAX_RETRY`, `CONFIG_PATH` |
| Type Parameters | Single Uppercase | `T`, `U` |
| Lifetimes | Single Lowercase | `'a`, `'b` |

### Special Rules
- **Booleans**: Must imply boolean logic (e.g., `is_valid`, `has_permission`).
- **Accessors**: Getters must **not** have a `get_` prefix. Setters must have a `set_` prefix.
- **Async**: Async functions are suggested to have an `_async` suffix.

## 3. Architecture & Organization
### 3.1 Directory Structure
- **Domain-Driven**: Organize modules by **functional domain** (e.g., `api/`, `auth/`, `model/`) rather than code type.
- **Monorepo**: Large projects should be split into sub-crates.
- **Lock Files**: Library projects (`lib`) must **not** upload `Cargo.lock`. Binary projects (`bin`) **must** upload it.

### 3.2 File Organization (Strict Mode)
Within each directory, only the following `.rs` files are permitted, named strictly after Rust keywords:
- `struct.rs`: Contains struct definitions only.
- `enum.rs`: Contains enum definitions only.
- `fn.rs`: Contains free functions only.
- `impl.rs`: Contains implementation blocks (`impl`) only (no type definitions).
- `mod.rs`: Module entry point, handling imports and exports.

### 3.3 Dependency Management
- **Reuse**: Prioritize reusing existing third-party libraries.
- **Review**: New dependencies require justification and security review.
- **Versions**: Always use the latest stable versions.

## 4. Coding Standards
### 4.1 Import Rules
- **Ordering**: Standard Library (`std`) → Third-party Libraries → Local Modules. Separate each group with a blank line.
- **Sorting**: Imports within the same group must be sorted alphabetically.
- **Prohibition**: Wildcard imports (`use module::*`) are strictly forbidden.
```rust
// Example
use std::collections::HashMap;      // Standard Library

use tokio::sync::Mutex;             // Third-party Library

use crate::models::user::User;      // Local Module
```

### 4.2 Documentation
- **Public API**: All public APIs must have clear documentation comments (Purpose, Arguments, Returns, Errors).
- **Impl Blocks**: The top of `impl` blocks must explain the purpose of the implementation.
- **Prohibition**: Comments and empty lines are **strictly prohibited** inside function bodies (except for logic segmentation markers if absolutely necessary).

## 5. Error Handling
- **Propagation**: Must use `Result<T, E>` to explicitly propagate errors.
- **Prohibition**: `.unwrap()` and `.expect()` are **forbidden** (except in tests or strictly proven safe scenarios).
- **Custom Errors**: Prioritize using the `thiserror` crate for custom error types.
- **Clarity**: Provide clear error messages; avoid "silent failures".

## 6. Core Language Features
- **Ownership**: Prioritize immutable references (`&T`). Follow the "Single Owner" principle.
- **Unsafe**: Strictly controlled, minimized scope, and must pass security review.
- **Concurrency**: Understand `Send`/`Sync`. Avoid deadlocks.
- **Async**: Follow `tokio`/`async-std` best practices. **No blocking operations** in async code.

## 7. Performance Optimization
- **Algorithms**: Choose optimal time complexity. Space-for-time is acceptable but avoid memory bloat.
- **Efficiency**: Reduce copying. Leverage **zero-cost abstractions**.
- **Hot Paths**: Design for performance in critical paths.
- **Methodology**: Measure first. Avoid premature optimization.

## 8. Testing & Quality Assurance
### 8.1 Testing Strategy
- **Unit Tests**: Must be located in `#[cfg(test)] mod tests { ... }` within the source file.
- **Integration Tests**: Must be located in the `tests/` directory.
- **Coverage**: High coverage required, including boundary conditions and error paths.
- **Naming**: Test functions must use the `test_` prefix.

### 8.2 Engineering Practices
- **Design Principles**: Follow **SOLID** principles. Use **DDD** for module organization. Use traits for decoupling.
- **CI/CD**: Pipeline must include `cargo fmt`, `cargo clippy`, and `cargo test`.
- **Code Review**: Focus on correctness, readability, maintainability, and security.
