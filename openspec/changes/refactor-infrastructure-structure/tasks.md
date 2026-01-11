## 1. Directory Structure Changes

- [ ] 1.1 Create `src/infrastructure/database/repositories/` directory
- [ ] 1.2 Move all files from `src/infrastructure/repositories/` to `src/infrastructure/database/repositories/`
- [ ] 1.3 Create `src/infrastructure/queue/` directory
- [ ] 1.4 Move all files from `src/queue/` to `src/infrastructure/queue/`
- [ ] 1.5 Remove old empty directories

## 2. Module Declarations

- [ ] 2.1 Update `src/infrastructure/database/mod.rs` to include `repositories` submodule
- [ ] 2.2 Update `src/infrastructure/mod.rs` to include `queue` submodule
- [ ] 2.3 Remove `repositories` from `src/infrastructure/mod.rs`
- [ ] 2.4 Remove `queue` from `src/lib.rs`

## 3. Import Updates

- [ ] 3.1 Update imports in `src/main.rs`
- [ ] 3.2 Update imports in `src/workers/` modules
- [ ] 3.3 Update imports in `src/presentation/handlers/`
- [ ] 3.4 Update imports in `src/application/`
- [ ] 3.5 Update imports in all other files using `queue` or `repositories`

## 4. Queue API Restriction

- [ ] 4.1 Make `TaskQueue` trait private in `src/infrastructure/queue/task_queue.rs`
- [ ] 4.2 Make `TaskScheduler` struct private in `src/infrastructure/queue/scheduler.rs`
- [ ] 4.3 Ensure `QueueClient` is the only public item exported from `src/infrastructure/queue/mod.rs`
- [ ] 4.4 Verify no code directly uses `TaskQueue` or `TaskScheduler`

## 5. Database Consolidation

- [ ] 5.1 Update repository implementations to use new module path
- [ ] 5.2 Verify all database-related code is under `src/infrastructure/database/`
- [ ] 5.3 Ensure entities remain at `src/infrastructure/database/entities/`

## 6. Testing

- [ ] 6.1 Run `cargo check` to verify compilation
- [ ] 6.2 Run `cargo test` to ensure tests pass
- [ ] 6.3 Run integration tests
- [ ] 6.4 Verify all imports resolve correctly

## 7. Documentation

- [ ] 7.1 Update module-level documentation
- [ ] 7.2 Update code examples in comments
- [ ] 7.3 Update IFLOW.md with new module structure
- [ ] 7.4 Verify README examples still work