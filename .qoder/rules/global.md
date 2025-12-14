---
trigger: always_on
alwaysApply: true
---

# Software Development Project Rules & Principles

## Part 1: Core Collaboration Principles & Communication Protocol

### 1. Core Principles

These are the supreme guidelines for all actions, designed to establish an efficient, precise, and unambiguous human-AI collaboration model.

1. **No Laziness, Pursue Excellence**: All outputs must be complete, detailed, and of high quality. Refuse any form of simplification or placeholders; every output should be a final, independently usable version.
2. **Deep Alignment, Eliminate Ambiguity**: Before taking action, you must ensure a complete and correct understanding of the user's intent. This is the highest priority. If instructions are ambiguous, you **must** proactively seek clarification.
3. **Proactive Feedback, Closed-Loop Communication**: At every critical juncture of a task, you must proactively communicate, report progress, and request confirmation to ensure the process is transparent and the direction is aligned.
4. **Incremental Progress over Big Bangs**: Prioritize small, safe code changes that compile and pass tests.
5. **No Mocks or Fake Data**: Strictly prohibit the use of mock data, placeholders, or fake implementations. All functionality must use real implementations, and all tests must use real data or realistic scenarios.
6. **Use Context 7 to Find Latest Library Documentation**: Before using any library or framework, you must use the web_search tool to find the latest version of official documentation, ensuring the use of current APIs and best practices.
7. **Use Sequential Thinking for Deep Analysis**: For complex problems, you must employ sequential thinking: break down problems step by step, document the thinking process, verify each reasoning step, and consider all edge cases before concluding.
8. **Project Memory Management**: Maintain a living document of project-specific knowledge
   - Append critical operational commands (e.g., `source .venv/bin/activate`) to `.trae/rules/project_rules.md`
   - Document environment setup steps, special configurations, and gotchas
   - Include project-specific conventions not covered in general standards
   - Update immediately when discovering new patterns or workarounds
9. **Leverage AI Agents & MCP Tools**: For complex or specialized tasks
   - Use MCP (Model Context Protocol) servers when available for domain-specific operations
   - Delegate to specialized AI agents for their areas of expertise
   - Combine multiple tools strategically to solve multi-faceted problems
   - Document which agents/tools were used and why in commit messages

### 2. Interaction & Communication Protocol

#### **[Primary Rule] Intent Confirmation & Clarification**

Before executing any complex task or instruction, you **must** initiate the "Intent Confirmation" process:

a. **Restate the Request**: In your own words, clearly restate your understanding of the instructions and the final objective

b. **Provide an Example**: Offer a short, concrete example to demonstrate the style or core content of the output you are about to generate

c. **Request Confirmation**: Explicitly ask: "Is my understanding correct? Does this example meet your expectations? May I proceed with execution?"

#### **Continuous Feedback & Iteration**

- After completing a significant stage in the plan, you should proactively report progress and ask: "Does the completed portion meet the requirements? Do we need to make any adjustments?"
- The task is only considered officially complete when the user explicitly states "done," "that's all," or "no further interaction needed"

## Part 2: Development Process & Execution Framework

### 1. Planning & Staging

Complex work must be broken down into 3-5 clear stages and documented in `IMPLEMENTATION_PLAN.md`.

#### **Thinking & Analysis**

- **Understand the Underlying Intent**: Before creating a plan, first consider the **root cause** and **ultimate goal** of the request
- **Expansive Thinking**: Actively consider potential edge cases, supplementary scenarios, or risks, and reflect them in the plan

#### **Plan Formulation**

```markdown
## Stage N: [Stage Name]
**Goal**: [A specific, deliverable outcome]
**Success Criteria**: [Testable and verifiable results]
**Tests**: [Specific test cases to be written or modified]
**Status**: [Not Started | In Progress | Complete]
```

#### **Plan Confirmation**

Before beginning implementation, you **must** submit this plan and follow the "Intent Confirmation" protocol to discuss it

### 2. Implementation Flow

1. **Understand** - Deeply study existing patterns and conventions in the codebase
2. **Test** - Write a failing test first (Red)
3. **Implement** - Write the minimum code required to make the test pass (Green)
4. **Refactor** - Clean up and optimize the code while ensuring all tests continue to pass
5. **Commit** - Write a clear commit message that links to the implementation plan
6. **Report** - Upon completing each stage, proactively report its completion and ask for the next instruction

### 3. Proactive Problem-Solving Framework

**Critical Rule**: Make a maximum of 3 attempts on a single issue. If it remains unresolved, you **must STOP** and initiate the following process:

#### **Document Failed Attempts**

- What approaches did you try?
- What specific error messages did you encounter?
- What is your analysis of why it failed?

#### **Research Alternatives**

- Find 2-3 implementations of similar features for reference
- Note the different approaches they used

#### **Question the Fundamentals**

- Is this the right level of abstraction?
- Can the problem be broken down into smaller parts?
- Is there a simpler overall approach?

#### **Request a Decision**

- Summarize the above analysis (failed attempts, alternatives, fundamental questions) and present it to the user to request a decision

## Part 3: Technical & Quality Standards

### 1. Architecture Principles

- **Composition over Inheritance** - Favor dependency injection
- **Interfaces over Singletons** - Ensure testability and flexibility
- **Explicit over Implicit** - Maintain clear data flow and dependencies
- **Test-Driven When Possible** - Never disable tests; fix them

### 2. SOLID Design Principles (Mandatory)

#### **S - Single Responsibility Principle**

- Each class/module should be responsible for one clearly defined function
- Modifying a function should only require changing one class
- Class names should clearly express their single responsibility

#### **O - Open/Closed Principle**

- Open for extension: implement new features through inheritance, composition, interfaces
- Closed for modification: existing code should not be modified due to new requirements
- Use abstraction, polymorphism, dependency injection to achieve extensibility

#### **L - Liskov Substitution Principle**

- Subclasses must be able to replace parent classes without breaking program correctness
- Subclasses should not strengthen preconditions or weaken postconditions
- Avoid subclasses throwing exceptions not declared by parent classes

#### **I - Interface Segregation Principle**

- Interfaces should be small and focused, avoiding "fat interfaces"
- Clients should not be forced to depend on methods they don't use
- Prefer multiple specialized interfaces over a single general interface

#### **D - Dependency Inversion Principle**

- High-level modules and low-level modules should both depend on abstractions
- Abstractions should not depend on details; details should depend on abstractions
- Use dependency injection frameworks to manage object dependencies

### 3. Three Code Quality Principles

#### **DRY - Don't Repeat Yourself**

- **Zero tolerance for duplicate code**
- Same logic appearing twice must be refactored immediately
- Extract common methods, utility classes, base classes
- Use configuration files instead of hardcoded repeated values
- Knowledge should have a single authoritative representation in the system

#### **KISS - Keep It Simple, Stupid**

- **Simple over complex, clear over clever**
- Avoid over-engineering and premature optimization
- Prioritize standard libraries and mature frameworks
- Code should be easy to understand, allowing newcomers to quickly onboard
- Break complex problems into simple sub-problems

#### **YAGNI - You Aren't Gonna Need It**

- **Only implement features currently needed**
- Don't code based on "might" or "future" requirements
- Avoid predictive design and redundant abstraction layers
- Refactor when requirements change; don't guess in advance
- Every line of code should have clear current value

### 4. Code Quality & Completeness

#### **Every Commit Must**

- Compile successfully
- Pass all existing tests
- Include necessary tests for new functionality
- Adhere to project formatting and linting standards

#### **Principle of Completeness**

- Strictly forbid the use of placeholders like "as above," "...", etc. Every output must be the complete, final version that can be used independently
- Even for minor modifications, you must output the **entire file content** after the change, not just the modified snippet
- **No Mocks or Fake Data**:
  - Do not use mock objects, stubs, fakes, or other test doubles (except when required by the testing framework itself)
  - All functionality must use real implementations
  - Test data must be representative of real scenarios
  - Prohibit placeholder data like "TODO", "sample data", "test data"

#### **Principle of Consistency**

- When modifying any part of the code, you must automatically check and ensure that all related content (e.g., documentation, tests, variable names, logical dependencies) is updated in sync to maintain global consistency

#### **Deep Thinking Principle**

- **Sequential Thinking**: For complex problems, you must:
  - Break problems down into logical steps
  - Document each thinking stage progressively
  - Verify the correctness of each reasoning step
  - Consider all edge cases before drawing conclusions
  - Record the thinking process for traceability and review

#### **Documentation Query Principle**

- Before using any library, framework, or tool, you **must** use the web_search tool to find:
  - The latest version of official documentation
  - Latest API changes and deprecation warnings
  - Community-recommended best practices
  - Known issues and solutions
- Do not rely on outdated information from training data

### 5. Test-Driven Development (TDD)

#### **Red-Green-Refactor Cycle**

- **Red Phase**: Write a failing unit test first, clarifying requirements and expected behavior
- **Green Phase**: Write the minimum code to make the test pass, without considering optimization
- **Refactor Phase**: Optimize code structure under test protection, eliminating duplication and code smells

#### **Testing Requirements**

- All new features must have test coverage first
- Tests should be fast, independent, and repeatable
- Unit test coverage target: core business logic ≥ 80%
- **Never Disable Tests** - When tests fail, fix the code or fix the test, never disable the test

### 6. Behavior-Driven Development (BDD)

- Use Gherkin syntax to write acceptance tests
- Follow Given-When-Then structure to describe scenarios
- Collaborate with product managers/business people to define behavior specifications
- Test cases should use business domain language, readable by non-technical people

### 7. Error Handling

- Fail fast with descriptive messages
- Include context that is helpful for debugging
- Handle errors at the appropriate level
- Never silently swallow exceptions

## Part 4: Domain-Driven Design (DDD) Specifications

### 1. Strategic Design

#### **Define Clear Bounded Contexts**

- Each context has independent models and ubiquitous language
- Contexts communicate through anti-corruption layers
- Clarify context mapping relationships (partnership, customer-supplier, etc.)

#### **Establish Ubiquitous Language**

- Technical teams and business experts use the same terminology
- Code naming directly reflects business concepts
- Documentation, discussions, and code maintain language consistency

### 2. Tactical Design

- **Entity**: Domain objects with unique identifiers
- **Value Object**: Immutable descriptive objects without identity
- **Aggregate**: Clusters of entities and value objects ensuring consistency
- **Aggregate Root**: The sole entry point for external access to aggregates
- **Repository**: Encapsulates persistence logic, provides collection-like interface
- **Domain Service**: Domain logic that doesn't belong to any entity
- **Domain Event**: Records important business facts in the domain

### 3. Layered Architecture

```
Presentation Layer
    ↓
Application Layer - Use case orchestration
    ↓
Domain Layer - Core business logic
    ↓
Infrastructure Layer - Technical implementation
```

## Part 5: Decisions, Integration & Quality Gates

### 1. Decision Framework

When multiple valid approaches exist, choose based on this order of priority:

1. **Testability** - Can I easily test this?
2. **Readability** - Will someone understand this in 6 months?
3. **Consistency** - Does this match existing project patterns?
4. **Simplicity** - Is this the simplest solution that works?
5. **Reversibility** - How hard will this be to change later?

### 2. Project Integration

#### **Learning the Codebase**

- Find and analyze 3 similar features or components
- Identify and follow common patterns and conventions

#### **Tooling**

- You **must** use the project's existing build system, test framework, and formatter/linter configurations
- Do not introduce new tools without strong justification and approval

### 3. Definition of Done

- [ ] Tests are written and passing
- [ ] Code follows project conventions
- [ ] No linter or formatter warnings
- [ ] Commit messages are clear and descriptive
- [ ] Implementation matches the plan
- [ ] No TODOs without an associated issue number
- [ ] No mocks or fake data (all implementations are real)

## Part 6: Coding Standards

### 1. Naming Conventions

- **Class Names**: PascalCase, nouns, e.g., `UserRepository`
- **Method Names**: camelCase, verb-led, e.g., `calculateTotalPrice()`
- **Variable Names**: camelCase, meaningful names, avoid abbreviations
- **Constants**: UPPER_SNAKE_CASE, e.g., `MAX_RETRY_COUNT`
- **Boolean Variables**: is/has/can prefix, e.g., `isActive`

### 2. Method Design

- Single method should not exceed 20 lines (special cases excepted)
- No more than 3 parameters; encapsulate multiple parameters in objects
- Avoid output parameters; use return values
- Avoid side effects; prefer pure functions

### 3. Comment Requirements

- Code should be self-explanatory; reduce reliance on comments
- Public APIs must have documentation comments
- Complex algorithms must comment on approach and time complexity
- Don't comment "what" (code shows that), comment "why"

## Part 7: Version Control Standards

### 1. Git Workflow

- **Main Branches**: `main` (production), `develop` (development)
- **Feature Branches**: `feature/feature-name`
- **Fix Branches**: `bugfix/issue-description` or `hotfix/urgent-fix`
- Merge using Pull Requests with at least 1 code review

### 2. Commit Conventions

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Type Categories:**

- `feat`: New feature
- `fix`: Bug fix
- `refactor`: Refactoring (no functionality change)
- `test`: Add or modify tests
- `docs`: Documentation update
- `style`: Code formatting adjustment
- `perf`: Performance optimization
- `chore`: Build tool or dependency update

**Example:**

```
feat(user-service): add email verification for user registration

- Implement email verification code sending service
- Add verification code validation logic
- Complete user registration flow unit tests

Closes #123
```

## Part 8: Code Review Checklist

### Functional Correctness

- [ ] Implementation meets requirement specifications
- [ ] Boundary conditions and exception handling complete
- [ ] Unit tests cover all branch paths
- [ ] Integration tests verify end-to-end scenarios
- [ ] No mocks or fake data (all implementations are real)

### Design Quality

- [ ] Follows SOLID principles
- [ ] No duplicate code (DRY)
- [ ] Reasonable complexity (KISS)
- [ ] No over-engineering (YAGNI)
- [ ] Clear responsibility separation

### Code Readability

- [ ] Naming clearly expresses intent
- [ ] Code structure is logically clear
- [ ] Complex logic has explanatory comments
- [ ] Format conforms to team standards

### Security

- [ ] Adequate input validation
- [ ] No SQL injection risks
- [ ] Sensitive information not hardcoded
- [ ] Complete permission validation

### Performance

- [ ] No obvious performance bottlenecks
- [ ] Database query optimization (indexing, N+1 problem)
- [ ] Resources properly released
- [ ] Reasonable cache usage

## Part 9: Continuous Integration/Continuous Deployment (CI/CD)

### Automation Requirements

- Every commit triggers automatic build
- All tests must pass before merging
- Code quality gates:
  - New code coverage ≥ 80%
  - Critical code smells = 0
  - Technical debt increment controlled within reasonable range

### Deployment Process

1. **Development Environment**: Auto-deploy `develop` branch
2. **Testing Environment**: QA manually triggers deployment
3. **Pre-production Environment**: Production-like configuration, regression testing
4. **Production Environment**: Requires approval, supports blue-green/canary deployment

## Part 10: Documentation Requirements

### Required Documentation

- **README.md**: Project overview, quick start, architecture description
- **ARCHITECTURE.md**: Architecture design, technology selection, module division
- **API.md**: Interface documentation (or use Swagger/OpenAPI)
- **CHANGELOG.md**: Version change log
- **IMPLEMENTATION_PLAN.md**: Implementation plan and stage status
- **Domain Model Diagram**: Core business entity relationships

### Optional Documentation

- Deployment and operations manual
- Troubleshooting guide
- Performance optimization records
- Architecture Decision Records (ADR)

## Part 11: Absolute Rules

### **NEVER**

- Use `--no-verify` or similar flags to bypass commit hooks
- Disable tests instead of fixing them
- Commit code that does not compile
- Make assumptions—verify them by studying the existing code
- **Use mocks, stubs, fakes, or other test doubles (except when required by testing framework)**
- **Use placeholders, fake data, or sample data**
- **Code based on outdated documentation or training data - must search for latest documentation**

### **ALWAYS**

- Commit working code incrementally
- Update the status in `IMPLEMENTATION_PLAN.md` as you progress
- Learn from existing implementations
- Stop after 3 failed attempts and reassess
- **Use web_search to find latest official documentation**
- **Use Sequential Thinking for complex problems**
- **Use real implementations and real data**
- **Output complete file content without placeholders**
- **Scope Discipline**: Implement ONLY what is explicitly requested
  - No "nice-to-have" features without approval
  - No preemptive refactoring of unrelated code
  - No architectural changes beyond task scope
  - When unsure if something is in scope, ASK first

## Part 12: Principle Conflict Resolution

When different principles conflict, priority order:

1. **Security** > All principles
2. **Correctness** > Performance optimization
3. **Maintainability** > Performance optimization
4. **SOLID** > Premature optimization
5. **Business Requirements** > Technical perfectionism

Principle conflicts require technical lead decision and documentation in ADR.

## Part 13: Violation Handling

- **Minor Violations** (naming non-compliance, missing comments): Code review points out, merge after correction
- **Moderate Violations** (insufficient test coverage, duplicate code): Must be fixed before merging
- **Severe Violations** (architecture破坏, security vulnerabilities, using mocks/fake data, not searching for latest documentation): Reject merge, redesign required

## Part 14: Performance Benchmarks

- API response time P95 < 200ms
- Database query P99 < 100ms
- Page first paint < 2s
- System availability ≥ 99.9%

## Part 15: Code Review Frequency

- **Daily**: Internal team peer review
- **Weekly**: Technical debt identification and planning
- **Monthly**: Architecture review, technical approach alignment