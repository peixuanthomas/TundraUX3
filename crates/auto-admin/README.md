# auto-admin

AutoAdmin (AA) handles approval before built-in management operations run, passes passwords, options, and terminal input while they run, and receives the final results.

| Component | Responsibility |
| --- | --- |
| `auto-admin` | Approval state, task input, stop requests, results, and Linux authorization connections |
| Shell | Dialogs, focus, buttons, on-screen keyboard, and page navigation back |
| watchdog | Task creation, panic recording, and thread cleanup |
| platform | Fixed system operations and actual permission checks |

```mermaid
flowchart LR
    Request[Operation request] --> Policy{AA setting}
    Policy -->|Deny| Rejected[Rejected]
    Policy -->|Manual| Approval[User approval]
    Policy -->|Automatic| OS[System authorization]
    Approval --> OS
    OS --> Task[Execute fixed operation]
    Task --> Interaction[Output, password, or options]
    Interaction --> Result[Completed, failed, or outcome unknown]
```

AA policies are `manual`, `automatic`, and `deny`. Automatic approval skips only AA confirmation; system authorization and the program's own prompts still require responses. Passwords are used only for the current input and are not written to logs, arguments, or configuration.

Approval, execution, cancellation, and results are synchronized to the shared task history. Output is archived in chunks; secret input is not archived. Pages and helpers use stable IDs, and read-only queries and previews are not registered as modification tasks.

Homebrew operations also use AA. brew runs as the current macOS user, and native installers receive system passwords for Casks through the task terminal.

## Documentation and validation

- [Approval, input, and termination workflow](docs/workflow.md)
- [Authorization connections and revocation](../platform/docs/authorization.md)
- [Task history](../platform/docs/task-history.md)

```sh
cargo test --locked -p auto-admin
```

Shell's AA tests cover dialogs, focus, password masking, and external control snapshots. Linux authorization interactions are validated in the corresponding test environment.
