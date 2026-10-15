# LEARNING_ROADMAP.md

## 1. Project Vision

Build a lightweight, reliable, terminal-based Software Engineering Agent (SWE Agent) in Rust.

The Agent should primarily focus on software engineering tasks while providing basic conversational capabilities.

The long-term objective is to create a practical open-source Coding Agent capable of understanding repositories, modifying code, executing development commands, running tests, and completing multi-step coding tasks.

This project is an engineering project, not a Rust learning exercise.

Development should be driven by real product requirements and verifiable engineering milestones.

---

# 2. Development Philosophy

## Engineering First

Prioritize the implementation of real SWE Agent capabilities.

Every major task should contribute to:

- Coding functionality.
- Agent reliability.
- Software architecture.
- Testing.
- Security.
- Maintainability.
- Developer experience.

Avoid unrelated educational exercises.

## Independent Implementation

The developer is responsible for implementing the project's core functionality.

AI should provide:

- Architecture guidance.
- Implementation planning.
- Interface design suggestions.
- Code review.
- Debugging assistance.
- Testing recommendations.

AI should not automatically implement core features unless explicitly requested.

## Learning on Demand

Do not organize development around Rust syntax lessons.

Rust concepts should be explained only when requested or when clarification is necessary to resolve an active engineering problem.

Do not introduce unrelated exercises.

## Incremental Development

Prefer small, verifiable milestones.

Avoid unnecessary architectural rewrites.

Build upon existing functionality whenever practical.

Each phase should produce a working engineering capability.

---

# 3. Current Project Baseline

The repository already contains an initial Agent implementation.

Important existing modules include:

```text
src/
├── agent.rs
├── tools.rs
├── openai_compatible.rs
├── cli.rs
├── config.rs
├── main.rs
└── commands/
    └── chat.rs
```

Existing functionality includes an initial implementation of:

- Agent execution loop.
- Model decision abstraction.
- Tool execution abstraction.
- OpenAI-compatible model communication.
- Basic tool calling.
- Multi-step tool execution.
- Execution iteration limits.
- Initial automated tests.

These components should be preserved and improved incrementally.

Do not rebuild the Agent Runtime from scratch without a concrete engineering reason.

The actual implementation status must be verified against the repository.

`PROJECT_STATUS.md` is responsible for tracking current progress.

This roadmap defines the intended engineering direction, not the verified completion state.

---

# 4. Development Priorities

The project follows these priorities:

| Priority | Area | Description |
|---|---|---|
| P0 | Agent Runtime | Reliable model-tool execution loop |
| P0 | Coding Tools | Repository exploration, file editing, shell execution |
| P0 | Coding Workflow | Complete code modification and verification loop |
| P0 | Security | Workspace protection and controlled tool execution |
| P1 | Context Management | Conversation and execution context |
| P1 | Session Management | Persistent coding sessions |
| P1 | Task Orchestration | Multi-step software engineering tasks |
| P1 | Chat | Basic conversational interaction |
| P1 | Evaluation | Repeatable coding task evaluation |
| P2 | Skills | Reusable task instructions |
| P2 | Hooks | Lifecycle event extensions |
| P3 | MCP | External tool integration |
| P3 | Multi-Agent | Multiple cooperating agents |

P0 capabilities are required for the first practical SWE Agent release.

P1 capabilities improve usability and reliability.

P2 and P3 capabilities are optional future extensions.

Do not prioritize optional extensions before the core coding workflow is functional.

---

# 5. Engineering Roadmap

The project is divided into nine development phases.

Each phase contains:

- Engineering Goal.
- Implementation Tasks.
- Acceptance Criteria.
- Deliverable.

The developer should implement the core functionality independently.

AI should guide implementation according to `AGENTS.md`.

---

# Phase 0 — Project Cleanup and Engineering Baseline

## Engineering Goal

Transform the existing Rust learning-oriented CLI into a focused SWE Agent codebase.

Preserve useful existing functionality while removing unrelated learning exercises.

## Implementation Tasks

### 0.1 Audit Existing Commands

Review the existing CLI commands.

Identify:

- Commands required by the SWE Agent.
- Commands containing reusable engineering functionality.
- Commands created only for Rust learning.
- Tests associated with obsolete commands.

Potential cleanup candidates include:

```text
divide.rs
echo.rs
hello.rs
repeat.rs
sum.rs
wait.rs
parallel_wait.rs
```

Review `fetch.rs` and `read_config.rs` before deciding whether to remove them.

Preserve useful implementations when appropriate.

### 0.2 Clean Up CLI Structure

Remove obsolete commands after confirming that they are not required by core functionality.

Update:

- Command registration.
- CLI parsing.
- Affected tests.
- Relevant documentation.

Do not remove files without developer authorization.

### 0.3 Review Existing Architecture

Inspect:

```text
agent.rs
tools.rs
openai_compatible.rs
commands/chat.rs
config.rs
cli.rs
```

Identify current responsibilities and unnecessary coupling.

Avoid large refactoring during cleanup.

### 0.4 Establish Engineering Baseline

Verify the existing project using appropriate commands:

```bash
cargo fmt --check
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

Document any existing failures.

Do not silently modify unrelated code to satisfy checks.

## Acceptance Criteria

- Obsolete learning commands are removed or explicitly retained with justification.
- Existing core Agent functionality remains operational.
- CLI command registration is consistent.
- Relevant tests pass.
- Current architectural limitations are documented.
- `PROJECT_STATUS.md` accurately reflects the baseline.

## Deliverable

A clean, working SWE Agent foundation.

---

# Phase 1 — Agent Runtime Stabilization

## Engineering Goal

Improve the existing Agent Runtime into a reliable foundation for coding tasks.

This phase builds upon the existing `agent.rs` implementation.

Do not rewrite the runtime unnecessarily.

## Implementation Tasks

### 1.1 Review Agent Message Representation

Inspect the current message model.

Ensure it can represent the information required for:

- User messages.
- Assistant responses.
- Tool calls.
- Tool results.

Preserve compatibility with the existing model integration where practical.

### 1.2 Improve Tool Call Handling

Review how model decisions are converted into tool executions.

Support reliable handling of:

- Valid tool calls.
- Invalid tool names.
- Invalid tool arguments.
- Tool execution failures.
- Model responses without tool calls.

Support multiple tool calls in a single model response when required by the supported API behavior.

### 1.3 Improve Error Handling

Distinguish between:

- Recoverable tool failures.
- Model communication failures.
- Invalid model responses.
- Fatal runtime failures.

Recoverable tool failures should be representable as tool results when appropriate.

Do not terminate the entire Agent automatically for every tool failure.

### 1.4 Execution Limits

Maintain appropriate safeguards:

- Maximum iterations.
- Maximum tool calls where necessary.
- Request timeouts.
- Cancellation handling.

Ensure the Agent cannot continue indefinitely.

### 1.5 Runtime Testing

Use mock model responses to test:

- Final answer generation.
- Single tool execution.
- Multiple sequential tool executions.
- Multiple tool calls in one response.
- Invalid tool arguments.
- Recoverable tool failures.
- Maximum iteration termination.
- Model failures.

## Acceptance Criteria

- Agent execution is deterministic under mocked model responses.
- Tool failures are handled according to their severity.
- Multiple supported tool calls are handled correctly.
- Execution limits prevent uncontrolled loops.
- Existing functionality remains operational.
- Relevant tests pass.

## Deliverable

A stable, testable Agent Runtime.

---

# Phase 2 — Repository Awareness

## Engineering Goal

Enable the Agent to inspect and understand a local source code repository.

The Agent should be able to locate relevant files and retrieve code context.

This phase introduces the first practical coding-oriented tools.

## Implementation Tasks

### 2.1 Workspace Context

Introduce a clear representation of the active workspace.

The Agent should understand which project directory it is operating in.

Avoid relying on arbitrary process working directories.

### 2.2 File Listing Tool

Implement:

`list_files`

Capabilities:

- List files within the authorized workspace.
- Support basic directory traversal.
- Handle invalid paths.
- Avoid excessive output.
- Respect workspace boundaries.

### 2.3 File Reading Tool

Implement:

`read_file`

Capabilities:

- Read source files.
- Handle nonexistent files.
- Handle invalid encodings appropriately.
- Limit output size.
- Respect workspace boundaries.

### 2.4 Code Search Tool

Implement:

`search_code`

Capabilities:

- Search source files.
- Return relevant matches.
- Include useful file locations.
- Handle empty results.
- Limit excessive output.

Prefer a simple, reliable implementation before adding advanced indexing.

### 2.5 Tool Integration

Integrate repository tools into the existing Agent Runtime.

Avoid duplicating tool definitions across unrelated modules.

Introduce a unified tool definition mechanism only when justified by the growing tool set.

### 2.6 Testing

Test:

- Valid file access.
- Missing files.
- Invalid paths.
- Workspace traversal attempts.
- Large files.
- Search results.
- Empty searches.
- Tool error propagation.

## Acceptance Criteria

- Agent can inspect a real Rust repository.
- Agent can locate relevant source files.
- Agent can read requested files.
- Agent can search code.
- Unauthorized workspace access is prevented.
- Tool outputs remain bounded.
- Relevant tests pass.

## Deliverable

A repository-aware Coding Agent capable of code exploration and explanation.

---

# Phase 3 — Code Editing

## Engineering Goal

Enable the Agent to modify source code safely.

The Agent should support controlled file changes and provide inspectable results.

## Implementation Tasks

### 3.1 File Creation and Modification

Introduce controlled file editing capabilities.

Support:

- Creating files.
- Modifying existing files.
- Handling missing target files.
- Detecting invalid operations.

### 3.2 Patch Application

Implement a reliable patch-based editing mechanism.

Prefer targeted modifications over unnecessary full-file replacement.

Handle patch conflicts explicitly.

### 3.3 Diff Inspection

Provide a way to inspect proposed or completed modifications.

Changes should be understandable before they are accepted or committed.

### 3.4 Workspace Protection

Prevent unintended modifications outside the authorized workspace.

Protect unrelated files and existing user changes.

### 3.5 Failure Handling

Handle:

- Invalid patches.
- Conflicting modifications.
- Permission errors.
- Missing files.
- Unexpected file changes.

Avoid silently overwriting user work.

### 3.6 Testing

Test:

- Successful file creation.
- Successful modification.
- Invalid patches.
- Patch conflicts.
- Workspace restrictions.
- Preservation of unrelated content.

## Acceptance Criteria

- Agent can modify source files.
- Changes are inspectable.
- Invalid modifications fail safely.
- Existing unrelated changes are preserved.
- Relevant tests pass.

## Deliverable

A Coding Agent capable of controlled source code modification.

---

# Phase 4 — Shell Execution and Verification

## Engineering Goal

Enable the Agent to execute development commands and inspect their results.

Shell execution is essential for compilation, testing, and debugging workflows.

## Implementation Tasks

### 4.1 Shell Tool

Implement a controlled command execution tool.

Support:

- Command execution.
- Working directory selection within the authorized workspace.
- Standard output.
- Standard error.
- Exit status.
- Execution duration.

### 4.2 Process Management

Handle:

- Command timeouts.
- Process cancellation.
- Nonzero exit codes.
- Excessive output.
- Failed process creation.

### 4.3 Execution Permissions

Introduce an explicit permission policy.

Distinguish between:

- Permitted operations.
- Operations requiring confirmation.
- Prohibited operations.

Do not assume that string-based command filtering provides complete isolation.

### 4.4 Verification Workflow

Allow the Agent to execute development commands such as:

```bash
cargo check
cargo test
cargo fmt --check
```

These are examples, not an unrestricted command whitelist.

### 4.5 Tool Result Representation

Return sufficient information for the Agent to understand:

- Whether the command succeeded.
- What output was produced.
- Why execution failed.
- Whether the failure may be recoverable.

### 4.6 Testing

Test:

- Successful commands.
- Nonzero exit codes.
- Timeouts.
- Cancellation.
- Invalid commands.
- Output limits.
- Permission restrictions.

## Acceptance Criteria

- Agent can execute authorized development commands.
- Command failures are represented correctly.
- Timeouts and cancellation work.
- Dangerous operations require appropriate authorization.
- Execution output is bounded.
- Relevant tests pass.

## Deliverable

A Coding Agent capable of running and verifying code.

---

# Phase 5 — End-to-End Coding Workflow

## Engineering Goal

Integrate repository exploration, code editing, and command execution into a complete software engineering workflow.

This is the project's primary functional milestone.

## Target Workflow

```text
User Request
     |
     v
Agent Runtime
     |
     v
Repository Exploration
     |
     v
Relevant Code Analysis
     |
     v
Code Modification
     |
     v
Build / Test Execution
     |
     v
Result Analysis
     |
     +---- Failure ----+
     |                 |
     |                 v
     |           Code Revision
     |                 |
     |                 v
     |           Reverification
     |                 |
     +-----------------+
     |
     v
Final Summary
```

## Implementation Tasks

### 5.1 Coding Task Execution

Allow users to describe coding tasks through natural language.

Examples:

- Fix a compilation error.
- Add a small feature.
- Modify an existing function.
- Add unit tests.
- Refactor a module.

### 5.2 Tool Coordination

The Agent should select and execute relevant tools.

It should be able to:

- Inspect files.
- Search code.
- Modify code.
- Execute tests.
- Analyze failures.
- Continue when appropriate.

### 5.3 Iterative Repair

Support bounded repair attempts.

Do not allow unlimited retry loops.

Recognize when a task cannot be completed safely.

### 5.4 Completion Reporting

After execution, report:

- Files changed.
- Actions performed.
- Tests executed.
- Verification results.
- Remaining problems.

Do not claim successful completion without verification.

### 5.5 Integration Testing

Create representative coding tasks.

Examples:

- Fix a broken Rust function.
- Repair a failing test.
- Add a CLI argument.
- Modify an existing module.
- Add a new unit test.

Use isolated test repositories or temporary workspaces.

## Acceptance Criteria

- Agent can complete representative coding tasks.
- Agent can inspect and modify multiple relevant files when necessary.
- Agent can run tests and interpret failures.
- Agent can attempt bounded corrections.
- Agent reports incomplete tasks accurately.
- User workspace changes remain protected.
- End-to-end tests demonstrate the workflow.

## Deliverable

**The first functional SWE Agent prototype.**

This milestone is the highest-priority product objective.

---

# Phase 6 — Task Planning and Orchestration

## Engineering Goal

Improve the Agent's ability to execute larger software engineering tasks.

Focus on single-Agent orchestration.

Do not introduce multi-agent collaboration.

## Implementation Tasks

### 6.1 Task Planning

Support decomposition of larger tasks into manageable steps.

Plans should remain adaptable.

Avoid excessive planning for simple tasks.

### 6.2 Task State

Track:

- Pending steps.
- Active step.
- Completed steps.
- Failed steps.
- Blocked steps.

### 6.3 Progress Reporting

Provide useful progress information during execution.

Avoid excessive terminal output.

### 6.4 Failure Recovery

Support controlled recovery from recoverable failures.

Avoid repeating failed actions without changing the approach.

### 6.5 Testing

Test:

- Multi-step task execution.
- Step completion.
- Step failure.
- Recovery behavior.
- Task termination.

## Acceptance Criteria

- Agent can execute multi-step tasks.
- Task progress is observable.
- Failed steps are handled predictably.
- Execution remains bounded.
- Relevant tests pass.

## Deliverable

A SWE Agent capable of structured project-level task execution.

---

# Phase 7 — Context, Session, and Chat

## Engineering Goal

Improve long-running coding sessions and conversational usability.

Coding and Chat should share infrastructure where appropriate.

## Implementation Tasks

### 7.1 Conversation History

Support structured conversation history.

Preserve relevant:

- User messages.
- Assistant messages.
- Tool calls.
- Tool results.

### 7.2 Session Persistence

Support saving and restoring sessions.

Prefer simple local persistence before introducing databases.

### 7.3 Context Management

Manage context growth.

Consider:

- Context window limits.
- Relevant message selection.
- Tool output truncation.
- Token usage.
- Context compaction when necessary.

### 7.4 Chat Experience

Provide a basic conversational mode.

Support:

- Multi-turn conversation.
- Technical questions.
- Code explanation.
- Project-related discussion.

Chat should not require unrestricted coding tool access.

### 7.5 Terminal Experience

Improve usability through:

- Clear execution feedback.
- Readable tool results.
- Useful error messages.
- Session controls.
- Streaming output when supported.

Avoid unnecessary terminal UI complexity.

### 7.6 Testing

Test:

- Session persistence.
- Session restoration.
- Conversation history.
- Context limits.
- Chat behavior.
- Error handling.

## Acceptance Criteria

- Agent supports multi-turn interaction.
- Sessions can be restored.
- Context growth is controlled.
- Chat functionality remains available.
- Coding and Chat modes have appropriate tool permissions.
- Relevant tests pass.

## Deliverable

A practical terminal SWE Agent with persistent sessions and basic Chat.

---

# Phase 8 — Evaluation, Reliability, and Release

## Engineering Goal

Prepare the SWE Agent for public use and open-source presentation.

The goal is a reliable, testable, and maintainable v1.0 release.

## Implementation Tasks

### 8.1 Evaluation Suite

Create repeatable coding tasks.

Measure:

- Task success.
- Failure categories.
- Tool usage.
- Execution time.
- Token consumption when available.

Evaluation results should be reproducible where practical.

### 8.2 Reliability

Review:

- Error handling.
- Execution limits.
- Cancellation.
- Tool failures.
- Context limits.
- Workspace safety.
- Logging and tracing.

### 8.3 Automated Testing

Establish appropriate CI checks.

Include:

- Formatting.
- Compilation.
- Clippy.
- Unit tests.
- Integration tests.

### 8.4 Documentation

Prepare:

- README.
- Installation instructions.
- Configuration instructions.
- Usage examples.
- Architecture overview.
- Known limitations.

Document only implemented functionality.

### 8.5 Release

Prepare an initial public release.

Consider:

- Versioning.
- Release artifacts.
- Installation workflow.
- Changelog.
- License.
- GitHub Releases.

### 8.6 Demonstration

Prepare representative examples demonstrating:

- Repository understanding.
- Code modification.
- Test execution.
- Failure recovery.
- Multi-step task execution.

## Acceptance Criteria

- Core coding workflows are operational.
- Relevant automated tests pass.
- Evaluation tasks are documented.
- Security limitations are documented.
- Installation instructions are usable.
- The project can be demonstrated independently.
- Release artifacts are available.

## Deliverable

**Rust SWE Agent v1.0**

A practical, open-source terminal Coding Agent.

---

# 6. Cross-Cutting Engineering Requirements

The following requirements apply throughout development.

They are not postponed until the final phase.

## Testing

Every major feature should include appropriate tests.

## Security

Workspace boundaries and execution permissions must be considered when introducing tools.

## Error Handling

Failures should be represented clearly and handled according to their severity.

## Observability

Important execution behavior should be inspectable.

Introduce structured logs and traces when needed.

## Documentation

Keep project documentation consistent with implemented functionality.

## Compatibility

Avoid unnecessary breaking changes.

## Maintainability

Prefer clear, simple code over speculative architecture.

---

# 7. Milestone Completion Policy

A phase is not complete merely because code has been written.

Before marking a phase complete, verify:

### Functionality

Does the implementation satisfy the engineering goal?

### Correctness

Does it behave correctly under expected conditions?

### Failure Handling

Are important error cases handled?

### Testing

Do relevant tests pass?

### Integration

Does the feature work with existing components?

### Security

Does the implementation respect relevant safety constraints?

### Documentation

Are important limitations documented?

If a criterion cannot be verified, record the limitation in `PROJECT_STATUS.md`.

Do not falsely mark incomplete functionality as complete.

---

# 8. Development Task Selection

When the developer invokes:

**开始学习**

Follow the workflow defined in `AGENTS.md`.

Use this roadmap to determine the next engineering task.

Task selection should follow these principles:

1. Continue the current milestone.
2. Prioritize unfinished prerequisite functionality.
3. Prefer small, independently verifiable tasks.
4. Avoid unrelated learning exercises.
5. Avoid unnecessary refactoring.
6. Respect existing architecture.
7. Prefer tasks that directly improve coding capability.
8. Do not advance to the next phase without satisfying the current phase's essential acceptance criteria.

A task should be concrete enough for the developer to implement.

Example:

Bad:

"Improve the Agent."

Good:

"Implement read_file with workspace path validation, bounded output, and tests for missing files."

The developer implements the core code independently.

AI provides design guidance and code review.

---

# 9. Project Progress Tracking

`PROJECT_STATUS.md` records actual project progress.

The roadmap should not be modified merely because a task is completed.

Use the status file to record:

- Current milestone.
- Completed functionality.
- Active tasks.
- Next step.
- Architecture notes.
- Open questions.
- Technical debt.

When the developer invokes:

**停止学习**

Follow the workflow defined in `AGENTS.md`.

Update:

- `PROJECT_STATUS.md`
- `journal/YYYY-MM-DD.md`

Do not modify the roadmap unless the developer explicitly requests a roadmap change.

---

# 10. Future Extensions

The following capabilities may be considered after the core SWE Agent is stable.

## Skills

Reusable instructions and workflows for specialized coding tasks.

## Hooks

Lifecycle event handling for tool execution and Agent events.

## MCP

Integration with external tools through the Model Context Protocol.

## Advanced Context Management

More sophisticated context selection and compaction.

## Multi-Agent Collaboration

Specialized agents cooperating on larger tasks.

## IDE Integration

Integration with development environments.

These features are optional.

They must not delay the core SWE Agent release without a clear engineering justification.

---

# 11. Final Engineering Objective

The project should ultimately demonstrate the ability to independently design and maintain a functional SWE Agent.

The completed system should be capable of:

- Understanding source repositories.
- Locating relevant code.
- Modifying source files.
- Executing development commands.
- Running tests.
- Analyzing failures.
- Iteratively repairing code.
- Managing multi-step tasks.
- Maintaining conversational context.
- Reporting execution results.

The project should demonstrate practical engineering skills in:

- Rust.
- Asynchronous programming.
- Agent Runtime design.
- Tool systems.
- Software architecture.
- Error handling.
- Testing.
- Security.
- Observability.
- Engineering judgment.

The developer should remain the primary author of the core implementation.

AI should accelerate engineering progress through guidance, review, and explicitly authorized implementation.

## Final Principle

**Build real SWE Agent functionality.**

**Learn through implementation.**

**Keep the architecture simple.**

**Verify every important milestone.**

**Prioritize a working Coding Agent over unnecessary features.**
