## ADDED Requirements

### Requirement: Credit Ledger Event Store
The system SHALL implement an event-sourced credit ledger for complete auditability and consistency.

The credit ledger SHALL be an append-only store that records:
- Event ID (UUID, auto-generated)
- API Key ID (references the key)
- Event Type (CREDIT_ADD, CREDIT_SPEND, CREDIT_REFUND, CREDIT_ADJUST)
- Amount (positive or negative decimal)
- Balance After (resulting balance)
- Metadata (JSON, optional context)
- Created At (timestamp with millisecond precision)
- Trace ID (for distributed tracing)
- Idempotency Key (unique constraint)

#### Scenario: Credit addition event
- **GIVEN** an API Key with balance of 1000 credits
- **WHEN** a credit addition of 500 is recorded
- **THEN** a `CREDIT_ADD` event SHALL be created
- **AND** the event SHALL have `amount=500`
- **AND** the event SHALL have `balance_after=1500`
- **AND** the event SHALL include metadata with source

#### Scenario: Credit spend event
- **GIVEN** an API Key with balance of 1000 credits
- **WHEN** a spend of 100 credits is recorded
- **THEN** a `CREDIT_SPEND` event SHALL be created
- **AND** the event SHALL have `amount=-100`
- **AND** the event SHALL have `balance_after=900`
- **AND** the event SHALL include metadata with request ID

#### Scenario: Idempotency key prevents duplicate
- **GIVEN** a credit event with idempotency key `req-123`
- **WHEN** the same event is submitted again
- **THEN** the system SHALL reject the duplicate
- **AND** return HTTP 409 Conflict
- **AND** the original event SHALL remain unchanged

#### Scenario: Complete audit trail
- **GIVEN** a series of credit events for an API Key
- **WHEN** querying the ledger for audit
- **THEN** the system SHALL return events in chronological order
- **AND** each event SHALL be immutable
- **AND** the final balance SHALL match the last event's `balance_after`

### Requirement: Materialized Balance View
The system SHALL maintain a materialized view of credit balances for fast queries.

The materialized view SHALL:
- Store the current balance per API Key
- Be asynchronously refreshed from the ledger
- Support near-real-time eventual consistency
- Include last update timestamp

#### Scenario: Fast balance query
- **GIVEN** a materialized balance view exists
- **WHEN** querying balance for an API Key
- **THEN** the response SHALL return in under 10ms
- **AND** the balance SHALL be accurate within 1 second of ledger

#### Scenario: Materialized view refresh
- **GIVEN** new events in the ledger
- **WHEN** the refresh job runs
- **THEN** the materialized view SHALL be updated
- **AND** the update SHALL be concurrent-safe
- **AND** stale reads SHALL not occur during refresh

#### Scenario: Consistency verification
- **GIVEN** a materialized balance of 1000
- **AND** the ledger shows total credits of 1200 and spends of 200
- **WHEN** consistency check executes
- **THEN** the system SHALL detect the discrepancy
- **AND** trigger a full refresh
- **AND** log the inconsistency

### Requirement: Concurrent Credit Operations
The system SHALL handle concurrent credit operations safely using optimistic locking or atomic operations.

#### Scenario: Concurrent spend requests
- **GIVEN** an API Key with balance of 100 credits
- **WHEN** two concurrent spend requests of 100 each arrive
- **THEN** exactly one SHALL succeed
- **AND** the other SHALL receive HTTP 409 Conflict
- **AND** the final balance SHALL be 0

#### Scenario: Optimistic locking
- **GIVEN** a credit operation with expected version
- **AND** the current version differs from expected
- **WHEN** the operation attempts to apply
- **THEN** the system SHALL retry with fresh version
- **AND** retry up to 3 times
- **AND** fail with HTTP 409 after retries exhausted

### Requirement: Credit Overdraft Protection
The system SHALL support configurable overdraft protection for API Keys.

Overdraft options:
- `DISALLOW`: No overdraft allowed (balance cannot go negative)
- `ALLOW_LIMITED`: Limited overdraft up to a configured amount
- `ALLOW_UNLIMITED`: Unlimited overdraft (credit line)

#### Scenario: Spend with no overdraft allowed
- **GIVEN** an API Key with balance of 50 credits
- **AND** overdraft mode is `DISALLOW`
- **WHEN** a spend of 100 credits is requested
- **THEN** the system SHALL return HTTP 402 Payment Required
- **AND** the error code SHALL be `INSUFFICIENT_CREDITS`

#### Scenario: Spend with limited overdraft
- **GIVEN** an API Key with balance of 50 credits
- **AND** overdraft mode is `ALLOW_LIMITED` with limit of 100
- **WHEN** a spend of 100 credits is requested
- **THEN** the spend SHALL be allowed
- **AND** the resulting balance SHALL be -50
- **AND** additional overdraft SHALL be blocked until balance restored

#### Scenario: Overdraft warning
- **GIVEN** an API Key approaching overdraft limit
- **WHEN** balance drops below 20% of overdraft limit
- **THEN** the system SHALL emit a warning event
- **AND** notify configured administrators
- **AND** include remaining overdraft capacity in response

### Requirement: Credit Ledger Queries
The system SHALL provide query APIs for credit history and analytics.

Query capabilities:
- Balance by API Key
- Event history with filters
- Time-range aggregation
- Usage patterns

#### Scenario: Balance query
- **GIVEN** an API Key with events in the ledger
- **WHEN** querying balance via `GET /api/v1/credits/balance`
- **THEN** the response SHALL include current balance
- **AND** include last updated timestamp
- **AND** include overdraft status

#### Scenario: Event history with pagination
- **GIVEN** an API Key with 1000 credit events
- **WHEN** querying history with `page=2, limit=100`
- **THEN** the response SHALL return events 101-200
- **AND** include pagination metadata
- **AND** support sorting by timestamp

#### Scenario: Time-range aggregation
- **GIVEN** credit events spanning multiple days
- **WHEN** querying with `from=2025-01-01, to=2025-01-31`
- **THEN** the response SHALL include daily breakdown
- **AND** include total credits added
- **AND** include total credits spent

## MODIFIED Requirements

### Requirement: Credit Consumption Tracking
**MODIFIED FROM**: Credits are deducted from a simple balance field.

**MODIFIED TO**: Credits are tracked through the event-sourced ledger with materialized views.

The system SHALL implement a comprehensive credit consumption tracking system that:
- Records every credit operation as an immutable event in the ledger
- Maintains a materialized view for fast balance queries
- Supports concurrent operations with optimistic locking
- Provides complete audit trail for all credit movements
- Enables historical analysis and consumption reporting

#### Scenario: Real-time consumption tracking
- **GIVEN** an API Key with sufficient balance
- **WHEN** a billable action is performed
- **THEN** a `CREDIT_SPEND` event SHALL be appended to the ledger
- **AND** the materialized view SHALL be updated asynchronously
- **AND** the response SHALL include current balance

#### Scenario: Historical consumption report
- **GIVEN** credit events over the past month
- **WHEN** generating a consumption report
- **THEN** the report SHALL be derived from the ledger
- **AND** include per-day breakdown
- **AND** include category breakdown if available

#### Scenario: Concurrent consumption tracking
- **GIVEN** an API Key with balance of 500 credits
- **WHEN** two concurrent requests each consume 300 credits
- **THEN** exactly one request SHALL succeed
- **AND** the other SHALL receive `INSUFFICIENT_CREDITS`
- **AND** the final balance SHALL be 200
