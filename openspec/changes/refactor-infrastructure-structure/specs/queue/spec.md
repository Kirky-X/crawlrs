## ADDED Requirements

### Requirement: Queue Infrastructure Location

The queue module SHALL be located under `src/infrastructure/queue/` as part of the infrastructure layer.

#### Scenario: Queue Module Location
- **WHEN** accessing queue infrastructure
- **THEN** it SHALL be located at `src/infrastructure/queue/`

#### Scenario: Import Path for Queue Client
- **WHEN** importing the queue client
- **THEN** the import path SHALL be `crawlrs::infrastructure::queue::{QueueClient, QueueClientBuilder}`

#### Scenario: Infrastructure Module Structure
- **WHEN** examining the infrastructure module
- **THEN** it SHALL include `queue` as a submodule

### Requirement: Unified Queue API

The queue module SHALL expose only `QueueClient` as the public interface. All low-level queue operations SHALL be encapsulated within the client.

#### Scenario: Public Queue API
- **WHEN** using queue operations
- **THEN** only `QueueClient` and its associated types SHALL be publicly accessible

#### Scenario: Private TaskQueue Trait
- **WHEN** examining the `task_queue` module
- **THEN** the `TaskQueue` trait SHALL be private/internal

#### Scenario: Private TaskScheduler
- **WHEN** examining the `scheduler` module
- **THEN** the `TaskScheduler` struct SHALL be private/internal

### Requirement: Queue Client Usage

All queue operations SHALL be performed through `QueueClient` using its request/response types.

#### Scenario: Enqueue Operation
- **WHEN** enqueuing a task
- **THEN** it SHALL use `QueueClient::enqueue(EnqueueRequest)` method

#### Scenario: Dequeue Operation
- **WHEN** dequeuing a task
- **THEN** it SHALL use `QueueClient::dequeue(DequeueRequest)` method

#### Scenario: Status Update Operation
- **WHEN** updating task status
- **THEN** it SHALL use `QueueClient::update_status(StatusUpdateRequest)` method

#### Scenario: Batch Operations
- **WHEN** performing batch queue operations
- **THEN** it SHALL use `QueueClient::enqueue_batch()` or `QueueClient::dequeue_batch()` methods

#### Scenario: Forbidden Direct Access
- **WHEN** attempting to use `TaskQueue` or `TaskScheduler` directly
- **THEN** the compiler SHALL report an error due to private visibility